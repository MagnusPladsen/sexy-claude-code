# Stream JSON Rewrite + Header Font Fix — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the PTY-based Claude Code wrapper with stream-json I/O so we render conversations ourselves (no Claude Code TUI leaking through), and fix the header font to be readable.

**Architecture:** Spawn `claude -p --output-format stream-json --input-format stream-json` as a child process via `tokio::process::Command`. Read NDJSON events from stdout, parse into a conversation model, render with custom ratatui widgets. User input goes through our InputEditor and is sent as stream-json events to stdin.

**Tech Stack:** Rust, tokio (async process + channels), serde_json (NDJSON parsing), ratatui 0.29 (TUI rendering)

---

### Task 1: Add `serde_json` dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add serde_json to dependencies**

In `Cargo.toml`, add `serde_json` to `[dependencies]`:

```toml
serde_json = "1"
```

The full dependencies section should be:

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
vt100 = "0.15"
portable-pty = "0.8"
tokio = { version = "1", features = ["full"] }
toml = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
dirs = "5"
anyhow = "1"
```

Note: We keep `vt100` and `portable-pty` for now to avoid breaking existing tests. We'll clean them up later.

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles without errors.

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add serde_json dependency for stream-json parsing"
```

---

### Task 2: Create `src/claude/events.rs` — NDJSON event types

**Files:**
- Create: `src/claude/mod.rs`
- Create: `src/claude/events.rs`
- Test: inline `#[cfg(test)]` module

This is the core data model. Claude CLI's `--output-format stream-json` emits NDJSON following the Anthropic streaming protocol:

```
{"type":"message_start","message":{"id":"msg_...","role":"assistant","content":[],"model":"...","usage":{...}}}
{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}
{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
{"type":"content_block_stop","index":0}
{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}
{"type":"message_stop"}
```

There are also tool_use blocks and result events. We need to handle the common ones.

**Step 1: Create module file**

Create `src/claude/mod.rs`:

```rust
pub mod events;
pub mod process;
pub mod conversation;
```

**Step 2: Write the failing tests**

Create `src/claude/events.rs` with just the test module first:

```rust
use serde::Deserialize;

/// A single NDJSON event from Claude CLI's stream-json output.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    MessageStart {
        message_id: String,
        model: String,
    },
    ContentBlockStart {
        index: usize,
        block_type: ContentBlockType,
    },
    ContentBlockDelta {
        index: usize,
        delta: Delta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        stop_reason: Option<String>,
    },
    MessageStop,
    /// Events we don't handle yet — ignored gracefully.
    Unknown(String),
}

#[derive(Debug, Clone)]
pub enum ContentBlockType {
    Text,
    ToolUse { id: String, name: String },
}

#[derive(Debug, Clone)]
pub enum Delta {
    TextDelta(String),
    InputJsonDelta(String),
}

/// Parse a single NDJSON line into a StreamEvent.
pub fn parse_event(line: &str) -> StreamEvent {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_start() {
        let line = r#"{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-opus-4-6","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageStart { message_id, model } => {
                assert_eq!(message_id, "msg_123");
                assert_eq!(model, "claude-opus-4-6");
            }
            other => panic!("Expected MessageStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_start_text() {
        let line = r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStart { index, block_type } => {
                assert_eq!(index, 0);
                assert!(matches!(block_type, ContentBlockType::Text));
            }
            other => panic!("Expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_start_tool_use() {
        let line = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"Bash","input":{}}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStart { index, block_type } => {
                assert_eq!(index, 1);
                match block_type {
                    ContentBlockType::ToolUse { id, name } => {
                        assert_eq!(id, "toolu_123");
                        assert_eq!(name, "Bash");
                    }
                    _ => panic!("Expected ToolUse"),
                }
            }
            other => panic!("Expected ContentBlockStart, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_delta_text() {
        let line = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello world"}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta(text) => assert_eq!(text, "Hello world"),
                    _ => panic!("Expected TextDelta"),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_delta_input_json() {
        let line = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 1);
                match delta {
                    Delta::InputJsonDelta(json) => assert!(json.contains("command")),
                    _ => panic!("Expected InputJsonDelta"),
                }
            }
            other => panic!("Expected ContentBlockDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_content_block_stop() {
        let line = r#"{"type":"content_block_stop","index":0}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::ContentBlockStop { index } => assert_eq!(index, 0),
            other => panic!("Expected ContentBlockStop, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_message_delta() {
        let line = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":42}}"#;
        let event = parse_event(line);
        match event {
            StreamEvent::MessageDelta { stop_reason } => {
                assert_eq!(stop_reason.as_deref(), Some("end_turn"));
            }
            other => panic!("Expected MessageDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_message_stop() {
        let line = r#"{"type":"message_stop"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_unknown_event() {
        let line = r#"{"type":"ping"}"#;
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }

    #[test]
    fn test_parse_invalid_json() {
        let line = "not json at all";
        let event = parse_event(line);
        assert!(matches!(event, StreamEvent::Unknown(_)));
    }
}
```

**Step 3: Run tests to verify they fail**

Run: `cargo test --lib claude::events`
Expected: Compilation fails with "not yet implemented" or test failures.

**Step 4: Implement `parse_event`**

Replace the `todo!()` in `parse_event` with the actual implementation using serde_json:

