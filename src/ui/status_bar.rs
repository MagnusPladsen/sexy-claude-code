use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::git::GitInfo;
use crate::theme::Theme;

pub struct StatusBar<'a> {
    theme_name: &'a str,
    theme: &'a Theme,
    input_tokens: u64,
    output_tokens: u64,
    git_info: &'a GitInfo,
}

impl<'a> StatusBar<'a> {
    pub fn new(
        theme_name: &'a str,
        theme: &'a Theme,
        input_tokens: u64,
        output_tokens: u64,
        git_info: &'a GitInfo,
    ) -> Self {
        Self {
            theme_name,
            theme,
            input_tokens,
            output_tokens,
            git_info,
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

/// Write a string into the buffer at (start_x, y) with the given style.
/// Returns the x position after the last written character.
fn write_str(buf: &mut Buffer, text: &str, x_start: u16, y: u16, x_limit: u16, style: Style) -> u16 {
    let mut x = x_start;
    for ch in text.chars() {
        if x >= x_limit {
            break;
        }
        buf[(x, y)].set_symbol(&ch.to_string());
        buf[(x, y)].set_style(style);
        x += 1;
    }
    x
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
        let mut left_end = write_str(buf, left, area.x, area.y, area.right(), left_style);

        // Git branch info (right after app name)
        if let Some(display) = self.git_info.display() {
            let sep = " | ";
            left_end = write_str(buf, sep, left_end, area.y, area.right(), style);

            let git_color = if self.git_info.is_dirty() {
                self.theme.warning
            } else {
                self.theme.success
            };
            let git_style = Style::default()
                .fg(git_color)
                .bg(self.theme.status_bg);
            write_str(buf, &display, left_end, area.y, area.right(), git_style);
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
        write_str(buf, &center, center_start, area.y, area.right(), style);

        // Right: help hint
        let right = "Ctrl+K: menu | Ctrl+T: theme | Ctrl+Q: quit ";
        let right_start = area.right().saturating_sub(right.len() as u16);
        write_str(buf, right, right_start, area.y, area.right(), style);
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
