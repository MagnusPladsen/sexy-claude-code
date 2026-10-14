#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Render the vt100 screen into a ratatui Buffer within the given area.
/// `theme_bg` replaces all default/terminal backgrounds so the wrapper's
/// theme dominates instead of the child process's own background.
pub fn render_screen(screen: &vt100::Screen, buf: &mut Buffer, area: Rect, theme_bg: Color) {
    let rows = area.height.min(screen.size().0);
    let cols = area.width.min(screen.size().1);

    for row in 0..rows {
        for col in 0..cols {
            let cell = screen.cell(row, col);
            let Some(cell) = cell else { continue };

            let x = area.x + col;
            let y = area.y + row;
            if x >= area.right() || y >= area.bottom() {
                continue;
            }

            let contents = cell.contents();
            // Skip wide-char continuation cells
            if contents.is_empty() && col > 0 {
                continue;
            }

            let fg = convert_fg(cell.fgcolor(), theme_bg);
            let bg = convert_bg(cell.bgcolor(), theme_bg);
            let mut modifiers = Modifier::empty();
            if cell.bold() {
                modifiers |= Modifier::BOLD;
            }
            if cell.italic() {
                modifiers |= Modifier::ITALIC;
            }
            if cell.underline() {
                modifiers |= Modifier::UNDERLINED;
            }
            if cell.inverse() {
                modifiers |= Modifier::REVERSED;
            }

            let style = Style::default().fg(fg).bg(bg).add_modifier(modifiers);

            let buf_cell = &mut buf[(x, y)];
            if contents.is_empty() {
                buf_cell.set_symbol(" ");
            } else {
                buf_cell.set_symbol(&contents);
            }
            buf_cell.set_style(style);
        }
    }
}

/// Convert foreground color. Default fg stays as Reset so terminal default applies.
fn convert_fg(color: vt100::Color, _theme_bg: Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(idx) => Color::Indexed(idx),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Convert background color. Default and near-black/dark-gray backgrounds
/// are replaced with the theme background so the wrapper's theme dominates.
fn convert_bg(color: vt100::Color, theme_bg: Color) -> Color {
    match color {
        vt100::Color::Default => theme_bg,
        vt100::Color::Idx(idx) => {
            // Indexed colors 0 (black) and 232-237 (dark grays) → theme bg
            if idx == 0 || (232..=237).contains(&idx) {
                theme_bg
            } else {
                Color::Indexed(idx)
            }
        }
        vt100::Color::Rgb(r, g, b) => {
            // Replace dark backgrounds (luminance < 30) with theme bg
            // This catches Claude Code's dark gray chrome colors
            if is_dark_bg(r, g, b) {
                theme_bg
            } else {
                Color::Rgb(r, g, b)
            }
        }
    }
}

/// Returns true if an RGB color is dark enough to be considered a "background"
/// color that should be replaced with the theme background.
fn is_dark_bg(r: u8, g: u8, b: u8) -> bool {
    // Perceived luminance (approximate)
    let lum = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
    lum < 35
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BG: Color = Color::Rgb(30, 30, 46); // Catppuccin Mocha base

    #[test]
    fn test_convert_bg_default_uses_theme() {
        assert_eq!(convert_bg(vt100::Color::Default, TEST_BG), TEST_BG);
    }

    #[test]
    fn test_convert_bg_dark_rgb_uses_theme() {
        // Very dark gray → theme bg
        assert_eq!(convert_bg(vt100::Color::Rgb(20, 20, 20), TEST_BG), TEST_BG);
    }

    #[test]
    fn test_convert_bg_bright_rgb_preserved() {
        // Bright color stays
        assert_eq!(
            convert_bg(vt100::Color::Rgb(200, 100, 50), TEST_BG),
            Color::Rgb(200, 100, 50)
        );
    }

    #[test]
    fn test_convert_bg_black_indexed_uses_theme() {
        assert_eq!(convert_bg(vt100::Color::Idx(0), TEST_BG), TEST_BG);
    }

    #[test]
    fn test_convert_bg_colored_indexed_preserved() {
        assert_eq!(convert_bg(vt100::Color::Idx(1), TEST_BG), Color::Indexed(1));
    }

    #[test]
    fn test_convert_fg_default() {
        assert_eq!(convert_fg(vt100::Color::Default, TEST_BG), Color::Reset);
    }

    #[test]
    fn test_convert_fg_indexed() {
        assert_eq!(convert_fg(vt100::Color::Idx(1), TEST_BG), Color::Indexed(1));
    }

    #[test]
    fn test_convert_fg_rgb() {
        assert_eq!(
            convert_fg(vt100::Color::Rgb(255, 128, 0), TEST_BG),
            Color::Rgb(255, 128, 0)
        );
    }

    #[test]
    fn test_render_screen_basic() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"Hello, world!");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_screen(parser.screen(), &mut buf, area, TEST_BG);

        assert_eq!(buf[(0, 0)].symbol(), "H");
        assert_eq!(buf[(1, 0)].symbol(), "e");
        assert_eq!(buf[(12, 0)].symbol(), "!");
    }

    #[test]
    fn test_render_screen_with_style() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[1;31mBold Red\x1b[0m");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_screen(parser.screen(), &mut buf, area, TEST_BG);

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "B");
        assert!(cell.style().add_modifier.contains(Modifier::BOLD));
        assert_eq!(cell.style().fg.unwrap(), Color::Indexed(1));
    }

    #[test]
    fn test_render_screen_clipped() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"Hello");

        let area = Rect::new(0, 0, 3, 1);
        let mut buf = Buffer::empty(area);
        render_screen(parser.screen(), &mut buf, area, TEST_BG);

        assert_eq!(buf[(0, 0)].symbol(), "H");
        assert_eq!(buf[(1, 0)].symbol(), "e");
        assert_eq!(buf[(2, 0)].symbol(), "l");
    }

    #[test]
    fn test_render_screen_bg_override() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"A");

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_screen(parser.screen(), &mut buf, area, TEST_BG);

        // Default bg should be replaced with theme bg
        let cell = &buf[(0, 0)];
        assert_eq!(cell.style().bg.unwrap(), TEST_BG);
    }

    #[test]
    fn test_is_dark_bg() {
        assert!(is_dark_bg(0, 0, 0)); // pure black
        assert!(is_dark_bg(20, 20, 30)); // dark gray/blue
        assert!(is_dark_bg(30, 30, 46)); // catppuccin base
        assert!(!is_dark_bg(100, 100, 100)); // mid gray
        assert!(!is_dark_bg(255, 255, 255)); // white
    }
}
