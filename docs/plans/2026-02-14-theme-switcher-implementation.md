# Theme Switcher + Action Menu + Default Themes — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a VS Code-style overlay system with action menu (Ctrl+K), theme picker (Ctrl+T) with live preview and config persistence, and 10 bundled themes.

**Architecture:** Elm Architecture extended with `AppMode` enum. A reusable overlay widget renders a centered filterable popup. Theme switching hot-swaps `self.theme` for live preview and persists to `config.toml` on confirm.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, toml 0.8, serde 1, anyhow 1

---

### Task 1: Add `Theme::list_available()`

**Files:**
- Modify: `src/theme.rs:71-144` (add method to `impl Theme`)

**Step 1: Write the failing test**

Add to the bottom of the `mod tests` block in `src/theme.rs`:

```rust
#[test]
fn test_list_available_includes_default() {
    let themes = Theme::list_available();
    assert!(themes.contains(&"catppuccin-mocha".to_string()));
}

#[test]
fn test_list_available_sorted() {
    let themes = Theme::list_available();
    let mut sorted = themes.clone();
    sorted.sort();
    assert_eq!(themes, sorted);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_list_available -- --nocapture`
Expected: FAIL — `list_available` method not found

**Step 3: Write minimal implementation**

Add this method inside `impl Theme` in `src/theme.rs` (after the `default_theme()` method, around line 103):

```rust
/// Discover all available theme names from bundled and user theme dirs.
pub fn list_available() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();

    // Bundled themes next to executable: ../themes/*.toml
    let exe_themes = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("../themes")));
    if let Some(dir) = exe_themes {
        Self::scan_theme_dir(&dir, &mut names);
    }

    // User themes: ~/.config/sexy-claude/themes/*.toml
    let user_themes = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("sexy-claude")
        .join("themes");
    Self::scan_theme_dir(&user_themes, &mut names);

    // Always include the embedded default
    names.insert("catppuccin-mocha".to_string());

    names.into_iter().collect()
}

fn scan_theme_dir(dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_list_available -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/theme.rs
git commit -m "feat: add Theme::list_available() for theme discovery"
```

---

### Task 2: Add `config::save_theme()`

**Files:**
- Modify: `src/config.rs:1-75` (add function + make `default_path` public)

**Step 1: Write the failing test**

Add to `mod tests` in `src/config.rs`:

```rust
#[test]
fn test_save_theme_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save_theme("nord", &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("theme = \"nord\""));
}

#[test]
fn test_save_theme_preserves_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "command = \"custom-claude\"\ntheme = \"old\"\nfps = 60\n").unwrap();
    save_theme("dracula", &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("theme = \"dracula\""));
    assert!(content.contains("command = \"custom-claude\""));
    assert!(content.contains("fps = 60"));
}

#[test]
fn test_save_theme_adds_to_existing_without_theme() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "fps = 45\n").unwrap();
    save_theme("nord", &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("theme = \"nord\""));
    assert!(content.contains("fps = 45"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_save_theme -- --nocapture`
Expected: FAIL — `save_theme` not found

**Step 3: Write minimal implementation**

Add this function and make `default_path` public in `src/config.rs`:

Change `fn default_path()` to `pub fn default_path()` (line 60).

Add after the `impl Config` block (after line 75):

```rust
/// Save the selected theme name to the config file.
/// Preserves all other config values. Creates the file and parent dirs if needed.
pub fn save_theme(theme_name: &str, path: &std::path::Path) -> Result<()> {
    use std::collections::BTreeMap;

    // Read existing config as a generic TOML table (preserves unknown fields)
    let mut table: BTreeMap<String, toml::Value> = if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        toml::from_str(&content).unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    table.insert(
        "theme".to_string(),
        toml::Value::String(theme_name.to_string()),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(&table)
        .context("Failed to serialize config")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write config to {}", path.display()))?;

    Ok(())
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_save_theme -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add save_theme() for persisting theme selection"
```

---

### Task 3: Add overlay state types and widget

**Files:**
- Create: `src/ui/overlay.rs`
- Modify: `src/ui/mod.rs:1` (add `pub mod overlay;`)

**Step 1: Create `src/ui/overlay.rs` with types and widget**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct OverlayItem {
    pub label: String,
    pub value: String,
    pub hint: String,
}