```rust
/// Raw JSON structures for deserialization.
#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    message: Option<RawMessage>,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    content_block: Option<RawContentBlock>,
    #[serde(default)]
    delta: Option<RawDelta>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: String,
    #[serde(default)]
    model: String,
}

#[derive(Deserialize)]
struct RawContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

pub fn parse_event(line: &str) -> StreamEvent {
    let raw: RawEvent = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => return StreamEvent::Unknown(line.to_string()),
    };

    match raw.event_type.as_str() {
        "message_start" => {
            let msg = raw.message.unwrap_or(RawMessage {
                id: String::new(),
                model: String::new(),
            });
            StreamEvent::MessageStart {
                message_id: msg.id,
                model: msg.model,
            }
        }
        "content_block_start" => {
            let index = raw.index.unwrap_or(0);
            let block = raw.content_block.unwrap_or(RawContentBlock {
                block_type: "text".to_string(),
                id: None,
                name: None,
            });
            let block_type = match block.block_type.as_str() {
                "tool_use" => ContentBlockType::ToolUse {
                    id: block.id.unwrap_or_default(),
                    name: block.name.unwrap_or_default(),
                },
                _ => ContentBlockType::Text,
            };
            StreamEvent::ContentBlockStart { index, block_type }
        }
        "content_block_delta" => {
            let index = raw.index.unwrap_or(0);
            let delta_raw = raw.delta.unwrap_or(RawDelta {
                delta_type: None,
                text: None,
                partial_json: None,
                stop_reason: None,
            });
            let delta = match delta_raw.delta_type.as_deref() {
                Some("input_json_delta") => {
                    Delta::InputJsonDelta(delta_raw.partial_json.unwrap_or_default())
                }
                _ => Delta::TextDelta(delta_raw.text.unwrap_or_default()),
            };
            StreamEvent::ContentBlockDelta { index, delta }
        }
        "content_block_stop" => StreamEvent::ContentBlockStop {
            index: raw.index.unwrap_or(0),
        },
        "message_delta" => {
            let stop_reason = raw.delta.and_then(|d| d.stop_reason);
            StreamEvent::MessageDelta { stop_reason }
        }
        "message_stop" => StreamEvent::MessageStop,
        _ => StreamEvent::Unknown(line.to_string()),
    }
}
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --lib claude::events`
Expected: All 10 tests pass.

**Step 6: Commit**

```bash
git add src/claude/
git commit -m "feat: add NDJSON event parser for Claude CLI stream-json output"
```

---

### Task 3: Create `src/claude/conversation.rs` — Conversation model

**Files:**
- Create: `src/claude/conversation.rs`
- Test: inline `#[cfg(test)]` module

The conversation model accumulates stream events into displayable messages.

**Step 1: Write failing tests**

Create `src/claude/conversation.rs`:

```rust
use crate::claude::events::{ContentBlockType, Delta, StreamEvent};

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

pub struct Conversation {
    pub messages: Vec<Message>,
    /// True while an assistant message is being streamed.
    streaming: bool,
    /// Accumulated JSON input for the current tool_use block.
    tool_input_buf: String,
    /// Track which blocks are tool_use by index.
    block_types: Vec<ContentBlockType>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            streaming: false,
            tool_input_buf: String::new(),
            block_types: Vec::new(),
        }
    }

    /// Add a user message to the conversation.
    pub fn push_user_message(&mut self, text: String) {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text(text)],
        });
    }

    /// Process a stream event and update the conversation state.
    pub fn apply_event(&mut self, event: &StreamEvent) {
        todo!()
    }

    /// Check if the assistant is currently streaming a response.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Get the current streaming text (partial response being built).
    pub fn streaming_text(&self) -> &str {
        if let Some(msg) = self.messages.last() {
            if msg.role == Role::Assistant {
                for block in msg.content.iter().rev() {
                    if let ContentBlock::Text(text) = block {
                        return text;
                    }
                }
            }
        }
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::events::{ContentBlockType, Delta, StreamEvent};

    #[test]
    fn test_push_user_message() {
        let mut conv = Conversation::new();
        conv.push_user_message("Hello".to_string());
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, Role::User);
    }

    #[test]
    fn test_message_start_creates_assistant_message() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_1".to_string(),
            model: "opus".to_string(),
        });
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, Role::Assistant);
        assert!(conv.is_streaming());
    }

    #[test]
    fn test_text_delta_accumulates() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_1".to_string(),
            model: "opus".to_string(),
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("Hello ".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("world".to_string()),
        });
        assert_eq!(conv.streaming_text(), "Hello world");
    }

    #[test]
    fn test_message_stop_ends_streaming() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_1".to_string(),
            model: "opus".to_string(),
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("Done".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });
        conv.apply_event(&StreamEvent::MessageDelta {
            stop_reason: Some("end_turn".to_string()),
        });
        conv.apply_event(&StreamEvent::MessageStop);
        assert!(!conv.is_streaming());
        assert_eq!(conv.messages.len(), 1);
    }

    #[test]
    fn test_tool_use_block() {
        let mut conv = Conversation::new();
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_1".to_string(),
            model: "opus".to_string(),
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::ToolUse {
                id: "toolu_1".to_string(),
                name: "Bash".to_string(),
            },
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta("{\"cmd\":".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::InputJsonDelta("\"ls\"}".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });

        let msg = &conv.messages[0];
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "Bash");
                assert_eq!(input, "{\"cmd\":\"ls\"}");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_full_conversation_flow() {
        let mut conv = Conversation::new();

        // User sends a message
        conv.push_user_message("What is 2+2?".to_string());

        // Assistant responds
        conv.apply_event(&StreamEvent::MessageStart {
            message_id: "msg_1".to_string(),
            model: "opus".to_string(),
        });
        conv.apply_event(&StreamEvent::ContentBlockStart {
            index: 0,
            block_type: ContentBlockType::Text,
        });
        conv.apply_event(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: Delta::TextDelta("2+2 = 4".to_string()),
        });
        conv.apply_event(&StreamEvent::ContentBlockStop { index: 0 });
        conv.apply_event(&StreamEvent::MessageStop);

        assert_eq!(conv.messages.len(), 2);
        assert_eq!(conv.messages[0].role, Role::User);
        assert_eq!(conv.messages[1].role, Role::Assistant);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib claude::conversation`
