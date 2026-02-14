pub mod borders;
pub mod claude_pane;
pub mod header;
pub mod input;
pub mod overlay;
pub mod status_bar;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Clear, Widget};
use ratatui::Frame;

use crate::app::CompletionState;
use crate::claude::conversation::Conversation;
use crate::theme::Theme;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT};
use input::{InputEditor, InputWidget};
use overlay::{OverlayState, OverlayWidget};
use status_bar::StatusBar;

/// Render the full UI layout.
#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    conversation: &Conversation,
    input: &InputEditor,
    theme: &Theme,
    frame_count: u64,
    scroll_offset: usize,
    is_streaming: bool,
    completion: Option<&CompletionState>,
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

    // Completion popup (rendered above input area)
    if let Some(state) = completion {
        render_completion_popup(frame.buffer_mut(), state, chunks[2], theme);
    }

    // Status bar
    frame.render_widget(StatusBar::new(&theme.name, theme), chunks[3]);
}

/// Render the slash command completion popup just above the input area.
fn render_completion_popup(buf: &mut Buffer, state: &CompletionState, input_area: Rect, theme: &Theme) {
    if state.matches.is_empty() {
        return;
    }

    let max_visible = 8usize.min(state.matches.len());
    let popup_height = max_visible as u16 + 2; // +2 for borders
    let popup_width = 40u16.min(input_area.width);

    // Position popup just above the input area
    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_x = input_area.x + 1;
    let popup = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear area behind popup
    Clear.render(popup, buf);

    // Draw border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.surface).fg(theme.foreground));

    let inner = block.inner(popup);
    block.render(popup, buf);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Scroll to keep selected visible
    let scroll = if state.selected >= max_visible {
        state.selected - max_visible + 1
    } else {
        0
    };

    for (vi, cmd) in state.matches.iter().skip(scroll).take(max_visible).enumerate() {
        let y = inner.y + vi as u16;
        if y >= inner.bottom() {
            break;
        }

        let is_selected = vi + scroll == state.selected;
        let style = if is_selected {
            Style::default()
                .fg(theme.primary)
                .bg(theme.overlay)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground).bg(theme.surface)
        };

        // Fill row background
        for x in inner.x..inner.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_style(style);
            }
        }

        // Write the command with / prefix
        let marker = if is_selected { " ▸ " } else { "   " };
        let text = format!("{marker}/{cmd}");
        for (i, ch) in text.chars().enumerate() {
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

/// Render an overlay popup on top of the existing UI.
pub fn render_overlay(frame: &mut Frame, title: &str, state: &OverlayState, theme: &Theme) {
    let widget = OverlayWidget::new(title, state, theme);
    frame.render_widget(widget, frame.area());
}
