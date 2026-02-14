pub mod borders;
pub mod claude_pane;
pub mod input;
pub mod status_bar;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::theme::Theme;
use claude_pane::ClaudePane;
use status_bar::StatusBar;

/// Render the full UI layout.
pub fn render(
    frame: &mut Frame,
    screen: &vt100::Screen,
    theme: &Theme,
) {
    let size = frame.area();

    // Main vertical layout: [claude pane] [status bar]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),   // Claude pane (fills remaining)
            Constraint::Length(1), // Status bar
        ])
        .split(size);

    // Claude pane with themed border
    let claude_block = borders::themed_block(" sexy-claude ", true, theme);
    let claude_inner = claude_block.inner(chunks[0]);
    frame.render_widget(claude_block, chunks[0]);
    frame.render_widget(ClaudePane::new(screen, theme.background), claude_inner);

    // Status bar
    frame.render_widget(StatusBar::new(&theme.name, theme), chunks[1]);
}
