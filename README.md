# sexy-claude (sc)

[![GitHub release](https://img.shields.io/github/v/release/MagnusPladsen/sexy-claude-code?include_prereleases&style=flat-square)](https://github.com/MagnusPladsen/sexy-claude-code/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![GitHub stars](https://img.shields.io/github/stars/MagnusPladsen/sexy-claude-code?style=flat-square)](https://github.com/MagnusPladsen/sexy-claude-code/stargazers)
[![GitHub issues](https://img.shields.io/github/issues/MagnusPladsen/sexy-claude-code?style=flat-square)](https://github.com/MagnusPladsen/sexy-claude-code/issues)

A beautiful terminal wrapper for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) built with Rust. All vanilla Claude Code features work out of the box — slash commands, permissions, hooks, MCP, skills — while adding themes, split panes, cost tracking, and more.

<img width="1703" height="1377" alt="image" src="https://github.com/user-attachments/assets/9761c22a-0862-44c1-bca4-ae2f4072fe98" />

## Features

- **Themed rendering** — 10+ bundled themes (Catppuccin, Nord, Dracula, Gruvbox, etc.) with custom theme support
- **Cost tracking** — Real-time session cost in the status bar with per-model pricing
- **Split pane mode** — Side-by-side conversation + file/diff preview (Ctrl+S)
- **Agent dashboard** — Monitor sub-agents spawned via the Task tool (Ctrl+A)
- **Plugin browser** — Browse, install, enable/disable Claude plugins (Ctrl+P)
- **Workflow templates** — Quick-launch common prompts: code review, tests, debug, etc. (Ctrl+W)
- **Session management** — Resume previous sessions, rename, checkpoint/rewind
- **Input history** — Persistent history with Ctrl+R fuzzy search
- **Diff viewer** — Word-level diff highlighting for file edits (Ctrl+D)
- **File context panel** — See all files accessed in the session (Ctrl+F)
- **Collapsible tool blocks** — Expand/collapse tool output (Ctrl+E)
- **Full vanilla passthrough** — All Claude Code slash commands, permissions, hooks, and MCP work natively

## Install

### From source

```bash
cargo install --path .
```

This installs both `sexy-claude` and `sc` as binaries.

### Build locally

```bash
cargo build --release
./target/release/sc
```

## Usage

```bash
# Launch with defaults
sc

# Continue most recent session
sc --continue

# Resume a specific session
sc --resume <session-id>

# Set model and permission mode
sc --model claude-sonnet-4-5-20250929 --permission-mode plan

# Bypass all permission checks
sc --dangerously-skip-permissions

# Specify a theme
sc --theme nord

# Set budget limit
sc --max-budget-usd 5.00

# Show help
sc --help
```

### CLI Flags

| Flag | Description |
|------|-------------|
| `--theme <name>` | Theme name (e.g., catppuccin-mocha, nord, dracula) |
| `--model <model>` | Claude model to use |
| `--effort <level>` | Effort level: low, medium, high |
| `--max-budget-usd <amount>` | Maximum spend per session in USD |
| `--permission-mode <mode>` | Permission mode: default, plan, acceptEdits, bypassPermissions, delegate, dontAsk |
| `--dangerously-skip-permissions` | Bypass all permission checks |
| `--allowed-tools <tool>` | Auto-allow specific tools (repeatable) |
| `--mcp-config <path>` | Path to MCP server config file |
| `--continue` | Continue the most recent session |
| `--resume <id>` | Resume a specific session by ID |
| `--config <path>` | Path to config file |

### Key Bindings

| Key | Action |
|-----|--------|
| `Ctrl+K` | Open action menu |
| `Ctrl+S` | Toggle split pane (conversation + file/diff) |
| `Ctrl+A` | Agent teams dashboard |
| `Ctrl+D` | Diff viewer (all session edits) |
| `Ctrl+F` | File context panel |
| `Ctrl+E` | Expand/collapse tool output blocks |
| `Ctrl+R` | Search input history |
| `Ctrl+T` | Switch theme |
| `Ctrl+W` | Workflow templates |
| `Ctrl+P` | Plugin browser |
| `Ctrl+M` | Auto-memory viewer |
| `Ctrl+I` | CLAUDE.md instructions viewer |
| `PageUp/Down` | Scroll conversation |
| `Shift+PageUp/Down` | Scroll split pane |
| `Ctrl+Q` | Quit |

### Action Menu (Ctrl+K)

The action menu provides quick access to all features: session management, slash commands (/compact, /model, /config, etc.), workflow templates, theme switching, split pane toggle, agent dashboard, and more.

## Configuration

Config file: `~/.config/sexy-claude/config.toml`

```toml
# Command to wrap (default: "claude")
command = "claude"

# Theme name
theme = "catppuccin-mocha"

# Render framerate
fps = 30

# Claude model
model = "claude-sonnet-4-5-20250929"

# Effort level
effort = "high"

# Max budget per session (USD)
max_budget_usd = 10.0

# Permission mode
permission_mode = "default"

# Auto-allowed tools
allowed_tools = ["Bash", "Read"]

# MCP server config path
mcp_config = "/path/to/mcp.json"

[layout]
# Claude pane width percentage (20-100)
claude_pane_percent = 100
```

## Themes

10+ bundled themes are included. Custom themes go in `~/.config/sexy-claude/themes/`.

Browse community themes at [sexy-claude-themes](https://github.com/MagnusPladsen/sexy-claude-themes).

```toml
name = "My Theme"

[colors]
background = "#1e1e2e"
foreground = "#cdd6f4"
primary = "#cba6f7"
border = "#585b70"
border_focused = "#cba6f7"
status_bg = "#181825"
status_fg = "#a6adc8"
# ... see themes/catppuccin-mocha.toml for all fields
```

## Architecture

sexy-claude spawns Claude Code as a child process using the stream-json protocol (`--output-format stream-json --input-format stream-json`). Events are parsed in real-time and rendered through Ratatui.

```
User Input --> stream-json stdin --> Claude CLI
                                        |
Claude CLI stdout --> Event Parser --> Conversation Model
                                        |
                                   Ratatui Renderer
                                     + Theme
                                     + Split Pane
                                     + Status Bar (cost, tokens, git, tools)
                                     + Overlays (menus, pickers, dashboards)
```

### Key crates

- `ratatui` + `crossterm` — TUI framework and terminal backend
- `tokio` — Async runtime for concurrent event handling
- `clap` — CLI argument parsing
- `serde` + `serde_json` + `toml` — Configuration and event parsing
- `fuzzy-matcher` — Fuzzy search for history and completions

## Requirements

- macOS or Linux
- Rust 1.80+ (for building)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed (`npm install -g @anthropic-ai/claude-code`)

## License

MIT