Expected: Fails with "not yet implemented".

**Step 3: Implement `apply_event`**

Replace the `todo!()` with:

```rust
pub fn apply_event(&mut self, event: &StreamEvent) {
    match event {
        StreamEvent::MessageStart { .. } => {
            self.messages.push(Message {
                role: Role::Assistant,
                content: Vec::new(),
            });
            self.streaming = true;
            self.block_types.clear();
            self.tool_input_buf.clear();
        }
        StreamEvent::ContentBlockStart { index: _, block_type } => {
            self.block_types.push(block_type.clone());
            if let Some(msg) = self.messages.last_mut() {
                match block_type {
                    ContentBlockType::Text => {
                        msg.content.push(ContentBlock::Text(String::new()));
                    }
                    ContentBlockType::ToolUse { id, name } => {
                        self.tool_input_buf.clear();
                        msg.content.push(ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input: String::new(),
                        });
                    }
                }
            }
        }
        StreamEvent::ContentBlockDelta { index: _, delta } => {
            if let Some(msg) = self.messages.last_mut() {
                if let Some(block) = msg.content.last_mut() {
                    match (block, delta) {
                        (ContentBlock::Text(text), Delta::TextDelta(new_text)) => {
                            text.push_str(new_text);
                        }
                        (ContentBlock::ToolUse { input, .. }, Delta::InputJsonDelta(json)) => {
                            self.tool_input_buf.push_str(json);
                            *input = self.tool_input_buf.clone();
                        }
                        _ => {}
                    }
                }
            }
        }
        StreamEvent::ContentBlockStop { .. } => {
            // Block finalized — nothing special needed
        }
        StreamEvent::MessageDelta { .. } => {
            // Could extract usage/stop_reason if needed later
        }
        StreamEvent::MessageStop => {
            self.streaming = false;
        }
        StreamEvent::Unknown(_) => {}
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --lib claude::conversation`
Expected: All 6 tests pass.

**Step 5: Commit**

```bash
git add src/claude/conversation.rs
git commit -m "feat: add conversation model with streaming event accumulation"
```

---

### Task 4: Create `src/claude/process.rs` — Process spawning

**Files:**
- Create: `src/claude/process.rs`
- Test: inline `#[cfg(test)]` module

Spawns `claude -p --output-format stream-json --input-format stream-json` via `tokio::process::Command`.

**Step 1: Write the process module**

Create `src/claude/process.rs`:

```rust
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
    pub fn spawn(
        command: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<StreamEvent>)> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let (program, args) = parts.split_first().context("Empty command")?;

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.args(["-p", "--output-format", "stream-json", "--input-format", "stream-json"]);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Inherit environment
        cmd.envs(std::env::vars());

        let mut child = cmd.spawn().with_context(|| format!("Failed to spawn '{}'", command))?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn stdout reader task
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
            "type": "user_input",
            "content": text,
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
        // Best-effort kill
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
            let result = ClaudeProcess::spawn("nonexistent_command_xyz");
            assert!(result.is_err());
        });
    }
}
```

**Step 2: Register the `claude` module in `src/main.rs`**

Add `mod claude;` to `src/main.rs` (below the existing mod declarations):

```rust
mod app;
mod claude;
mod config;
// ... rest stays
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles (the `process.rs` module references `events.rs` which is already created).

**Step 4: Run the test**

Run: `cargo test --lib claude::process`
Expected: 1 test passes.

**Step 5: Commit**

```bash
git add src/claude/ src/main.rs
git commit -m "feat: add Claude process spawner with stream-json I/O"
```

---

### Task 5: Update header font to Big figlet style

**Files:**
- Modify: `src/ui/header.rs`

**Step 1: Replace the LOGO constant**

In `src/ui/header.rs`, replace lines 9-16 (the `HEADER_HEIGHT` and `LOGO` constants):

Old:
```rust
pub const HEADER_HEIGHT: u16 = 7;

const LOGO: [&str; 3] = [
    "╔═╗╔═╗═╗ ╦╦ ╦  ╔═╗╦  ╔═╗╦ ╦╔╦╗╔═╗",
    "╚═╗║╣ ╠╦╝╚╦╝  ║  ║  ╠═╣║ ║ ║║║╣ ",
    "╚═╝╚═╝╩╚═ ╩   ╚═╝╩═╝╩ ╩╚═╝═╩╝╚═╝",
];
```

New:
```rust
pub const HEADER_HEIGHT: u16 = 10;

