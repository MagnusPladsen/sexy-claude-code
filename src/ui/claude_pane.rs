use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::terminal::converter;

/// A widget that renders the vt100 screen content into a ratatui buffer,
/// overriding backgrounds with the theme color.
pub struct ClaudePane<'a> {
    screen: &'a vt100::Screen,
    theme_bg: Color,
}

impl<'a> ClaudePane<'a> {
    pub fn new(screen: &'a vt100::Screen, theme_bg: Color) -> Self {
        Self { screen, theme_bg }
    }
}

impl<'a> Widget for ClaudePane<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        converter::render_screen(self.screen, buf, area, self.theme_bg);
    }
}