#[derive(Debug)]
pub struct OverlayState {
    pub items: Vec<OverlayItem>,
    pub selected: usize,
    pub filter: String,
    pub original_theme: Option<String>,
}

impl OverlayState {
    pub fn new(items: Vec<OverlayItem>, original_theme: Option<String>) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
            original_theme,
        }
    }

    pub fn filtered_items(&self) -> Vec<(usize, &OverlayItem)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if self.filter.is_empty() {
                    return true;
                }
                let lower = item.label.to_lowercase();
                let filter = self.filter.to_lowercase();
                lower.contains(&filter)
            })
            .collect()
    }

    pub fn move_up(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        }
    }

    pub fn move_down(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn selected_value(&self) -> Option<String> {
        let filtered = self.filtered_items();
        filtered.get(self.selected).map(|(_, item)| item.value.clone())
    }

    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}

/// Reusable overlay popup widget.
pub struct OverlayWidget<'a> {
    pub title: &'a str,
    pub state: &'a OverlayState,
    pub theme: &'a Theme,
}

impl<'a> OverlayWidget<'a> {
    pub fn new(title: &'a str, state: &'a OverlayState, theme: &'a Theme) -> Self {
        Self { title, state, theme }
    }

    /// Calculate the centered popup area.
    pub fn popup_area(&self, screen: Rect) -> Rect {
        let filtered_count = self.state.filtered_items().len() as u16;
        // Width: ~50% of screen, min 30, max 60
        let width = screen.width.saturating_mul(50) / 100;
        let width = width.clamp(30, 60).min(screen.width.saturating_sub(4));
        // Height: filter line + border (2) + items, max ~60% of screen
        let content_height = 1 + filtered_count; // filter row + items
        let max_height = screen.height.saturating_mul(60) / 100;
        let height = (content_height + 2).clamp(5, max_height); // +2 for borders

        let x = screen.x + (screen.width.saturating_sub(width)) / 2;
        let y = screen.y + (screen.height.saturating_sub(height)) / 2;

        Rect::new(x, y, width, height)
    }
}