/// ASCII art logo — "SEXY CLAUDE" in Big figlet style (6 rows, ~76 chars wide).
const LOGO: [&str; 6] = [
    r"  _____ ________   ____     __   _____ _              _    _ _____  ______ ",
    r" / ____|  ____\ \ / /\ \   / /  / ____| |        /\  | |  | |  __ \|  ____|",
    r"| (___ | |__   \ V /  \ \_/ /  | |    | |       /  \ | |  | | |  | | |__   ",
    r" \___ \|  __|   > <    \   /   | |    | |      / /\ \| |  | | |  | |  __|  ",
    r" ____) | |____ / . \    | |    | |____| |____ / ____ \ |__| | |__| | |____ ",
    r"|_____/|______/_/ \_\   |_|     \_____|______/_/    \_\____/|_____/|______|",
];
```

**Step 2: Update logo rendering loop**

The render method references `LOGO.iter()` — since LOGO is now 6 rows instead of 3, update the sparkle row and version row offsets. The render body in `Widget::render`:

- Row 0: sparkle row (unchanged)
- Rows 1-6: logo (was 1-3, now 1-6)
- Row 7: sparkle row (was row 4)
- Row 8: version text (was row 5)
- Row 9: decorative line (was row 6)

Update the hard-coded offsets:
- Second sparkle row: `area.top() + 4` → `area.top() + 7`
- Version row: `area.top() + 5` → `area.top() + 8`
- Decorative line: `area.top() + 6` → `area.top() + 9`

**Step 3: Update tests that reference row indices**

In the test `test_header_renders_without_panic`, update the row check (was checking row 1 for `╔` or `═`). Change to check for `_` or `/` or `|` characters in the logo:

```rust
let row: String = (0..80)
    .map(|x| {
        buf.cell((x, 1))
            .unwrap()
            .symbol()
            .chars()
            .next()
            .unwrap_or(' ')
    })
    .collect();
assert!(row.contains('_') || row.contains('/') || row.contains('|'));
```

**Step 4: Run tests**

Run: `cargo test --lib ui::header`
Expected: All header tests pass.

**Step 5: Commit**

```bash
git add src/ui/header.rs
git commit -m "feat: replace Calvin S font with Big figlet for readable header"
```

---

### Task 6: Rewrite `src/ui/claude_pane.rs` — Conversation renderer

**Files:**
- Modify: `src/ui/claude_pane.rs`
- Test: inline `#[cfg(test)]` module

Replace the vt100 screen renderer with a conversation renderer that displays messages as styled text.

**Step 1: Write the new ClaudePane**

Completely rewrite `src/ui/claude_pane.rs`:

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::claude::conversation::{ContentBlock, Conversation, Message, Role};
use crate::theme::Theme;

/// A widget that renders the conversation as a scrollable chat.
pub struct ClaudePane<'a> {
    conversation: &'a Conversation,
    theme: &'a Theme,
    scroll_offset: usize,
}

impl<'a> ClaudePane<'a> {
    pub fn new(conversation: &'a Conversation, theme: &'a Theme, scroll_offset: usize) -> Self {
        Self {
            conversation,
            theme,
            scroll_offset,
        }
    }
}

impl Widget for ClaudePane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let bg = self.theme.background;
        let fg = self.theme.foreground;

        // Fill background
        let bg_style = Style::default().bg(bg);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(bg_style);
                    cell.set_char(' ');
                }
            }
        }

        // Convert conversation to lines
        let lines = render_conversation(self.conversation, area.width as usize);

        // Apply scroll offset: skip `scroll_offset` lines, render up to area.height lines
        let visible_lines: Vec<&StyledLine> = lines
            .iter()
            .skip(self.scroll_offset)
            .take(area.height as usize)
            .collect();

        for (row_idx, line) in visible_lines.iter().enumerate() {
            let y = area.top() + row_idx as u16;
            if y >= area.bottom() {
                break;
            }
            let mut x = area.left();
            for span in &line.spans {
                for ch in span.text.chars() {
                    if x >= area.right() {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        cell.set_style(span.style.bg(bg));
                    }
                    x += 1;
                }
            }
        }
    }
}

/// A styled span within a line.
#[derive(Debug, Clone)]
struct StyledSpan {
    text: String,
    style: Style,
}

/// A line composed of styled spans.
#[derive(Debug, Clone)]
struct StyledLine {
    spans: Vec<StyledSpan>,
}

impl StyledLine {
    fn empty() -> Self {
        Self { spans: Vec::new() }
    }

    fn plain(text: &str, style: Style) -> Self {
        Self {
            spans: vec![StyledSpan {
                text: text.to_string(),
                style,
            }],
        }
    }
}

/// Convert the entire conversation into styled lines ready for rendering.
fn render_conversation(conversation: &Conversation, width: usize) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    for (i, msg) in conversation.messages.iter().enumerate() {
        if i > 0 {
            lines.push(StyledLine::empty()); // blank separator
        }
        render_message(msg, width, &mut lines);
    }

    lines
}

fn render_message(msg: &Message, width: usize, lines: &mut Vec<StyledLine>) {
    let (prefix, prefix_style) = match msg.role {
        Role::User => (
            "> ",
            Style::default()
                .fg(Color::Rgb(137, 180, 250)) // blue
                .add_modifier(Modifier::BOLD),
        ),
        Role::Assistant => (
            "",
            Style::default().fg(Color::Rgb(166, 227, 161)), // green
        ),
    };

    for block in &msg.content {
        match block {
            ContentBlock::Text(text) => {
                render_text_block(text, prefix, prefix_style, width, lines);
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let tool_style = Style::default()
                    .fg(Color::Rgb(249, 226, 175)) // yellow
                    .add_modifier(Modifier::DIM);
                let summary = if input.len() > 60 {
                    format!("[{}] {}...", name, &input[..57])
                } else {
                    format!("[{}] {}", name, input)
                };
                lines.push(StyledLine::plain(&summary, tool_style));
            }
        }
    }
}

