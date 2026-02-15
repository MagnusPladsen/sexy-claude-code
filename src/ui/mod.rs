pub mod borders;
pub mod claude_pane;
pub mod header;
pub mod input;
pub mod markdown;
pub mod overlay;
pub mod status_bar;
pub mod toast;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders, Clear, Widget};
use ratatui::Frame;

use crate::app::CompletionState;
use crate::claude::conversation::Conversation;
use crate::git::GitInfo;
use crate::theme::Theme;
use crate::ui::toast::Toast;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT};
use input::{InputEditor, InputWidget};
use overlay::{OverlayState, OverlayWidget};
use status_bar::StatusBar;
use toast::ToastWidget;

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
    toast: Option<&Toast>,
    token_usage: (u64, u64),
    git_info: &GitInfo,
    todo_summary: Option<&str>,
    model_name: Option<&str>,
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
        ClaudePane::new(conversation, theme, scroll_offset, frame_count),
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
    frame.render_widget(
        StatusBar::new(theme, token_usage.0, token_usage.1, git_info, todo_summary, model_name),
        chunks[3],
    );

    // Toast notification (floats above status bar)
    if let Some(t) = toast {
        frame.render_widget(ToastWidget::new(t, theme), size);
    }
}

/// Render the slash command completion popup just above the input area.
fn render_completion_popup(buf: &mut Buffer, state: &CompletionState, input_area: Rect, theme: &Theme) {
    if state.matches.is_empty() {
        return;
    }

    let max_visible = 8usize.min(state.matches.len());
    let popup_height = max_visible as u16 + 2; // +2 for borders

    // Auto-fit width based on longest visible item, capped at 70% terminal width
    let max_width = (input_area.width as f32 * 0.7) as u16;
    let content_width = state
        .matches
        .iter()
        .map(|item| {
            let name_len = item.name.len() + 5; // " ▸ /" + name
            if item.description.is_empty() {
                name_len
            } else {
                name_len + 2 + item.description.len() // "  " + description
            }
        })
        .max()
        .unwrap_or(20) as u16;
    let popup_width = (content_width + 4).max(20).min(max_width); // +4 for borders + padding

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

    for (vi, item) in state.matches.iter().skip(scroll).take(max_visible).enumerate() {
        let y = inner.y + vi as u16;
        if y >= inner.bottom() {
            break;
        }

        let is_selected = vi + scroll == state.selected;
        let name_style = if is_selected {
            Style::default()
                .fg(theme.primary)
                .bg(theme.overlay)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground).bg(theme.surface)
        };
        let desc_style = if is_selected {
            Style::default()
                .fg(theme.info)
                .bg(theme.overlay)
        } else {
            Style::default()
                .fg(theme.info)
                .bg(theme.surface)
        };

        // Fill row background
        let bg_style = if is_selected {
            Style::default().bg(theme.overlay)
        } else {
            Style::default().bg(theme.surface)
        };
        for x in inner.x..inner.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_style(bg_style);
            }
        }

        // Write the command name with / prefix
        let marker = if is_selected { " \u{25b8} " } else { "   " };
        let name_text = format!("{marker}/{}", item.name);
        let mut col = inner.x;
        for ch in name_text.chars() {
            if col >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((col, y)) {
                cell.set_char(ch);
                cell.set_style(name_style);
            }
            col += 1;
        }

        // Write description (dim) if available
        if !item.description.is_empty() && col + 2 < inner.right() {
            // Add separator
            for _ in 0..2 {
                if col >= inner.right() {
                    break;
                }
                col += 1;
            }
            for ch in item.description.chars() {
                if col >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut((col, y)) {
                    cell.set_char(ch);
                    cell.set_style(desc_style);
                }
                col += 1;
            }
        }
    }
}

/// Render an overlay popup on top of the existing UI.
pub fn render_overlay(frame: &mut Frame, title: &str, state: &OverlayState, theme: &Theme) {
    let widget = OverlayWidget::new(title, state, theme);
    frame.render_widget(widget, frame.area());
}

/// Render a scrollable text viewer overlay on top of the UI.
pub fn render_text_viewer(
    frame: &mut Frame,
    title: &str,
    lines: &[String],
    scroll: usize,
    theme: &Theme,
) {
    let area = frame.area();

    // Calculate popup size (~80% of screen)
    let width = (area.width * 80 / 100).max(40).min(area.width.saturating_sub(4));
    let height = (area.height * 80 / 100).max(10).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    let buf = frame.buffer_mut();

    // Clear area
    Clear.render(popup, buf);

    // Draw border with title and scroll hint
    let scroll_hint = format!(" {}/{} | Esc to close ", scroll + 1, lines.len().max(1));
    let block = Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .title_bottom(scroll_hint)
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.surface).fg(theme.foreground));

    let inner = block.inner(popup);
    block.render(popup, buf);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Clamp scroll
    let max_scroll = lines.len().saturating_sub(inner.height as usize);
    let scroll = scroll.min(max_scroll);

    // Render lines
    let visible = inner.height as usize;
    let text_style = Style::default().fg(theme.foreground).bg(theme.surface);
    let heading_style = Style::default()
        .fg(theme.primary)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    let code_style = Style::default().fg(theme.accent).bg(theme.surface);
    let diff_add_style = Style::default()
        .fg(ratatui::style::Color::Rgb(100, 255, 100))
        .bg(theme.surface);
    let diff_remove_style = Style::default()
        .fg(ratatui::style::Color::Rgb(255, 100, 100))
        .bg(theme.surface);
    let diff_header_style = Style::default()
        .fg(theme.info)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);

    for (i, line) in lines.iter().skip(scroll).take(visible).enumerate() {
        let row_y = inner.y + i as u16;

        // Context-aware styling: diff lines, markdown headings, code blocks
        let style = if line.starts_with("+ ") || line.starts_with("+++ ") {
            diff_add_style
        } else if line.starts_with("- ") || line.starts_with("--- ") {
            diff_remove_style
        } else if line.starts_with("@@ ") {
            diff_header_style
        } else if line.starts_with('#') {
            heading_style
        } else if line.starts_with("```") || line.starts_with('\t') {
            code_style
        } else {
            text_style
        };

        for (j, ch) in line.chars().enumerate() {
            let col_x = inner.x + j as u16;
            if col_x >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((col_x, row_y)) {
                cell.set_char(ch);
                cell.set_style(style);
            }
        }
    }
}
