# sexy-claude (sc)

A beautiful terminal wrapper for Claude Code built with Rust.

## Project Structure

- `src/main.rs` — CLI args (clap), bootstrap
- `src/app.rs` — Elm Architecture event loop
- `src/config.rs` — TOML config parsing + defaults
- `src/theme.rs` — Theme loading + color mapping
- `src/pty/` — PTY management and process spawning
- `src/terminal/` — vt100 wrapper and cell converter
- `src/ui/` — Ratatui widgets (claude pane, input, status bar, borders)
- `src/keybindings.rs` — Key handling
- `themes/` — TOML theme files

## Conventions

- Use `anyhow::Result` for error handling in application code
- Use Elm Architecture pattern: `App` with `update(msg)` and `view(frame)`
- Theme colors are always `ratatui::style::Color::Rgb(r, g, b)`
- PTY output flows through vt100 parser before rendering — never raw passthrough
- All async code uses Tokio runtime
- Config lives at `~/.config/sexy-claude/config.toml`

## Build & Test

```bash
cargo build          # Build debug
cargo build --release # Build release
cargo test           # Run all tests
cargo run            # Run with default settings
```

## Planning Rules

When creating any implementation plan:

1. **Check GitHub issues first** — Run `gh issue list` to find relevant open issues. Pull details from related issues. If an issue maps to a plan task, reference it. Move issues to "done" when completed.
2. **Iterate 3 times** — After drafting the plan, review it 3 times. Each iteration: ask the user multiple clarifying questions about scope, edge cases, trade-offs, and priorities before finalizing.

## Binary Names

Both `sexy-claude` and `sc` are valid binary names for this project.