/// Render a text block with basic formatting: **bold**, `code`, ```code blocks```, # headers.
fn render_text_block(
    text: &str,
    prefix: &str,
    prefix_style: Style,
    width: usize,
    lines: &mut Vec<StyledLine>,
) {
    let text_style = Style::default().fg(Color::Rgb(205, 214, 244)); // light text
    let bold_style = text_style.add_modifier(Modifier::BOLD);
    let code_style = Style::default().fg(Color::Rgb(166, 227, 161)); // green for inline code
    let code_block_style = Style::default().fg(Color::Rgb(180, 190, 220));
    let header_style = Style::default()
        .fg(Color::Rgb(203, 166, 247)) // purple for headers
        .add_modifier(Modifier::BOLD);

    let mut in_code_block = false;

    for (line_idx, raw_line) in text.lines().enumerate() {
        // Code block fence
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            let fence_style = Style::default().fg(Color::Rgb(127, 132, 156)).add_modifier(Modifier::DIM);
            lines.push(StyledLine::plain(raw_line, fence_style));
            continue;
        }

        if in_code_block {
            // Render code block lines verbatim
            lines.push(StyledLine::plain(raw_line, code_block_style));
            continue;
        }

        // Headers
        if raw_line.starts_with('#') {
            lines.push(StyledLine::plain(raw_line, header_style));
            continue;
        }

        // Normal text with inline formatting
        let effective_prefix = if line_idx == 0 { prefix } else { "" };
        let mut spans = Vec::new();

        if !effective_prefix.is_empty() {
            spans.push(StyledSpan {
                text: effective_prefix.to_string(),
                style: prefix_style,
            });
        }

        // Simple inline parsing: **bold** and `code`
        let mut remaining = raw_line;
        while !remaining.is_empty() {
            if remaining.starts_with("**") {
                if let Some(end) = remaining[2..].find("**") {
                    spans.push(StyledSpan {
                        text: remaining[2..2 + end].to_string(),
                        style: bold_style,
                    });
                    remaining = &remaining[2 + end + 2..];
                    continue;
                }
            }
            if remaining.starts_with('`') {
                if let Some(end) = remaining[1..].find('`') {
                    spans.push(StyledSpan {
                        text: remaining[1..1 + end].to_string(),
                        style: code_style,
                    });
                    remaining = &remaining[1 + end + 1..];
                    continue;
                }
            }
            // Find next special char
            let next_special = remaining
                .find(|c: char| c == '*' || c == '`')
                .unwrap_or(remaining.len());
            if next_special > 0 {
                spans.push(StyledSpan {
                    text: remaining[..next_special].to_string(),
                    style: text_style,
                });
                remaining = &remaining[next_special..];
            } else {
                // Single special char that didn't match a pattern
                spans.push(StyledSpan {
                    text: remaining[..1].to_string(),
                    style: text_style,
                });
                remaining = &remaining[1..];
            }
        }

        lines.push(StyledLine { spans });
    }
}

/// Calculate total number of rendered lines for scroll calculations.
pub fn total_lines(conversation: &Conversation, width: usize) -> usize {
    render_conversation(conversation, width).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::conversation::{ContentBlock, Conversation, Message, Role};

    #[test]
    fn test_empty_conversation_renders() {
        let conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 0);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        // Should not panic, should fill with bg
    }

    #[test]
    fn test_user_message_has_prefix() {
        let mut conv = Conversation::new();
        conv.push_user_message("Hello".to_string());
        let lines = render_conversation(&conv, 80);
        assert!(!lines.is_empty());
        let first_line = &lines[0];
        let text: String = first_line.spans.iter().map(|s| s.text.as_str()).collect();
        assert!(text.starts_with("> "));
        assert!(text.contains("Hello"));
    }

    #[test]
    fn test_code_block_rendering() {
        let mut conv = Conversation::new();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(
                "Here is code:\n```rust\nfn main() {}\n```\nDone.".to_string(),
            )],
        });
        let lines = render_conversation(&conv, 80);
        // Should have: "Here is code:", "```rust", "fn main() {}", "```", "Done."
        assert!(lines.len() >= 5);
    }

    #[test]
    fn test_tool_use_rendering() {
        let mut conv = Conversation::new();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "Bash".to_string(),
                input: "{\"command\":\"ls\"}".to_string(),
            }],
        });
        let lines = render_conversation(&conv, 80);
        let text: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("[Bash]"));
    }

    #[test]
    fn test_scroll_offset() {
        let mut conv = Conversation::new();
        // Add enough messages to fill more than a screen
        for i in 0..30 {
            conv.push_user_message(format!("Message {}", i));
        }
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 10);
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
        // Should not panic with scroll offset
    }

    #[test]
    fn test_zero_area() {
        let conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }
}
```

**Step 2: Run tests**

Run: `cargo test --lib ui::claude_pane`
Expected: All 6 tests pass.

**Step 3: Commit**

```bash
git add src/ui/claude_pane.rs
git commit -m "feat: rewrite ClaudePane to render conversation model instead of vt100"
```

---

### Task 7: Rewrite `src/app.rs` — Stream-json event loop

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/mod.rs`

This is the biggest change. We replace the PTY-based event loop with a stream-json event loop.

**Step 1: Rewrite `src/app.rs`**

