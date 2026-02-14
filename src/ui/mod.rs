pub mod borders;
pub mod claude_pane;
pub mod header;
pub mod overlay;
pub mod input;
pub mod status_bar;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::theme::Theme;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT};
use status_bar::StatusBar;

/// Render the full UI layout.
pub fn render(
    frame: &mut Frame,
    screen: &vt100::Screen,
    theme: &Theme,
    frame_count: u64,
) {
    let size = frame.area();

    // Main vertical layout: [header] [claude pane] [status bar]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT), // Animated header
            Constraint::Min(3),               // Claude pane (fills remaining)
            Constraint::Length(1),             // Status bar
        ])
        .split(size);

    // Animated header
    frame.render_widget(Header::new(theme, frame_count), chunks[0]);

    // Claude pane with themed border (no title — header handles branding)
    let claude_block = borders::themed_block("", true, theme);
    let claude_inner = claude_block.inner(chunks[1]);
    frame.render_widget(claude_block, chunks[1]);
    frame.render_widget(ClaudePane::new(screen, theme.background), claude_inner);

    // Status bar
    frame.render_widget(StatusBar::new(&theme.name, theme), chunks[2]);
}