impl Widget for OverlayWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup = self.popup_area(area);

        // Clear the area behind the popup
        Clear.render(popup, buf);

        // Draw border
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(self.theme.border_focused))
            .style(Style::default().bg(self.theme.surface).fg(self.theme.foreground));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Row 0: filter input
        let filter_y = inner.y;
        let prompt = "> ";
        let filter_text = format!("{}{}", prompt, self.state.filter);
        let filter_style = Style::default().fg(self.theme.accent).bg(self.theme.surface);
        for (i, ch) in filter_text.chars().enumerate() {
            let x = inner.x + i as u16;
            if x >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((x, filter_y)) {
                cell.set_char(ch);
                cell.set_style(filter_style);
            }
        }
        // Cursor indicator
        let cursor_x = inner.x + filter_text.len() as u16;
        if cursor_x < inner.right() {
            if let Some(cell) = buf.cell_mut((cursor_x, filter_y)) {
                cell.set_char('_');
                cell.set_style(Style::default().fg(self.theme.input_cursor).bg(self.theme.surface));
            }
        }
        // Fill rest of filter row
        for x in (cursor_x + 1)..inner.right() {
            if let Some(cell) = buf.cell_mut((x, filter_y)) {
                cell.set_char(' ');
                cell.set_style(Style::default().bg(self.theme.surface));
            }
        }

        // Separator line
        let sep_y = filter_y + 1;
        if sep_y < inner.bottom() {
            for x in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut((x, sep_y)) {
                    cell.set_char('─');
                    cell.set_style(Style::default().fg(self.theme.border).bg(self.theme.surface));
                }
            }
        }

        // Item list
        let items_start_y = sep_y + 1;
        let filtered = self.state.filtered_items();
        let max_visible = (inner.bottom().saturating_sub(items_start_y)) as usize;

        // Scroll offset so selected item is always visible
        let scroll = if self.state.selected >= max_visible {
            self.state.selected - max_visible + 1
        } else {
            0
        };

        for (vi, (_, item)) in filtered.iter().skip(scroll).take(max_visible).enumerate() {
            let y = items_start_y + vi as u16;
            if y >= inner.bottom() {
                break;
            }

            let is_selected = vi + scroll == self.state.selected;
            let marker = if is_selected { " ▸ " } else { "   " };
            let label = &item.label;
            let hint = &item.hint;

            let style = if is_selected {
                Style::default()
                    .fg(self.theme.primary)
                    .bg(self.theme.overlay)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.foreground).bg(self.theme.surface)
            };

            // Fill row background
            for x in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ');
                    cell.set_style(style);
                }
            }

            // Write marker + label
            let text = format!("{}{}", marker, label);
            for (i, ch) in text.chars().enumerate() {
                let x = inner.x + i as u16;
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }

            // Write hint on the right side
            if !hint.is_empty() {
                let hint_style = if is_selected {
                    Style::default().fg(self.theme.secondary).bg(self.theme.overlay)
                } else {
                    Style::default().fg(self.theme.border).bg(self.theme.surface)
                };
                let hint_start = inner.right().saturating_sub(hint.len() as u16 + 1);
                for (i, ch) in hint.chars().enumerate() {
                    let x = hint_start + i as u16;
                    if x >= inner.right() || x <= inner.x + text.len() as u16 {
                        continue;
                    }
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        cell.set_style(hint_style);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, value: &str, hint: &str) -> OverlayItem {
        OverlayItem {
            label: label.to_string(),
            value: value.to_string(),
            hint: hint.to_string(),
        }
    }

    #[test]
    fn test_overlay_state_navigation() {
        let mut state = OverlayState::new(
            vec![item("A", "a", ""), item("B", "b", ""), item("C", "c", "")],
            None,
        );
        assert_eq!(state.selected, 0);
        state.move_down();
        assert_eq!(state.selected, 1);
        state.move_down();
        assert_eq!(state.selected, 2);
        state.move_down();
        assert_eq!(state.selected, 0); // wraps
        state.move_up();
        assert_eq!(state.selected, 2); // wraps back
    }

    #[test]
    fn test_overlay_state_filter() {
        let mut state = OverlayState::new(
            vec![
                item("Catppuccin Mocha", "catppuccin-mocha", ""),
                item("Tokyo Night", "tokyo-night", ""),
                item("Dracula", "dracula", ""),
            ],
            None,
        );
        state.type_char('t');
        state.type_char('o');
        let filtered = state.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].1.value, "tokyo-night");
    }

    #[test]
    fn test_overlay_state_selected_value() {
        let mut state = OverlayState::new(
            vec![item("A", "val-a", ""), item("B", "val-b", "")],
            None,
        );
        assert_eq!(state.selected_value(), Some("val-a".to_string()));
        state.move_down();
        assert_eq!(state.selected_value(), Some("val-b".to_string()));
    }

    #[test]
    fn test_overlay_state_backspace() {
        let mut state = OverlayState::new(vec![item("A", "a", "")], None);
        state.type_char('x');
        assert_eq!(state.filter, "x");
        state.backspace();
        assert_eq!(state.filter, "");
    }

    #[test]
    fn test_overlay_widget_renders_without_panic() {
        let theme = crate::theme::Theme::default_theme();
        let state = OverlayState::new(
            vec![item("Theme A", "a", "Ctrl+A"), item("Theme B", "b", "")],
            None,
        );
        let widget = OverlayWidget::new("Test", &state, &theme);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }
}
```

**Step 2: Register the module**

Add `pub mod overlay;` to `src/ui/mod.rs` (after line 3, with the other mods).

**Step 3: Run tests**

Run: `cargo test overlay -- --nocapture`
Expected: PASS (all 5 overlay tests)

**Step 4: Commit**

```bash
git add src/ui/overlay.rs src/ui/mod.rs
git commit -m "feat: add reusable overlay widget with filterable list"
```

---

### Task 4: Add AppMode and key handling to App

**Files:**
- Modify: `src/app.rs` (add AppMode, chord state, overlay key handling)

**Step 1: Add AppMode and modify App struct**

At the top of `src/app.rs`, add the import and types (after existing imports, before `enum Msg`):

```rust
use crate::ui::overlay::{OverlayItem, OverlayState};
```

Add after the `enum Msg` block:

```rust
enum AppMode {
    Normal,
    ChordPending, // Ctrl+K was pressed, waiting for next key
    ActionMenu(OverlayState),
    ThemePicker(OverlayState),
}
```

Add `mode: AppMode` field to `App` struct. Add `original_theme_name: String` to track the theme name for revert.

Update `App::new()` to initialize:
```rust
mode: AppMode::Normal,
original_theme_name: theme.name.clone(),  // store for theme name tracking
```

Wait — we need the theme *slug* not display name. Add a `theme_name: String` field:

In `App::new()`, accept the theme slug and store it:
- Add parameter: `theme_name: String`
- Store: `theme_name,`
- Initialize: `mode: AppMode::Normal,`

In `src/main.rs`, pass the theme name to `App::new()`:
```rust
let mut app = app::App::new(config, theme, theme_name.to_string(), pty_process, rows, cols);
```

**Step 2: Rewrite `handle_key` for modal dispatch**

Replace the existing `handle_key` method:

```rust
fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
    match &self.mode {
        AppMode::Normal => self.handle_key_normal(key),
        AppMode::ChordPending => self.handle_key_chord(key),
        AppMode::ActionMenu(_) | AppMode::ThemePicker(_) => self.handle_key_overlay(key),
    }
}

