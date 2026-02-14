# Stream JSON Rewrite + Header Font Fix

## Problem

1. Claude Code's default TUI header (pig logo, version, model info) is visible inside the PTY pane because we wrap the full interactive TUI
2. The current ASCII art font (Calvin S with box-drawing characters) is hard to read

## Solution

### 1. Replace PTY wrapping with stream-json I/O

Instead of running Claude Code as a full interactive TUI inside a PTY, run it in print mode with structured streaming:

```
claude -p --output-format stream-json --input-format stream-json
```

This gives us complete control over rendering. No more Claude Code UI leaking through.

### 2. New header font

Replace Calvin S (`╔═╗`) with Big figlet font — clean ASCII block letters using `|/_\()><=`:

```
  _____ ________   ____     __   _____ _              _    _ _____  ______
 / ____|  ____\ \ / /\ \   / /  / ____| |        /\  | |  | |  __ \|  ____|
| (___ | |__   \ V /  \ \_/ /  | |    | |       /  \ | |  | | |  | | |__
 \___ \|  __|   > <    \   /   | |    | |      / /\ \| |  | | |  | |  __|
 ____) | |____ / . \    | |    | |____| |____ / ____ \ |__| | |__| | |____
|_____/|______/_/ \_\   |_|     \_____|______/_/    \_\____/|_____/|______|
```

6 rows, ~76 chars wide. Universal ASCII rendering.

## Architecture

### New modules

- `src/claude/mod.rs` — Module root
- `src/claude/process.rs` — Spawn claude process, manage stdin/stdout pipes
- `src/claude/events.rs` — NDJSON event parsing into typed structs
- `src/claude/conversation.rs` — Conversation state: messages, content blocks, streaming accumulator

### Modified modules

- `src/app.rs` — New Msg variants (ClaudeEvent, UserSubmit), stream-json event loop
- `src/ui/claude_pane.rs` — Rewritten: renders conversation model instead of vt100 screen
- `src/ui/header.rs` — New Big font, keep animation effects
- `src/ui/mod.rs` — Updated render function signature
- `src/main.rs` — New process spawning, remove PTY setup

### Removed/unused modules

- `src/pty/` — No longer needed (portable_pty dependency can be removed)
- `src/terminal/` — vt100 parsing no longer needed

### NDJSON Event Flow

```
message_start → content_block_start → content_block_delta (text chunks)
→ content_block_stop → message_delta (usage/stop_reason) → message_stop
```

### Conversation Model

```rust
struct Conversation {
    messages: Vec<Message>,
    streaming: Option<StreamingState>,
}

struct Message {
    role: Role,           // User | Assistant
    content: Vec<ContentBlock>,
}

enum ContentBlock {
    Text(String),
    ToolUse { id: String, name: String, input: String },
    ToolResult { id: String, output: String },
}

struct StreamingState {
    partial_text: String,
    block_index: usize,
}
```

### UI — Scrollable Chat Pane

- Full conversation history with scrollback
- User messages styled differently (e.g., different color/prefix)
- Assistant messages with basic formatting: bold, code blocks, headers, lists
- Tool calls shown as status lines
- Auto-scroll on new content; manual scroll with arrow keys / PgUp / PgDown

### Input

- Existing input widget captures text
- On Enter: serialize as stream-json event, write to claude's stdin
- On Ctrl+C: send interrupt signal to child process

## Dependencies

### Add
- `serde` + `serde_json` — JSON parsing
- `tokio::process` — Already available via tokio

### Remove (optional, can defer)
- `portable_pty`
- `vt100`

## Decisions

- Keep all existing UI chrome (header, borders, status bar, overlay system)
- Basic markdown formatting (bold, code blocks, headers, lists) — not full markdown
- Scrollable chat with auto-scroll behavior
- Stream-json bidirectional communication