The new App struct:
- Replaces `PtyProcess` + `TerminalEmulator` with `ClaudeProcess` + `Conversation`
- New Msg variants: `ClaudeEvent(StreamEvent)`, `UserSubmit(String)`
- Key handling: Enter submits input, arrows/PgUp/PgDown scroll, Esc for overlays
- InputEditor is now a first-class field

```rust
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::claude::conversation::Conversation;
use crate::claude::events::StreamEvent;
use crate::claude::process::ClaudeProcess;
use crate::config::Config;
use crate::theme::Theme;
use crate::ui;
use crate::ui::header::HEADER_HEIGHT;
use crate::ui::input::InputEditor;
use crate::ui::overlay::{OverlayItem, OverlayState};

enum Msg {
    ClaudeEvent(StreamEvent),
    ClaudeExited,
    Key(event::KeyEvent),
    Resize(u16, u16),
    Tick,
}

enum AppMode {
    Normal,
    ActionMenu(OverlayState),
    ThemePicker(OverlayState),
}

pub struct App {
    config: Config,
    theme: Theme,
    conversation: Conversation,
    claude: Option<ClaudeProcess>,
    input: InputEditor,
    should_quit: bool,
    frame_count: u64,
    mode: AppMode,
    theme_name: String,
    scroll_offset: usize,
    auto_scroll: bool,
    /// The command to pass to Claude process
    command: String,
}

impl App {
    pub fn new(config: Config, theme: Theme, theme_name: String, command: String) -> Self {
        Self {
            config,
            theme,
            conversation: Conversation::new(),
            claude: None,
            input: InputEditor::new(),
            should_quit: false,
            frame_count: 0,
            mode: AppMode::Normal,
            theme_name,
            scroll_offset: 0,
            auto_scroll: true,
            command,
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

        // Spawn Claude process
        let (claude_process, mut event_rx) = ClaudeProcess::spawn(&self.command)?;
        self.claude = Some(claude_process);

        // Forward Claude events to the main channel
        let tx_claude = tx.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if tx_claude.send(Msg::ClaudeEvent(event)).is_err() {
                    break;
                }
            }
            let _ = tx_claude.send(Msg::ClaudeExited);
        });

        // Spawn crossterm event reader task
        let tx_event = tx.clone();
        std::thread::spawn(move || {
            event_reader_loop(tx_event);
        });

        // Spawn tick task
        let tick_ms = 1000 / self.config.fps as u64;
        let tx_tick = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
            loop {
                interval.tick().await;
                if tx_tick.send(Msg::Tick).is_err() {
                    break;
                }
            }
        });

        // Initial render
        self.view(terminal)?;

        // Event loop
        while let Some(msg) = rx.recv().await {
            self.update(msg).await?;
            if self.should_quit {
                break;
            }
            self.view(terminal)?;
        }

        // Cleanup
        if let Some(ref mut claude) = self.claude {
            let _ = claude.kill().await;
        }

        Ok(())
    }

    async fn update(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::ClaudeEvent(event) => {
                self.conversation.apply_event(&event);
                if self.auto_scroll {
                    self.scroll_to_bottom();
                }
            }
            Msg::ClaudeExited => {
                // Claude process ended — could mark this in UI
            }
            Msg::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }
                self.handle_key(key).await?;
            }
            Msg::Resize(_width, _height) => {
                // Ratatui handles terminal resize automatically
                if self.auto_scroll {
                    self.scroll_to_bottom();
                }
            }
            Msg::Tick => {
                self.frame_count = self.frame_count.wrapping_add(1);
            }
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match &self.mode {
            AppMode::Normal => self.handle_key_normal(key).await,
            AppMode::ActionMenu(_) | AppMode::ThemePicker(_) => self.handle_key_overlay(key),
        }
    }

    async fn handle_key_normal(&mut self, key: event::KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // Global shortcuts
        if ctrl && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('k') {
            self.open_action_menu();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('t') {
            self.open_theme_picker();
            return Ok(());
        }

        // Scrolling
        match key.code {
            KeyCode::PageUp => {
                self.auto_scroll = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                return Ok(());
            }
            KeyCode::PageDown => {
                self.scroll_offset += 10;
                // Re-enable auto-scroll if we're near the bottom
                self.clamp_scroll();
                return Ok(());
            }
            _ => {}
        }

        // Input handling
        match key.code {
            KeyCode::Enter if !shift => {
                if !self.input.is_empty() && !self.conversation.is_streaming() {
                    let text = self.input.take_content();
                    self.conversation.push_user_message(text.clone());
                    self.auto_scroll = true;
                    self.scroll_to_bottom();
                    if let Some(ref mut claude) = self.claude {
                        claude.send_message(&text).await?;
                    }
                }
            }
            KeyCode::Enter if shift => {
                self.input.insert_newline();
            }
            KeyCode::Char(c) if !ctrl => {
                self.input.insert_char(c);
            }
            KeyCode::Backspace => {
                self.input.backspace();
            }
            KeyCode::Delete => {
                self.input.delete();
            }
            KeyCode::Left => {
                self.input.move_left();
            }
            KeyCode::Right => {
                self.input.move_right();
            }
            KeyCode::Home => {
                self.input.move_home();
            }
            KeyCode::End => {
                self.input.move_end();
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_key_overlay(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                self.confirm_overlay()?;
            }
            KeyCode::Up => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.move_up();
                }
                self.preview_theme();
            }
            KeyCode::Down => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.move_down();
                }
                self.preview_theme();
            }
            KeyCode::Backspace => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.type_char(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn scroll_to_bottom(&mut self) {
        // Will be clamped during rendering
        self.scroll_offset = usize::MAX;
    }

    fn clamp_scroll(&mut self) {
        let total = ui::claude_pane::total_lines(&self.conversation, 80);
        let max_scroll = total.saturating_sub(10); // rough estimate
        if self.scroll_offset >= max_scroll {
            self.scroll_offset = max_scroll;
            self.auto_scroll = true;
        }
    }

    fn open_theme_picker(&mut self) {
        let themes = crate::theme::Theme::list_available();
        let items: Vec<OverlayItem> = themes
            .iter()
            .map(|name| {
                let display = crate::theme::Theme::load(name)
                    .map(|t| t.name)
                    .unwrap_or_else(|_| name.clone());
                OverlayItem {
                    label: display,
                    value: name.clone(),
                    hint: String::new(),
                }
            })
            .collect();

        let current_idx = items
            .iter()
            .position(|i| i.value == self.theme_name)
            .unwrap_or(0);
        let mut state = OverlayState::new(items, Some(self.theme_name.clone()));
        state.selected = current_idx;
        self.mode = AppMode::ThemePicker(state);
    }

    fn open_action_menu(&mut self) {
        let items = vec![
            OverlayItem {
                label: "Switch Theme".to_string(),
                value: "theme".to_string(),
                hint: "Ctrl+T".to_string(),
            },
            OverlayItem {
                label: "Quit".to_string(),
                value: "quit".to_string(),
                hint: "Ctrl+Q".to_string(),
            },
        ];
        self.mode = AppMode::ActionMenu(OverlayState::new(items, None));
    }

    fn preview_theme(&mut self) {
        if let AppMode::ThemePicker(ref state) = self.mode {
            if let Some(value) = state.selected_value() {
                if let Ok(new_theme) = crate::theme::Theme::load(&value) {
                    self.theme = new_theme;
                }
            }
        }
    }

    fn close_overlay(&mut self) {
        if let AppMode::ThemePicker(ref state) = self.mode {
            if let Some(ref original) = state.original_theme {
                if let Ok(theme) = crate::theme::Theme::load(original) {
                    self.theme = theme;
                }
            }
        }
        self.mode = AppMode::Normal;
    }

    fn confirm_overlay(&mut self) -> Result<()> {
        let mode = std::mem::replace(&mut self.mode, AppMode::Normal);

        match mode {
            AppMode::ThemePicker(state) => {
                if let Some(value) = state.selected_value() {
                    if let Ok(new_theme) = crate::theme::Theme::load(&value) {
                        self.theme = new_theme;
                        self.theme_name = value.clone();
                        let config_path = crate::config::Config::default_path();
                        let _ = crate::config::save_theme(&value, &config_path);
                    }
                }
            }
            AppMode::ActionMenu(state) => {
                if let Some(value) = state.selected_value() {
                    match value.as_str() {
                        "theme" => self.open_theme_picker(),
                        "quit" => self.should_quit = true,
                        _ => {}
                    }
                }
            }
            AppMode::Normal => {}
        }
        Ok(())
    }

    fn view(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let theme = &self.theme;
        let frame_count = self.frame_count;
        let overlay = match &self.mode {
            AppMode::ActionMenu(state) => Some(("Actions", state)),
            AppMode::ThemePicker(state) => Some(("Select Theme", state)),
            AppMode::Normal => None,
        };

        // Clamp scroll before rendering
        let total_height = terminal.size()?.height.saturating_sub(HEADER_HEIGHT + 4) as usize; // approx visible area
        let total_conv_lines = ui::claude_pane::total_lines(&self.conversation, terminal.size()?.width.saturating_sub(4) as usize);
        if self.auto_scroll || self.scroll_offset > total_conv_lines {
            self.scroll_offset = total_conv_lines.saturating_sub(total_height);
        }

        let conversation = &self.conversation;
        let input = &self.input;
        let scroll_offset = self.scroll_offset;
        let is_streaming = self.conversation.is_streaming();

        terminal.draw(|frame| {
            ui::render(frame, conversation, input, theme, frame_count, scroll_offset, is_streaming);
            if let Some((title, state)) = overlay {
                ui::render_overlay(frame, title, state, theme);
            }
        })?;

        Ok(())
    }
}

fn event_reader_loop(tx: mpsc::UnboundedSender<Msg>) {
    loop {
        match event::read() {
            Ok(Event::Key(key)) => {
                if tx.send(Msg::Key(key)).is_err() {
                    break;
                }
            }
            Ok(Event::Resize(w, h)) => {
                if tx.send(Msg::Resize(w, h)).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}
```

