use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
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
        filtered
            .get(self.selected)
            .map(|(_, item)| item.value.clone())
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
        Self {
            title,
            state,
            theme,
        }
    }

    /// Calculate the centered popup area.
    pub fn popup_area(&self, screen: Rect) -> Rect {
        let filtered_count = self.state.filtered_items().len() as u16;
        // Width: ~50% of screen, min 30, max 60
        let width = screen.width.saturating_mul(50) / 100;
        let width = width.clamp(30, 60).min(screen.width.saturating_sub(4));
        // Height: filter line + separator + items + borders
        let content_height = 2 + filtered_count; // filter row + separator + items
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
            .title_style(
                Style::default()
                    .fg(self.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(self.theme.border_focused))
            .style(
                Style::default()
                    .bg(self.theme.surface)
                    .fg(self.theme.foreground),
            );

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Row 0: filter input
        let filter_y = inner.y;
        let prompt = "> ";
        let filter_text = format!("{}{}", prompt, self.state.filter);
        let filter_style = Style::default()
            .fg(self.theme.accent)
            .bg(self.theme.surface);
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
                cell.set_style(
                    Style::default()
                        .fg(self.theme.input_cursor)
                        .bg(self.theme.surface),
                );
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
                    cell.set_style(
                        Style::default()
                            .fg(self.theme.border)
                            .bg(self.theme.surface),
                    );
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
                Style::default()
                    .fg(self.theme.foreground)
                    .bg(self.theme.surface)
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
                    Style::default()
                        .fg(self.theme.secondary)
                        .bg(self.theme.overlay)
                } else {
                    Style::default()
                        .fg(self.theme.border)
                        .bg(self.theme.surface)
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
        let mut state =
            OverlayState::new(vec![item("A", "val-a", ""), item("B", "val-b", "")], None);
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
