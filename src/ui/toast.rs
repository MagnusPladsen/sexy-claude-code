use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Clear, Widget};
use std::time::Instant;

use crate::theme::Theme;

/// Duration the toast is visible (total).
const TOAST_DURATION_MS: u128 = 2000;
/// Duration of the fade-out at the end.
const FADE_DURATION_MS: u128 = 500;

/// A brief, auto-dismissing notification.
pub struct Toast {
    pub message: String,
    pub created_at: Instant,
}

impl Toast {
    pub fn new(message: String) -> Self {
        Self {
            message,
            created_at: Instant::now(),
        }
    }

    /// Returns true if the toast has expired and should be removed.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_millis() >= TOAST_DURATION_MS
    }

    /// Returns 0.0 (fully faded) to 1.0 (fully visible).
    fn opacity(&self) -> f32 {
        let age = self.created_at.elapsed().as_millis();
        if age >= TOAST_DURATION_MS {
            return 0.0;
        }
        let fade_start = TOAST_DURATION_MS - FADE_DURATION_MS;
        if age <= fade_start {
            1.0
        } else {
            let fade_progress = (age - fade_start) as f32 / FADE_DURATION_MS as f32;
            1.0 - fade_progress
        }
    }
}

/// Widget that renders a toast notification floating above the status bar.
pub struct ToastWidget<'a> {
    toast: &'a Toast,
    theme: &'a Theme,
}

impl<'a> ToastWidget<'a> {
    pub fn new(toast: &'a Toast, theme: &'a Theme) -> Self {
        Self { toast, theme }
    }

    /// Interpolate between two RGB colors. `t` ranges from 0.0 (= `from`) to 1.0 (= `to`).
    fn lerp_color(
        from: ratatui::style::Color,
        to: ratatui::style::Color,
        t: f32,
    ) -> ratatui::style::Color {
        use ratatui::style::Color;
        match (from, to) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let r = (r1 as f32 + (r2 as f32 - r1 as f32) * t) as u8;
                let g = (g1 as f32 + (g2 as f32 - g1 as f32) * t) as u8;
                let b = (b1 as f32 + (b2 as f32 - b1 as f32) * t) as u8;
                Color::Rgb(r, g, b)
            }
            _ => from,
        }
    }
}

impl<'a> Widget for ToastWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let opacity = self.toast.opacity();
        if opacity <= 0.0 {
            return;
        }

        // Toast dimensions: pad the message with some margin
        let text = &self.toast.message;
        let content_width = text.len() as u16 + 2; // 1 padding each side
        let popup_width = content_width + 2; // +2 for border
        let popup_height: u16 = 3; // border + content line + border

        // Position: above status bar (last row), right-aligned
        let popup_x = area.right().saturating_sub(popup_width + 1);
        let popup_y = area.bottom().saturating_sub(popup_height + 1); // above status bar
        let popup = Rect::new(popup_x, popup_y, popup_width, popup_height);

        if popup.width == 0 || popup.height == 0 {
            return;
        }

        // Fade colors toward background
        let fade = 1.0 - opacity;
        let fg = Self::lerp_color(self.theme.foreground, self.theme.surface, fade);
        let border_color = Self::lerp_color(self.theme.border_focused, self.theme.surface, fade);
        let bg = self.theme.surface;

        // Clear area behind popup
        Clear.render(popup, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(border_color).bg(bg))
            .style(Style::default().bg(bg));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Render message text
        let style = Style::default().fg(fg).bg(bg);
        let y = inner.y;
        // Prefix with checkmark
        let display = format!(" {text}");
        for (i, ch) in display.chars().enumerate() {
            let x = inner.x + i as u16;
            if x >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(ch);
                cell.set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_new_is_not_expired() {
        let toast = Toast::new("test".to_string());
        assert!(!toast.is_expired());
    }

    #[test]
    fn test_toast_opacity_starts_at_one() {
        let toast = Toast::new("test".to_string());
        let opacity = toast.opacity();
        assert!((opacity - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_lerp_color_endpoints() {
        use ratatui::style::Color;
        let from = Color::Rgb(0, 0, 0);
        let to = Color::Rgb(255, 255, 255);
        assert_eq!(ToastWidget::lerp_color(from, to, 0.0), from);
        assert_eq!(ToastWidget::lerp_color(from, to, 1.0), to);
    }

    #[test]
    fn test_lerp_color_midpoint() {
        use ratatui::style::Color;
        let from = Color::Rgb(0, 0, 0);
        let to = Color::Rgb(200, 100, 50);
        let mid = ToastWidget::lerp_color(from, to, 0.5);
        assert_eq!(mid, Color::Rgb(100, 50, 25));
    }
}