fn handle_key_normal(&mut self, key: event::KeyEvent) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if ctrl && key.code == KeyCode::Char('q') {
        self.should_quit = true;
        return Ok(());
    }

    if ctrl && key.code == KeyCode::Char('k') {
        self.mode = AppMode::ChordPending;
        return Ok(());
    }

    if ctrl && key.code == KeyCode::Char('t') {
        self.open_theme_picker();
        return Ok(());
    }

    // Pass through to PTY
    let bytes = key_to_bytes(&key);
    if !bytes.is_empty() {
        self.pty_write(&bytes)?;
    }
    Ok(())
}

fn handle_key_chord(&mut self, key: event::KeyEvent) -> Result<()> {
    self.mode = AppMode::Normal; // Reset chord state regardless

    match key.code {
        KeyCode::Char('t') => {
            self.open_theme_picker();
        }
        KeyCode::Char('q') => {
            self.should_quit = true;
        }
        _ => {
            // Unrecognized chord — cancel silently
        }
    }
    Ok(())
}

fn handle_key_overlay(&mut self, key: event::KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            self.close_overlay(false);
        }
        KeyCode::Enter => {
            self.confirm_overlay()?;
        }
        KeyCode::Up => {
            if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                state.move_up();
            }
            self.preview_theme();
        }
        KeyCode::Down => {
            if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                state.move_down();
            }
            self.preview_theme();
        }
        KeyCode::Backspace => {
            if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                state.backspace();
            }
        }
        KeyCode::Char(c) => {
            if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                state.type_char(c);
            }
        }
        _ => {}
    }
    Ok(())
}

