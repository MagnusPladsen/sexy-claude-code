use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::claude::events::{parse_event, StreamEvent};

pub struct ClaudeProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
}

impl ClaudeProcess {
    /// Spawn claude in print mode with stream-json I/O.
    /// Returns the process handle and a receiver for parsed events.
    pub fn spawn(command: &str) -> Result<(Self, mpsc::UnboundedReceiver<StreamEvent>)> {
        Self::spawn_inner(command, None)
    }

    /// Spawn claude resuming an existing session.
    pub fn spawn_with_resume(
        command: &str,
        session_id: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<StreamEvent>)> {
        Self::spawn_inner(command, Some(session_id))
    }

    fn spawn_inner(
        command: &str,
        resume_session_id: Option<&str>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<StreamEvent>)> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let (program, args) = parts.split_first().context("Empty command")?;

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.args([
            "-p",
            "--output-format", "stream-json",
            "--input-format", "stream-json",
            "--verbose",
            "--include-partial-messages",
        ]);
        if let Some(session_id) = resume_session_id {
            cmd.args(["--resume", session_id]);
        }
        // Prevent "cannot run inside another Claude Code session" error
        cmd.env_remove("CLAUDECODE");
        cmd.env_remove("CLAUDE_CODE_ENTRYPOINT");
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().with_context(|| format!("Failed to spawn '{}'", command))?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn stdout reader task — reads NDJSON lines and parses them
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let event = parse_event(&line);
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        Ok((Self { child, stdin }, rx))
    }

    /// Send a user message as a stream-json input event.
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        let event = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": text,
            },
        });
        let mut line = serde_json::to_string(&event)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("Failed to write to claude stdin")?;
        self.stdin
            .flush()
            .await
            .context("Failed to flush claude stdin")?;
        Ok(())
    }

    /// Check if the process is still running.
    #[allow(dead_code)]
    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(self.child.try_wait()?)
    }

    /// Kill the child process.
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.context("Failed to kill claude process")
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_nonexistent_command_fails() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = ClaudeProcess::spawn("nonexistent_command_xyz_12345");
            assert!(result.is_err());
        });
    }
}
