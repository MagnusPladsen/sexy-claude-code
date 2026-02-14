# sexy-claude (sc)

A beautiful terminal wrapper for [Claude Code](https://docs.anthropic.com/en/docs/claude-code) built with Rust. It re-renders Claude Code's output inside a themed TUI shell with configurable colors, styled borders, and a clean status bar — without modifying Claude Code's behavior.

## Features

- **Themed rendering** — Catppuccin Mocha by default, with support for custom TOML themes
- **Background override** — Replaces Claude Code's default dark backgrounds with your theme colors for a seamless look
- **Styled chrome** — Rounded borders, branded header, and a status bar
- **Full pass-through** — All keystrokes go directly to Claude Code; it handles its own input, prompts, and interactivity
- **PTY isolation** — Claude Code runs in a pseudo-terminal subprocess, no shared memory or raw ANSI passthrough
- **Fast** — Built on [Ratatui](https://ratatui.rs) + [Crossterm](https://github.com/crossterm-rs/crossterm), renders at 30fps with differential updates

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
# Launch with defaults (wraps `claude`)
sc

# Specify a theme
sc --theme catppuccin-mocha

# Wrap a different command
sc bash
sc -- claude --model sonnet

# Show help
sc --help
```

### Key bindings

| Key | Action |
|-----|--------|
| All keys | Passed directly to Claude Code |
| `Ctrl+Q` | Quit sexy-claude |

## Configuration

Config file: `~/.config/sexy-claude/config.toml`

```toml
# Command to wrap (default: "claude")
command = "claude"

# Theme name (looks in themes/ dir or ~/.config/sexy-claude/themes/)
theme = "catppuccin-mocha"

# Render framerate
fps = 30

[layout]
# Claude pane width percentage (20-100)
claude_pane_percent = 100
```

## Themes

Themes are TOML files with semantic color names mapped to hex values. See `themes/catppuccin-mocha.toml` for the full format.

Place custom themes in `~/.config/sexy-claude/themes/`.

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

```
User Input --> PTY stdin --> Claude Code
                                  |
Claude Code stdout --> PTY reader --> vt100 Parser
                                        |
                             Screen Buffer (cell grid)
                                        |
                            Ratatui Converter (cell->cell)
                              + theme bg override
                                        |
                             Ratatui Buffer --> Terminal
                                  +
                        Chrome (borders, status bar)
```

### Key crates

- `ratatui` + `crossterm` — TUI framework and terminal backend
- `vt100` — Terminal emulator / ANSI parser
- `portable-pty` — Cross-platform PTY (from the WezTerm project)
- `tokio` — Async runtime for concurrent event handling
- `clap` — CLI argument parsing
- `toml` + `serde` — Configuration and theme parsing

## Requirements

- macOS or Linux
- Rust 1.80+ (for building)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) installed (`npm install -g @anthropic-ai/claude-code`)

## License

MIT