fn open_theme_picker(&mut self) {
    let themes = crate::theme::Theme::list_available();
    let items: Vec<OverlayItem> = themes
        .iter()
        .map(|name| {
            // Load theme to get display name, fall back to slug
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

    // Pre-select current theme
    let current_idx = items.iter().position(|i| i.value == self.theme_name).unwrap_or(0);
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

fn close_overlay(&mut self, _confirmed: bool) {
    // Restore original theme if cancelling theme picker
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
    // Take the mode out to avoid borrow issues
    let mode = std::mem::replace(&mut self.mode, AppMode::Normal);

    match mode {
        AppMode::ThemePicker(state) => {
            if let Some(value) = state.selected_value() {
                // Load and apply the theme
                if let Ok(new_theme) = crate::theme::Theme::load(&value) {
                    self.theme = new_theme;
                    self.theme_name = value.clone();
                    // Persist to config
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
        _ => {}
    }
    Ok(())
}
```

**Step 3: Update `handle_key_chord` to open action menu for unbound keys**

Actually, looking at the design again — Ctrl+K should open the action menu as a filterable list, not just wait for a second key. Let me adjust:

Replace `handle_key_chord`:

```rust
fn handle_key_chord(&mut self, key: event::KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('t') => {
            self.mode = AppMode::Normal;
            self.open_theme_picker();
        }
        KeyCode::Char('q') => {
            self.mode = AppMode::Normal;
            self.should_quit = true;
        }
        _ => {
            // Any unrecognized key — open the full action menu
            self.open_action_menu();
        }
    }
    Ok(())
}
```

Also update `handle_key_normal` so that Ctrl+K directly opens the action menu (not chord pending):

```rust
if ctrl && key.code == KeyCode::Char('k') {
    self.open_action_menu();
    return Ok(());
}
```

This is simpler — Ctrl+K always opens the action menu. The chord shortcuts (Ctrl+K then t) still work because the action menu handles key input.

Remove `AppMode::ChordPending` entirely and simplify. Replace with just `Normal`, `ActionMenu`, `ThemePicker`.

**Step 4: Update `view()` to pass mode info**

Update `view()` to pass the mode to the render function:

```rust
fn view(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
    let screen = self.emulator.screen();
    let theme = &self.theme;
    let frame_count = self.frame_count;
    let overlay = match &self.mode {
        AppMode::ActionMenu(state) => Some(("Actions", state)),
        AppMode::ThemePicker(state) => Some(("Select Theme", state)),
        _ => None,
    };

    terminal.draw(|frame| {
        ui::render(frame, screen, theme, frame_count);
        if let Some((title, state)) = overlay {
            ui::render_overlay(frame, title, state, theme);
        }
    })?;

    Ok(())
}
```

**Step 5: Run `cargo check`**

Run: `cargo check`
Expected: Errors about missing `ui::render_overlay` — that's Task 5.

**Step 6: Commit (WIP — will compile after Task 5)**

```bash
git add src/app.rs src/main.rs
git commit -m "feat: add AppMode with action menu and theme picker key handling"
```

---

### Task 5: Wire overlay rendering into UI

**Files:**
- Modify: `src/ui/mod.rs` (add `render_overlay` function)

**Step 1: Add the render_overlay function**

Add to `src/ui/mod.rs`:

```rust
use overlay::{OverlayState, OverlayWidget};
```

And the function:

```rust
/// Render an overlay popup on top of the existing UI.
pub fn render_overlay(
    frame: &mut Frame,
    title: &str,
    state: &OverlayState,
    theme: &Theme,
) {
    let widget = OverlayWidget::new(title, state, theme);
    frame.render_widget(widget, frame.area());
}
```

**Step 2: Run `cargo check`**

Run: `cargo check`
Expected: PASS (compiles)

**Step 3: Run all tests**

Run: `cargo test`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: wire overlay rendering into UI layer"
```

---

### Task 6: Update status bar with new keybinding hints

**Files:**
- Modify: `src/ui/status_bar.rs:57-58`

**Step 1: Update the hint text**

Change line 58 in `src/ui/status_bar.rs`:

From: `let right = "Ctrl+Q: quit ";`
To: `let right = "Ctrl+K: menu | Ctrl+T: theme | Ctrl+Q: quit ";`

**Step 2: Run `cargo check`**

Run: `cargo check`
Expected: PASS

**Step 3: Commit**

```bash
git add src/ui/status_bar.rs
git commit -m "feat: update status bar with action menu and theme picker hints"
```

---

### Task 7: Create default theme files

**Files:**
- Create: `themes/tokyo-night.toml`
- Create: `themes/monokai-pro.toml`
- Create: `themes/dracula.toml`
- Create: `themes/gruvbox-dark.toml`
- Create: `themes/nord.toml`
- Create: `themes/one-dark.toml`
- Create: `themes/rose-pine.toml`
- Create: `themes/solarized-dark.toml`
- Create: `themes/kanagawa.toml`

**Step 1: Create all 9 theme files**

Each theme file follows the same TOML format as `catppuccin-mocha.toml` with a credit comment at the top. See the theme color values below (sourced from original theme repos):

**tokyo-night.toml:**
```toml
# Tokyo Night — inspired by the Tokyo Night VS Code theme
# Credit: enkia/tokyo-night-vscode-theme
# https://github.com/enkia/tokyo-night-vscode-theme
name = "Tokyo Night"

[colors]
background = "#1a1b26"
foreground = "#c0caf5"
surface = "#24283b"
overlay = "#414868"

primary = "#7aa2f7"
secondary = "#bb9af7"
accent = "#7dcfff"

success = "#9ece6a"
warning = "#e0af68"
error = "#f7768e"
info = "#2ac3de"

border = "#3b4261"
border_focused = "#7aa2f7"

status_bg = "#16161e"
status_fg = "#a9b1d6"

input_bg = "#24283b"
input_fg = "#c0caf5"
input_cursor = "#c0caf5"
input_placeholder = "#565f89"
```

**monokai-pro.toml:**
```toml
# Monokai Pro (Spectrum filter) — inspired by the Monokai Pro theme
# Credit: monokai/monokai-pro-vscode
# https://monokai.pro
name = "Monokai Pro"

[colors]
background = "#222222"
foreground = "#f7f1ff"
surface = "#2d2a2e"
overlay = "#403e41"

primary = "#fc9867"
secondary = "#ab9df2"
accent = "#ff6188"

success = "#a9dc76"
warning = "#ffd866"
error = "#ff6188"
info = "#78dce8"

border = "#5b595c"
border_focused = "#fc9867"

status_bg = "#19181a"
status_fg = "#c1c0c0"

input_bg = "#2d2a2e"
input_fg = "#f7f1ff"
input_cursor = "#fcfcfa"
input_placeholder = "#727072"
```

**dracula.toml:**
```toml
# Dracula — inspired by the Dracula theme
# Credit: dracula/visual-studio-code
# https://github.com/dracula/visual-studio-code
name = "Dracula"

[colors]
background = "#282a36"
foreground = "#f8f8f2"
surface = "#343746"
overlay = "#44475a"

primary = "#bd93f9"
secondary = "#8be9fd"
accent = "#ff79c6"

success = "#50fa7b"
warning = "#f1fa8c"
error = "#ff5555"
info = "#8be9fd"

border = "#6272a4"
border_focused = "#bd93f9"

status_bg = "#21222c"
status_fg = "#f8f8f2"

input_bg = "#343746"
input_fg = "#f8f8f2"
input_cursor = "#f8f8f2"
input_placeholder = "#6272a4"
```

**gruvbox-dark.toml:**
```toml
# Gruvbox Dark — inspired by the Gruvbox color scheme
# Credit: morhetz/gruvbox
# https://github.com/morhetz/gruvbox
name = "Gruvbox Dark"

[colors]
background = "#282828"
foreground = "#ebdbb2"
surface = "#3c3836"
overlay = "#504945"

primary = "#fabd2f"
secondary = "#83a598"
accent = "#d3869b"

success = "#b8bb26"
warning = "#fabd2f"
error = "#fb4934"
info = "#83a598"

border = "#665c54"
border_focused = "#fabd2f"

status_bg = "#1d2021"
status_fg = "#a89984"

input_bg = "#3c3836"
input_fg = "#ebdbb2"
input_cursor = "#ebdbb2"
input_placeholder = "#928374"
```

**nord.toml:**
```toml
# Nord — inspired by the Nord color palette
# Credit: nordtheme/visual-studio-code
# https://www.nordtheme.com
name = "Nord"

[colors]
background = "#2e3440"
foreground = "#d8dee9"
surface = "#3b4252"
overlay = "#434c5e"

primary = "#88c0d0"
secondary = "#81a1c1"
accent = "#b48ead"

success = "#a3be8c"
warning = "#ebcb8b"
error = "#bf616a"
info = "#5e81ac"

border = "#4c566a"
border_focused = "#88c0d0"

status_bg = "#272c36"
status_fg = "#d8dee9"

input_bg = "#3b4252"
input_fg = "#eceff4"
input_cursor = "#d8dee9"
input_placeholder = "#4c566a"
```

**one-dark.toml:**
```toml
# One Dark — inspired by Atom's One Dark theme
# Credit: Binaryify/OneDark-Pro
# https://github.com/Binaryify/OneDark-Pro
name = "One Dark"

[colors]
background = "#282c34"
foreground = "#abb2bf"
surface = "#21252b"
overlay = "#3e4451"

primary = "#61afef"
secondary = "#c678dd"
accent = "#e06c75"

success = "#98c379"
warning = "#e5c07b"
error = "#e06c75"
info = "#56b6c2"

border = "#3b4048"
border_focused = "#61afef"

status_bg = "#21252b"
status_fg = "#9da5b4"

input_bg = "#2c313a"
input_fg = "#abb2bf"
input_cursor = "#528bff"
input_placeholder = "#5c6370"
```

**rose-pine.toml:**
```toml
# Rosé Pine — inspired by the Rosé Pine theme
# Credit: rose-pine/vscode
# https://rosepinetheme.com
name = "Rosé Pine"

[colors]
background = "#191724"
foreground = "#e0def4"
surface = "#1f1d2e"
overlay = "#26233a"

primary = "#c4a7e7"
secondary = "#9ccfd8"
accent = "#ebbcba"

success = "#31748f"
warning = "#f6c177"
error = "#eb6f92"
info = "#9ccfd8"

border = "#403d52"
border_focused = "#c4a7e7"

status_bg = "#16141f"
status_fg = "#908caa"

input_bg = "#1f1d2e"
input_fg = "#e0def4"
input_cursor = "#e0def4"
input_placeholder = "#6e6a86"
```

**solarized-dark.toml:**
```toml
# Solarized Dark — inspired by Ethan Schoonover's Solarized
# Credit: Ethan Schoonover
# https://ethanschoonover.com/solarized/
name = "Solarized Dark"

[colors]
background = "#002b36"
foreground = "#839496"
surface = "#073642"
overlay = "#094856"

primary = "#268bd2"
secondary = "#2aa198"
accent = "#d33682"

success = "#859900"
warning = "#b58900"
error = "#dc322f"
info = "#6c71c4"

border = "#586e75"
border_focused = "#268bd2"

status_bg = "#00232c"
status_fg = "#93a1a1"

input_bg = "#073642"
input_fg = "#839496"
input_cursor = "#839496"
input_placeholder = "#586e75"
```

**kanagawa.toml:**
```toml
# Kanagawa — inspired by the colors of the famous painting by Katsushika Hokusai
# Credit: rebelot/kanagawa.nvim
# https://github.com/rebelot/kanagawa.nvim
name = "Kanagawa"

[colors]
background = "#1f1f28"
foreground = "#dcd7ba"
surface = "#2a2a37"
overlay = "#363646"

primary = "#7e9cd8"
secondary = "#957fb8"
accent = "#d27e99"

success = "#76946a"
warning = "#e6c384"
error = "#c34043"
info = "#7fb4ca"

border = "#54546d"
border_focused = "#7e9cd8"

status_bg = "#16161d"
status_fg = "#c8c093"

input_bg = "#2a2a37"
input_fg = "#dcd7ba"
input_cursor = "#dcd7ba"
input_placeholder = "#727169"
```

**Step 2: Verify all themes parse correctly**

Run: `cargo test test_load -- --nocapture`
Then also add a quick test in `src/theme.rs`:

```rust
#[test]
fn test_all_bundled_themes_parse() {
    let theme_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    for entry in std::fs::read_dir(&theme_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            let content = std::fs::read_to_string(&path).unwrap();
            let result = Theme::from_toml(&content);
            assert!(result.is_ok(), "Failed to parse theme {}: {:?}", path.display(), result.err());
        }
    }
}
```

Run: `cargo test test_all_bundled_themes_parse -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add themes/*.toml src/theme.rs
git commit -m "feat: add 9 bundled themes (tokyo-night, monokai-pro, dracula, gruvbox, nord, one-dark, rose-pine, solarized-dark, kanagawa)"
```

---

### Task 8: Full integration — build and manual test

**Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

**Step 2: Build release**

Run: `cargo build --release`
Expected: PASS

**Step 3: Manual smoke test**

Run: `cargo run`

Verify:
- App launches normally
- Status bar shows `Ctrl+K: menu | Ctrl+T: theme | Ctrl+Q: quit`
- Press Ctrl+T → theme picker overlay appears centered
- Arrow up/down navigates themes, background live-previews
- Type to filter themes (e.g., "dra" shows only Dracula)
- Press Enter to confirm, Escape to cancel
- Press Ctrl+K → action menu appears with "Switch Theme" and "Quit"
- Selected theme persists in `~/.config/sexy-claude/config.toml`

**Step 4: Commit any fixes**

If any fixes were needed, commit them.

---

### Task 9: Final cleanup and squash commit

**Step 1: Run all tests one final time**

Run: `cargo test`
Expected: All PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Fix any warnings if present.

**Step 3: Commit any final fixes**

```bash
git add -A
git commit -m "chore: clippy fixes and final cleanup"
```
