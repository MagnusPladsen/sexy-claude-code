use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Height of the header area in terminal rows.
pub const HEADER_HEIGHT: u16 = 10;

/// ASCII art logo — "SEXY CLAUDE" in Big figlet style (6 rows, ~76 chars wide).
const LOGO: [&str; 6] = [
    r"  _____ ________   ____     __   _____ _              _    _ _____  ______ ",
    r" / ____|  ____\ \ / /\ \   / /  / ____| |        /\  | |  | |  __ \|  ____|",
    r"| (___ | |__   \ V /  \ \_/ /  | |    | |       /  \ | |  | | |  | | |__   ",
    r" \___ \|  __|   > <    \   /   | |    | |      / /\ \| |  | | |  | |  __|  ",
    r" ____) | |____ / . \    | |    | |____| |____ / ____ \ |__| | |__| | |____ ",
    r"|_____/|______/_/ \_\   |_|     \_____|______/_/    \_\____/|_____/|______|",
];

/// Sparkle characters — cycled through for the particle effect.
const SPARKLES: [char; 6] = ['✦', '✧', '⋆', '·', '∘', '⊹'];

/// Animated header widget displaying a big sexy-claude brand with
/// gradient wave, sparkle particles, and shimmer sweep effects.
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
        if area.height == 0 || area.width == 0 {
            return;
        }

        let bg = self.theme.background;
        let frame = self.frame_count;

        // Fill background
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(Style::default().bg(bg));
                }
            }
        }

        // --- Row 0: sparkle particle row ---
        self.render_sparkle_row(area.top(), area, buf, 0);

        // --- Rows 1-6: centered ASCII art logo with gradient wave + shimmer ---
        let logo_start_y = area.top() + 1;
        for (row_idx, line) in LOGO.iter().enumerate() {
            let y = logo_start_y + row_idx as u16;
            if y >= area.bottom() {
                break;
            }
            let char_count: usize = line.chars().count();
            let start_x = area.left() + area.width.saturating_sub(char_count as u16) / 2;

            let wave_phase = frame as f64 * 0.05;
            // Shimmer: a bright band that sweeps across every ~120 frames
            let shimmer_pos = (frame as f64 * 0.3) % (char_count as f64 + 20.0) - 10.0;

            for (i, ch) in line.chars().enumerate() {
                let x = start_x + i as u16;
                if x >= area.right() {
                    break;
                }
                if ch == ' ' {
                    continue;
                }

                // Gradient wave: position offset by char index + sine wave
                let wave = (i as f64 * 0.15 + row_idx as f64 * 0.3).sin() * 0.1;
                let position = (i as f64 / char_count.max(1) as f64) + wave_phase + wave;
                let mut color = gradient_color(self.theme, position);

                // Shimmer: brighten characters near the shimmer band
                let dist = (i as f64 - shimmer_pos).abs();
                if dist < 4.0 {
                    let intensity = 1.0 - (dist / 4.0);
                    color = brighten(color, intensity * 0.6);
                }

                let style = Style::default()
                    .fg(color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD);
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        }

        // --- Row 7: sparkle particle row ---
        if area.top() + 7 < area.bottom() {
            self.render_sparkle_row(area.top() + 7, area, buf, 1);
        }

        // --- Row 8: version text centered ---
        if area.top() + 8 < area.bottom() {
            let version = format!("v{}", env!("CARGO_PKG_VERSION"));
            let ver_len = version.len() as u16;
            let ver_x = area.left() + area.width.saturating_sub(ver_len) / 2;
            let ver_y = area.top() + 8;

            let ver_phase = frame as f64 * 0.02;
            for (i, ch) in version.chars().enumerate() {
                let x = ver_x + i as u16;
                if x >= area.right() {
                    break;
                }
                let position = (i as f64 / ver_len.max(1) as f64) + ver_phase;
                let color = gradient_color(self.theme, position);
                let style = Style::default().fg(color).bg(bg);
                if let Some(cell) = buf.cell_mut((x, ver_y)) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        }

        // --- Row 9: thin decorative line ---
        if area.top() + 9 < area.bottom() {
            let line_y = area.top() + 9;
            let line_phase = frame as f64 * 0.03;
            for x in area.left()..area.right() {
                let i = (x - area.left()) as f64;
                let position = (i / area.width.max(1) as f64) + line_phase;
                let color = gradient_color(self.theme, position);
                // Fade the line color to ~30% intensity for subtlety
                let faded = lerp_color(bg, color, 0.35);
                let style = Style::default().fg(faded).bg(bg);
                if let Some(cell) = buf.cell_mut((x, line_y)) {
                    cell.set_char('─');
                    cell.set_style(style);
                }
            }
        }
    }
}

