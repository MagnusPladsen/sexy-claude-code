use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::theme::Theme;

pub struct StatusBar<'a> {
    theme_name: &'a str,
    theme: &'a Theme,
    input_tokens: u64,
    output_tokens: u64,
}

impl<'a> StatusBar<'a> {
    pub fn new(
        theme_name: &'a str,
        theme: &'a Theme,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Self {
        Self {
            theme_name,
            theme,
            input_tokens,
            output_tokens,
        }
    }
}

/// Format a token count as a compact string (e.g. "1.2k", "42").
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
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

        // Center: theme name + token usage
        let center = if self.input_tokens > 0 || self.output_tokens > 0 {
            format!(
                " {} | {} in / {} out ",
                self.theme_name,
                format_tokens(self.input_tokens),
                format_tokens(self.output_tokens),
            )
        } else {
            format!(" {} ", self.theme_name)
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens_small() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(42), "42");
        assert_eq!(format_tokens(999), "999");
    }

    #[test]
    fn test_format_tokens_thousands() {
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(1234), "1.2k");
        assert_eq!(format_tokens(52800), "52.8k");
    }

    #[test]
    fn test_format_tokens_millions() {
        assert_eq!(format_tokens(1_000_000), "1.0M");
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }
}
