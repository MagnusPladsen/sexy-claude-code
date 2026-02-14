# Theme Switcher + Action Menu + Default Themes

Addresses: Issue #1 (Theme Switcher), Issue #4 (More Default Themes), Issue #5 (Keybindings Menu)

## Summary

Add a VS Code-style floating overlay system with a filterable action menu (Ctrl+K) and theme picker (Ctrl+T). Include 10 popular default themes with live preview and config persistence.

## App Mode & State

`App` gains a mode enum:

```rust
enum AppMode {
    Normal,
    ActionMenu(OverlayState),
    ThemePicker(OverlayState),
}

struct OverlayState {
    items: Vec<OverlayItem>,
    selected: usize,
    filter: String,
    original_theme: Option<String>,  // For live preview revert on Escape
}

struct OverlayItem {
    label: String,
    value: String,   // theme name or action id
    hint: String,    // shortcut hint displayed on right
}
```

## Key Handling

- **Normal mode:** Keys pass to PTY. Ctrl+K sets chord pending. Ctrl+T opens ThemePicker. Ctrl+Q quits.
- **Chord pending (Ctrl+K):** Next key resolves — `t` opens ThemePicker, unrecognized key cancels chord and passes both keys to PTY.
- **Overlay mode:** Up/Down navigate, typing filters, Enter confirms, Escape cancels.

## Overlay Widget (`src/ui/overlay.rs`)

Single reusable popup rendered over the Claude pane:

```
┌─ Select Theme ──────────┐
│ > filter text_           │
├─────────────────────────┤
│   Catppuccin Mocha       │
│ ▸ Tokyo Night            │
│   Monokai Pro            │
│   Dracula                │
└─────────────────────────┘
```

- Centered, ~50% width, height scales with items (max ~60%)
- Rounded borders, themed colors
- Filter input at top, scrollable item list below
- Highlighted item uses primary color

## Action Menu

Default actions via Ctrl+K:

| Action | Key | Description |
|--------|-----|-------------|
| Switch Theme | t | Open theme picker |
| Quit | q | Exit sexy-claude |

Extensible — actions are a simple Vec.

## Theme Picker with Live Preview

1. Store `original_theme` on open
2. As selection changes, load and swap `self.theme`
3. Enter: persist to config, close overlay
4. Escape: restore original theme, close overlay

## Config Persistence

`config::save_theme(name)` reads `~/.config/sexy-claude/config.toml`, updates `theme` field, writes back preserving other values. Creates file and parent directories if needed.

## Theme Discovery

`Theme::list_available()` scans:
1. `themes/` next to executable (bundled)
2. `~/.config/sexy-claude/themes/` (user themes)

Returns deduplicated sorted Vec of theme names.

## Default Themes

| File | Based On | Credit |
|------|----------|--------|
| catppuccin-mocha.toml | Catppuccin Mocha | catppuccin/catppuccin |
| tokyo-night.toml | Tokyo Night | enkia/tokyo-night-vscode-theme |
| monokai-pro.toml | Monokai Pro Spectrum | monokai/monokai-pro-vscode |
| dracula.toml | Dracula | dracula/visual-studio-code |
| gruvbox-dark.toml | Gruvbox | morhetz/gruvbox |
| nord.toml | Nord | nordtheme/visual-studio-code |
| one-dark.toml | One Dark Pro | Binaryify/OneDark-Pro |
| rose-pine.toml | Rosé Pine | rose-pine/vscode |
| solarized-dark.toml | Solarized | ethanschoonover.com/solarized |
| kanagawa.toml | Kanagawa | rebelot/kanagawa.nvim |

Each file includes comment header crediting original creator with link.

## File Changes

New files:
- `src/ui/overlay.rs` — Reusable overlay popup widget
- `themes/*.toml` — 9 new theme files

Modified files:
- `src/app.rs` — AppMode enum, chord handling, overlay state management
- `src/ui/mod.rs` — Render overlay on top of normal layout
- `src/theme.rs` — `list_available()` function
- `src/config.rs` — `save_theme()` function
- `src/keybindings.rs` — Chord state and keybinding definitions
