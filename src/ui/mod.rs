pub mod borders;
pub mod claude_pane;
pub mod header;
pub mod input;
pub mod overlay;
pub mod status_bar;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::claude::conversation::Conversation;
use crate::theme::Theme;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT};
use input::{InputEditor, InputWidget};
use overlay::{OverlayState, OverlayWidget};
use status_bar::StatusBar;

/// Render the full UI layout.
pub fn render(
    frame: &mut Frame,
    conversation: &Conversation,
    input: &InputEditor,
    theme: &Theme,
    frame_count: u64,
    scroll_offset: usize,
    is_streaming: bool,
) {
    let size = frame.area();

    let input_height = if input.is_empty() {
        1
    } else {
        3u16.min(input.content().lines().count() as u16 + 1)
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Min(3),
            Constraint::Length(input_height + 2), // +2 for border
            Constraint::Length(1),
        ])
        .split(size);

    // Animated header
    frame.render_widget(Header::new(theme, frame_count), chunks[0]);

    // Claude pane
    let claude_block = borders::themed_block("", true, theme);
    let claude_inner = claude_block.inner(chunks[1]);
    frame.render_widget(claude_block, chunks[1]);
    frame.render_widget(
        ClaudePane::new(conversation, theme, scroll_offset),
        claude_inner,
    );

    // Input area
    let input_title = if is_streaming { " streaming... " } else { "" };
    let input_block = borders::themed_block(input_title, !is_streaming, theme);
    let input_inner = input_block.inner(chunks[2]);
    frame.render_widget(input_block, chunks[2]);
    frame.render_widget(InputWidget::new(input, theme), input_inner);

    // Status bar
    frame.render_widget(StatusBar::new(&theme.name, theme), chunks[3]);
}

/// Render an overlay popup on top of the existing UI.
pub fn render_overlay(frame: &mut Frame, title: &str, state: &OverlayState, theme: &Theme) {
    let widget = OverlayWidget::new(title, state, theme);
    frame.render_widget(widget, frame.area());
}
