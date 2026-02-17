# Plugin Browser UI Design

## Overview

Visual browser for discovering and managing Claude Code plugins from within sexy-claude.

## Trigger

- `Ctrl+P` keyboard shortcut
- `/plugins` local slash command

## Data Sources

1. **Available plugins**: Scan `~/.claude/plugins/marketplaces/*/plugins/` and `*/external_plugins/` directories. Read `.claude-plugin/plugin.json` for name, description, author. Check for `.mcp.json` to determine if MCP type.
2. **Installed plugins**: Parse `~/.claude/plugins/installed_plugins.json`. Match by `name@marketplace` key.
3. **Enabled plugins**: Parse `~/.claude/settings.json` → `enabledPlugins` map.

## UI: AppMode::PluginBrowser

Scrollable list overlay (similar to action menu / session picker):

```
┌─ Plugins (28 available, 16 enabled) ─────────────────┐
│ [+] agent-sdk-dev          — Agent SDK dev tools      │
│ [-] atlassian              — Atlassian integration     │
│ [+] claude-code-setup      — Setup recommendations     │
│ [ ] clangd-lsp             — C/C++ language server     │
│ [+] code-review            — Automated code review     │
│ ...                                                    │
│                                                        │
│ Enter:readme  Space:toggle  i:install  u:uninstall     │
└────────────────────────────────────────────────────────┘
```

- `[+]` = installed + enabled (green)
- `[-]` = installed + disabled (yellow)
- `[ ]` = available, not installed (dim)
- MCP plugins shown with `[MCP]` suffix

## Actions

All mutations via `claude plugin` CLI (with CLAUDECODE env unset):

- **Enter**: Open plugin README in TextViewer
- **Space**: Toggle enable/disable (`claude plugin enable/disable <name>`)
- **i**: Install uninstalled plugin (`claude plugin install <name>`)
- **u**: Uninstall installed plugin (`claude plugin uninstall <name>`)
- **Esc**: Close browser

After each action, re-scan filesystem to refresh state.

## Files to Modify

- `src/app.rs` — Ctrl+P handler, `/plugins` local command, `AppMode::PluginBrowser` variant, plugin discovery, action handlers
- `src/ui/mod.rs` — `render_plugin_browser()` overlay

## Plugin Data Struct

```rust
struct PluginInfo {
    name: String,
    marketplace: String,
    description: String,
    is_mcp: bool,
    installed: bool,
    enabled: bool,
}
```
