use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::cost;
use crate::git::GitInfo;
use crate::theme::Theme;

/// Default context window size in tokens (Claude's 200k window).
const CONTEXT_WINDOW_TOKENS: u64 = 200_000;

pub struct StatusBar<'a> {
    theme: &'a Theme,
    input_tokens: u64,
    output_tokens: u64,
    git_info: &'a GitInfo,
    todo_summary: Option<&'a str>,
    model_name: Option<&'a str>,
    permission_mode: Option<&'a str>,
}

impl<'a> StatusBar<'a> {
    pub fn new(
        theme: &'a Theme,
        input_tokens: u64,
        output_tokens: u64,
        git_info: &'a GitInfo,
        todo_summary: Option<&'a str>,
        model_name: Option<&'a str>,
        permission_mode: Option<&'a str>,
    ) -> Self {
        Self {
            theme,
            input_tokens,
            output_tokens,
            git_info,
            todo_summary,
            model_name,
            permission_mode,
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

/// Build a context budget bar string like "▓▓▓▓▓░░░░░" for the given usage ratio.
/// Returns (bar_string, fill_ratio) where fill_ratio is 0.0..=1.0.
fn context_bar(total_tokens: u64, bar_width: usize) -> (String, f64) {
    let ratio = (total_tokens as f64 / CONTEXT_WINDOW_TOKENS as f64).min(1.0);
    let filled = (ratio * bar_width as f64).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    (bar, ratio)
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

        // Permission mode indicator (after app name)
        if let Some(mode) = self.permission_mode {
            let (label, color) = match mode {
                "plan" => ("PLAN", self.theme.warning),
                "bypassPermissions" => ("BYPASS", self.theme.error),
                _ => ("DEFAULT", self.theme.success),
            };
            let sep = " | ";
            left_end = write_str(buf, sep, left_end, area.y, area.right(), style);
            let mode_style = Style::default()
                .fg(color)
                .bg(self.theme.status_bg);
            left_end = write_str(buf, label, left_end, area.y, area.right(), mode_style);
        }

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
            left_end = write_str(buf, &display, left_end, area.y, area.right(), git_style);
        }

        // Todo summary (after git info)
        if let Some(summary) = self.todo_summary {
            let sep = " | ";
            left_end = write_str(buf, sep, left_end, area.y, area.right(), style);
            let todo_style = Style::default()
                .fg(self.theme.info)
                .bg(self.theme.status_bg);
            write_str(buf, summary, left_end, area.y, area.right(), todo_style);
        }

        // Center: model | tokens | cost | context bar
        let total_tokens = self.input_tokens + self.output_tokens;
        let has_usage = total_tokens > 0;

        let short_model = self.model_name
            .map(|m| cost::short_model_name(m))
            .unwrap_or_default();

        let center_text = if has_usage {
            let pricing = self.model_name
                .map(|m| cost::pricing_for_model(m))
                .unwrap_or_else(|| cost::pricing_for_model("sonnet"));
            let session_cost = pricing.calculate_cost(self.input_tokens, self.output_tokens);
            let pct = ((total_tokens as f64 / CONTEXT_WINDOW_TOKENS as f64) * 100.0).min(100.0);
            format!(
                " {} | {} in / {} out | {} | {:.0}% ",
                short_model,
                format_tokens(self.input_tokens),
                format_tokens(self.output_tokens),
                cost::format_cost(session_cost),
                pct,
            )
        } else if !short_model.is_empty() {
            format!(" {} ", short_model)
        } else {
            String::new()
        };

        // Calculate bar width and center position
        let bar_width: usize = if has_usage { 10 } else { 0 };
        let total_center_len = center_text.len() + bar_width;
        let center_start = area.x + (area.width.saturating_sub(total_center_len as u16)) / 2;

        // Write center text
        let after_text = write_str(buf, &center_text, center_start, area.y, area.right(), style);

        // Write context bar with color coding
        if has_usage {
            let (bar, ratio) = context_bar(total_tokens, bar_width);
            let bar_color = if ratio < 0.5 {
                self.theme.success
            } else if ratio < 0.8 {
                self.theme.warning
            } else {
                self.theme.error
            };
            let bar_style = Style::default()
                .fg(bar_color)
                .bg(self.theme.status_bg);
            write_str(buf, &bar, after_text, area.y, area.right(), bar_style);
        }

        // Right: help hint
        let right = "^K:menu | ^F:files | ^D:diff | ^Q:quit ";
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

    #[test]
    fn test_context_bar_empty() {
        let (bar, ratio) = context_bar(0, 10);
        assert_eq!(bar, "░░░░░░░░░░");
        assert!((ratio - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_context_bar_half() {
        let (bar, ratio) = context_bar(100_000, 10);
        assert_eq!(bar, "█████░░░░░");
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_context_bar_full() {
        let (bar, ratio) = context_bar(200_000, 10);
        assert_eq!(bar, "██████████");
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_context_bar_over_limit_capped() {
        let (bar, ratio) = context_bar(300_000, 10);
        assert_eq!(bar, "██████████");
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }
}
