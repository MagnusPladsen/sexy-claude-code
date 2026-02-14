use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl PtyProcess {
    pub fn spawn(command: &str, cols: u16, rows: u16) -> Result<Self> {
        Self::spawn_with_env(command, cols, rows, HashMap::new())
    }

    pub fn spawn_with_env(
        command: &str,
        cols: u16,
        rows: u16,
        extra_env: HashMap<String, String>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        let parts: Vec<&str> = command.split_whitespace().collect();
        let (program, args) = parts
            .split_first()
            .context("Empty command")?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);

        // Inherit environment
        for (key, val) in std::env::vars() {
            cmd.env(key, val);
        }

        // Apply extra env vars (overrides inherited ones)
        for (key, val) in &extra_env {
            cmd.env(key, val);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn child process")?;

        // Drop slave — we only interact through master
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        Ok(Self {
            master: pair.master,
            child,
            writer: Arc::new(Mutex::new(writer)),
        })
    }

    pub fn take_reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master
            .try_clone_reader()
            .context("Failed to clone PTY reader")
    }

    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        writer.write_all(data).context("Failed to write to PTY")?;
        writer.flush().context("Failed to flush PTY writer")?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to resize PTY")
    }

    pub fn is_alive(&mut self) -> bool {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .is_none()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.kill();
    }
}