**Step 2: Update `src/ui/mod.rs`**

The `render` function signature changes — it now takes a `Conversation` and `InputEditor` instead of a `vt100::Screen`:

```rust
pub mod borders;
pub mod claude_pane;
pub mod header;
pub mod input;
pub mod overlay;
pub mod status_bar;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::claude::conversation::Conversation;
use crate::theme::Theme;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT};
use input::{InputEditor, InputWidget};
use overlay::{OverlayState, OverlayWidget};
use status_bar::StatusBar;

/// Render the full UI layout.
pub fn render(
    frame: &mut Frame,
    conversation: &Conversation,
    input: &InputEditor,
    theme: &Theme,
    frame_count: u64,
    scroll_offset: usize,
    is_streaming: bool,
) {
    let size = frame.area();

    // Main vertical layout: [header] [claude pane] [input] [status bar]
    let input_height = if input.is_empty() { 1 } else { 3.min(input.content().lines().count() as u16 + 1) };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT), // Animated header
            Constraint::Min(3),               // Claude pane (fills remaining)
            Constraint::Length(input_height),  // Input area
            Constraint::Length(1),            // Status bar
        ])
        .split(size);

    // Animated header
    frame.render_widget(Header::new(theme, frame_count), chunks[0]);

    // Claude pane with themed border
    let claude_block = borders::themed_block("", true, theme);
    let claude_inner = claude_block.inner(chunks[1]);
    frame.render_widget(claude_block, chunks[1]);
    frame.render_widget(
        ClaudePane::new(conversation, theme, scroll_offset),
        claude_inner,
    );

    // Input area with themed border
    let input_title = if is_streaming { " streaming... " } else { "" };
    let input_block = borders::themed_block(input_title, !is_streaming, theme);
    let input_inner = input_block.inner(chunks[2]);
    frame.render_widget(input_block, chunks[2]);
    frame.render_widget(InputWidget::new(input, theme), input_inner);

    // Status bar
    frame.render_widget(StatusBar::new(&theme.name, theme), chunks[3]);
}

/// Render an overlay popup on top of the existing UI.
pub fn render_overlay(frame: &mut Frame, title: &str, state: &OverlayState, theme: &Theme) {
    let widget = OverlayWidget::new(title, state, theme);
    frame.render_widget(widget, frame.area());
}
```

