use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Height of the header area in terminal rows.
pub const HEADER_HEIGHT: u16 = 3;

/// Animated header widget displaying the sexy-claude brand with a gradient.
pub struct Header<'a> {
    theme: &'a Theme,
    frame_count: u64,
}

impl<'a> Header<'a> {
    pub fn new(theme: &'a Theme, frame_count: u64) -> Self {
        Self { theme, frame_count }
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(Style::default().bg(self.theme.background));
                }
            }
        }

        let text = format!("~ sexy-claude ~  v{}", env!("CARGO_PKG_VERSION"));
        let text_len = text.len() as u16;

        // Center on the middle row of the 3-row header
        let mid_y = area.top() + HEADER_HEIGHT / 2;
        let start_x = area.left() + area.width.saturating_sub(text_len) / 2;

        let phase = self.frame_count as f64 * 0.04;

        for (i, ch) in text.chars().enumerate() {
            let x = start_x + i as u16;
            if x >= area.right() {
                break;
            }
            let position = (i as f64 / text_len.max(1) as f64) + phase;
            let color = gradient_color(self.theme, position);
            let style = Style::default()
                .fg(color)
                .bg(self.theme.background)
                .add_modifier(Modifier::BOLD);
            if let Some(cell) = buf.cell_mut((x, mid_y)) {
                cell.set_char(ch);
                cell.set_style(style);
            }
        }
    }
}

/// Linearly interpolate between two RGB colors.
pub fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let r = (r1 as f64 + (r2 as f64 - r1 as f64) * t).round() as u8;
            let g = (g1 as f64 + (g2 as f64 - g1 as f64) * t).round() as u8;
            let b = (b1 as f64 + (b2 as f64 - b1 as f64) * t).round() as u8;
            Color::Rgb(r, g, b)
        }
        _ => a,
    }
}

/// Sample the 3-color gradient loop: primary -> secondary -> accent -> primary.
/// `position` is an unbounded f64; the fractional part determines the color.
pub fn gradient_color(theme: &Theme, position: f64) -> Color {
    let t = position.rem_euclid(1.0) * 3.0;
    let segment = t as usize;
    let frac = t - segment as f64;

    match segment {
        0 => lerp_color(theme.primary, theme.secondary, frac),
        1 => lerp_color(theme.secondary, theme.accent, frac),
        _ => lerp_color(theme.accent, theme.primary, frac),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn test_theme() -> Theme {
        Theme::default_theme()
    }

    #[test]
    fn test_lerp_color_endpoints() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(255, 255, 255);
        assert_eq!(lerp_color(a, b, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(lerp_color(a, b, 1.0), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn test_lerp_color_midpoint() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(200, 100, 50);
        let mid = lerp_color(a, b, 0.5);
        assert_eq!(mid, Color::Rgb(100, 50, 25));
    }

    #[test]
    fn test_lerp_color_clamps() {
        let a = Color::Rgb(100, 100, 100);
        let b = Color::Rgb(200, 200, 200);
        assert_eq!(lerp_color(a, b, -0.5), Color::Rgb(100, 100, 100));
        assert_eq!(lerp_color(a, b, 1.5), Color::Rgb(200, 200, 200));
    }

    #[test]
    fn test_gradient_color_at_zero() {
        let theme = test_theme();
        let color = gradient_color(&theme, 0.0);
        assert_eq!(color, theme.primary);
    }

    #[test]
    fn test_gradient_color_wraps() {
        let theme = test_theme();
        let a = gradient_color(&theme, 0.0);
        let b = gradient_color(&theme, 1.0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_header_renders_without_panic() {
        let theme = test_theme();
        let header = Header::new(&theme, 42);
        let area = Rect::new(0, 0, 60, HEADER_HEIGHT);
        let mut buf = Buffer::empty(area);
        header.render(area, &mut buf);

        // Check that the middle row has non-empty content
        let mid_y = HEADER_HEIGHT / 2;
        let row: String = (0..60).map(|x| buf.cell((x, mid_y)).unwrap().symbol().chars().next().unwrap_or(' ')).collect();
        assert!(row.contains("sexy-claude"));
    }
}