impl Header<'_> {
    /// Render a row of animated sparkle particles.
    fn render_sparkle_row(&self, y: u16, area: Rect, buf: &mut Buffer, seed: u64) {
        if y >= area.bottom() {
            return;
        }
        let frame = self.frame_count;
        let bg = self.theme.background;

        for x in area.left()..area.right() {
            let hash = pseudo_hash(x as u64, seed, frame / 4);
            // ~8% chance of a sparkle at any position per 4-frame window
            if hash.is_multiple_of(13) {
                let sparkle_idx = (hash / 13) as usize % SPARKLES.len();
                let ch = SPARKLES[sparkle_idx];

                // Fade in/out: sparkle lives for 4 frames, peaks at frame 2
                let life = (frame % 4) as f64 / 3.0;
                let brightness = 1.0 - (life - 0.5).abs() * 2.0;
                let brightness = brightness.clamp(0.2, 1.0);

                let position = (x as f64 / area.width.max(1) as f64) + frame as f64 * 0.04;
                let base_color = gradient_color(self.theme, position);
                let color = lerp_color(bg, base_color, brightness);

                let style = Style::default().fg(color).bg(bg);
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        }
    }
}

/// Simple pseudo-random hash for deterministic sparkle placement.
fn pseudo_hash(x: u64, y: u64, frame: u64) -> u64 {
    let mut v = x.wrapping_mul(2654435761) ^ y.wrapping_mul(2246822519) ^ frame.wrapping_mul(3266489917);
    v ^= v >> 16;
    v = v.wrapping_mul(0x45d9f3b);
    v ^= v >> 16;
    v
}

/// Brighten a color by blending it toward white.
fn brighten(color: Color, amount: f64) -> Color {
    let white = Color::Rgb(255, 255, 255);
    lerp_color(color, white, amount.clamp(0.0, 1.0))
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
    fn test_brighten() {
        let color = Color::Rgb(100, 50, 0);
        let bright = brighten(color, 0.5);
        // Should be halfway between (100,50,0) and (255,255,255)
        assert_eq!(bright, Color::Rgb(178, 153, 128));
    }

    #[test]
    fn test_pseudo_hash_deterministic() {
        let a = pseudo_hash(5, 3, 10);
        let b = pseudo_hash(5, 3, 10);
        assert_eq!(a, b);
    }

    #[test]
    fn test_pseudo_hash_varies() {
        let a = pseudo_hash(0, 0, 0);
        let b = pseudo_hash(1, 0, 0);
        let c = pseudo_hash(0, 1, 0);
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_header_renders_without_panic() {
        let theme = test_theme();
        let header = Header::new(&theme, 42);
        let area = Rect::new(0, 0, 80, HEADER_HEIGHT);
        let mut buf = Buffer::empty(area);
        header.render(area, &mut buf);

        // Logo should be on rows 1-6 — check row 1 has content
        let row: String = (0..80)
            .map(|x| {
                buf.cell((x, 1))
                    .unwrap()
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        assert!(row.contains('_') || row.contains('/') || row.contains('|'));
    }

    #[test]
    fn test_header_narrow_terminal() {
        let theme = test_theme();
        let header = Header::new(&theme, 0);
        let area = Rect::new(0, 0, 20, HEADER_HEIGHT);
        let mut buf = Buffer::empty(area);
        // Should not panic even if terminal is narrower than logo
        header.render(area, &mut buf);
    }

    #[test]
    fn test_header_zero_size() {
        let theme = test_theme();
        let header = Header::new(&theme, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        header.render(area, &mut buf);
    }
}