**Step 3: Update `src/main.rs`**

Replace the PTY spawning with the new App constructor:

```rust
mod app;
mod claude;
mod config;
mod keybindings;
mod pty;
mod terminal;
mod theme;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sexy-claude", about = "A beautiful terminal wrapper for Claude Code")]
#[command(version)]
struct Cli {
    /// Theme name (e.g., catppuccin-mocha, nord, dracula)
    #[arg(short, long)]
    theme: Option<String>,

    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Command to run (default: claude)
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = config::Config::load(cli.config.as_ref())
        .context("Failed to load configuration")?;

    // Determine theme: CLI flag > config > default
    let theme_name = cli.theme.as_deref().unwrap_or(&config.theme);
    let theme = theme::Theme::load(theme_name).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load theme '{}': {}. Using default.", theme_name, e);
        theme::Theme::default_theme()
    });

    // Determine command
    let command = if cli.command.is_empty() {
        config.command.clone()
    } else {
        cli.command.join(" ")
    };

    // Check that the command exists
    let program = command.split_whitespace().next().unwrap_or("claude");
    if which(program).is_none() {
        anyhow::bail!(
            "'{}' not found in PATH. Please install Claude Code first:\n  npm install -g @anthropic-ai/claude-code",
            program
        );
    }

    // Check terminal size
    let (cols, rows) = crossterm::terminal::size().context("Failed to get terminal size")?;
    if cols < 40 || rows < 10 {
        anyhow::bail!("Terminal too small ({}x{}). Need at least 40x10.", cols, rows);
    }

    // Initialize terminal
    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle("sexy-claude")
    )?;

    // Run the app
    let theme_name_owned = theme_name.to_string();
    let mut app = app::App::new(config, theme, theme_name_owned, command);
    let result = app.run(&mut terminal).await;

    // Cleanup — always restore terminal
    ratatui::restore();

    result
}

/// Simple which implementation
fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(program);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    })
}
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: Compiles. Some warnings about unused `pty` and `terminal` modules (ok for now).

**Step 5: Run all tests**

Run: `cargo test`
Expected: All unit tests pass. Integration tests in `tests/pty_integration_test.rs` and `tests/converter_test.rs` still pass (they test the old modules which are still present).

**Step 6: Commit**

```bash
git add src/app.rs src/ui/mod.rs src/main.rs
git commit -m "feat: rewrite app to use stream-json I/O instead of PTY wrapping"
```

---

### Task 8: Clean up old modules and run full verification

**Files:**
- Modify: `Cargo.toml` (remove `portable-pty` and `vt100` from deps — optional, can keep for now)
- Modify: `src/main.rs` (remove `mod pty` and `mod terminal` if desired)

**Step 1: Suppress dead code warnings on old modules**

If `pty` and `terminal` modules are no longer imported by `app.rs` or `ui`, they'll get dead_code warnings. For now, add `#[allow(dead_code)]` annotations or remove the `mod` declarations from `main.rs`.

The safest approach: keep the modules but remove the `mod pty;` and `mod terminal;` lines from `main.rs`. This means the old integration tests won't compile against the lib, so also remove or gate them.

Alternatively, keep everything and just suppress warnings. This is safer for incremental migration.

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No errors. Fix any clippy suggestions.

**Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

**Step 4: Build release**

Run: `cargo build --release`
Expected: Builds successfully.

**Step 5: Commit**

```bash
git add -A
git commit -m "chore: clean up after stream-json rewrite"
```

---

### Task 9: Integration test — verify end-to-end flow

**Step 1: Manual smoke test**

Run: `cargo run`

Verify:
1. The new header renders with Big figlet font and animations
2. Claude Code's original header is NOT visible
3. The input area shows with placeholder text
4. Typing text appears in the input area
5. Pressing Enter sends the message and shows it as a user message
6. Claude's response streams in as text
7. Code blocks render with different styling
8. Ctrl+T opens theme picker
9. Ctrl+K opens action menu
10. Ctrl+Q quits

**Step 2: Commit any fixes**

If any issues found during smoke testing, fix and commit.

**Step 3: Final commit + push**

```bash
git push origin master
```
