use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::theme::Theme;

pub struct StatusBar<'a> {
    theme_name: &'a str,
    theme: &'a Theme,
}

impl<'a> StatusBar<'a> {
    pub fn new(theme_name: &'a str, theme: &'a Theme) -> Self {
        Self { theme_name, theme }
    }
}

impl<'a> Widget for StatusBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default()
            .fg(self.theme.status_fg)
            .bg(self.theme.status_bg);

        // Fill entire bar with background
        for x in area.x..area.right() {
            buf[(x, area.y)].set_style(style);
            buf[(x, area.y)].set_symbol(" ");
        }

        // Left: app name
        let left = " sexy-claude";
        let left_style = Style::default()
            .fg(self.theme.primary)
            .bg(self.theme.status_bg);
        for (i, ch) in left.chars().enumerate() {
            let x = area.x + i as u16;
            if x >= area.right() {
                break;
            }
            buf[(x, area.y)].set_symbol(&ch.to_string());
            buf[(x, area.y)].set_style(left_style);
        }

        // Center: theme name
        let center = format!(" {} ", self.theme_name);
        let center_start = area.x + (area.width.saturating_sub(center.len() as u16)) / 2;
        for (i, ch) in center.chars().enumerate() {
            let x = center_start + i as u16;
            if x >= area.right() {
                break;
            }
            buf[(x, area.y)].set_symbol(&ch.to_string());
            buf[(x, area.y)].set_style(style);
        }

        // Right: help hint
        let right = "Ctrl+K: menu | Ctrl+T: theme | Ctrl+Q: quit ";
        let right_start = area.right().saturating_sub(right.len() as u16);
        for (i, ch) in right.chars().enumerate() {
            let x = right_start + i as u16;
            if x >= area.right() {
                break;
            }
            buf[(x, area.y)].set_symbol(&ch.to_string());
            buf[(x, area.y)].set_style(style);
        }
    }
}
