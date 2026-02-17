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
use crate::diff::{self, DiffOp};
use crate::git::GitInfo;
use crate::theme::Theme;
use crate::ui::toast::Toast;
use claude_pane::ClaudePane;
use header::{Header, HEADER_HEIGHT, COMPACT_HEADER_HEIGHT};
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
    permission_mode: Option<&str>,
    tools_expanded: bool,
    active_tool: Option<(&str, u64)>,
) {
    let size = frame.area();

    let input_height = if input.is_empty() {
        1
    } else {
        // Allow input to grow up to 10 lines for multi-line content (e.g. paste)
        let line_count = input.content().lines().count() as u16 + 1;
        let max_height = (size.height / 3).max(3).min(10);
        max_height.min(line_count)
    };

    // Collapse header to single line once conversation has messages
    let compact_header = !conversation.messages.is_empty();
    let header_height = if compact_header { COMPACT_HEADER_HEIGHT } else { HEADER_HEIGHT };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(3),
            Constraint::Length(input_height + 2), // +2 for border
            Constraint::Length(1),
        ])
        .split(size);

    // Animated header (compact when conversation has content)
    frame.render_widget(Header::new(theme, frame_count).compact(compact_header), chunks[0]);

    // Claude pane
    let claude_block = borders::themed_block("", true, theme);
    let claude_inner = claude_block.inner(chunks[1]);
    frame.render_widget(claude_block, chunks[1]);
    frame.render_widget(
        ClaudePane::new(conversation, theme, scroll_offset, frame_count)
            .with_tools_expanded(tools_expanded),
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
        StatusBar::new(theme, token_usage.0, token_usage.1, git_info, todo_summary, model_name, permission_mode, active_tool),
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

    // Collect visible lines with their absolute indices for lookahead
    let visible_lines: Vec<(usize, &String)> = lines.iter().skip(scroll).take(visible).enumerate().collect();
    let mut skip_next = false;

    for (vi, &(i, line)) in visible_lines.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let row_y = inner.y + i as u16;

        // Check for adjacent Remove+Add pair for word-level diff
        let is_remove = line.starts_with("- ") && !line.starts_with("--- ");
        let next_is_add = visible_lines.get(vi + 1)
            .map(|&(_, next)| next.starts_with("+ ") && !next.starts_with("+++ "))
            .unwrap_or(false);

        if is_remove && next_is_add {
            let next_line = visible_lines[vi + 1].1;
            let old_text = &line[2..];
            let new_text = &next_line[2..];
            let word_ops = diff::diff_words(old_text, new_text);

            // Render remove line with word-level highlighting
            let mut col = inner.x;
            // Write "- " prefix
            for ch in "- ".chars() {
                if col >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut((col, row_y)) {
                    cell.set_char(ch);
                    cell.set_style(diff_remove_style);
                }
                col += 1;
            }
            for op in &word_ops {
                let (text, style) = match op {
                    DiffOp::Equal(t) => (*t, text_style.add_modifier(Modifier::DIM)),
                    DiffOp::Remove(t) => (*t, diff_remove_style),
                    DiffOp::Add(_) => continue, // skip adds on the remove line
                };
                for ch in text.chars() {
                    if col >= inner.right() { break; }
                    if let Some(cell) = buf.cell_mut((col, row_y)) {
                        cell.set_char(ch);
                        cell.set_style(style);
                    }
                    col += 1;
                }
            }

            // Render add line with word-level highlighting
            let next_row_y = inner.y + (i + 1) as u16;
            if next_row_y < inner.bottom() {
                let mut col = inner.x;
                for ch in "+ ".chars() {
                    if col >= inner.right() { break; }
                    if let Some(cell) = buf.cell_mut((col, next_row_y)) {
                        cell.set_char(ch);
                        cell.set_style(diff_add_style);
                    }
                    col += 1;
                }
                for op in &word_ops {
                    let (text, style) = match op {
                        DiffOp::Equal(t) => (*t, text_style.add_modifier(Modifier::DIM)),
                        DiffOp::Add(t) => (*t, diff_add_style),
                        DiffOp::Remove(_) => continue, // skip removes on the add line
                    };
                    for ch in text.chars() {
                        if col >= inner.right() { break; }
                        if let Some(cell) = buf.cell_mut((col, next_row_y)) {
                            cell.set_char(ch);
                            cell.set_style(style);
                        }
                        col += 1;
                    }
                }
            }
            skip_next = true;
            continue;
        }

        // Standard single-line styling
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

/// Render a history search overlay with a query input and scrollable match list.
pub fn render_history_search(
    frame: &mut Frame,
    query: &str,
    matches: &[String],
    selected: usize,
    theme: &Theme,
) {
    let area = frame.area();

    // Popup size: ~60% width, up to 50% height
    let width = (area.width * 60 / 100).max(30).min(area.width.saturating_sub(4));
    let max_items = 12usize;
    let height = ((max_items as u16) + 4).min(area.height.saturating_sub(2)); // +4 for borders+query
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    let buf = frame.buffer_mut();
    Clear.render(popup, buf);

    let title = format!(" History Search: {} ", if query.is_empty() { "(type to filter)" } else { query });
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .title_bottom(format!(" {} matches | Enter to select | Esc to cancel ", matches.len()))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.surface).fg(theme.foreground));

    let inner = block.inner(popup);
    block.render(popup, buf);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let visible = inner.height as usize;

    // Scroll to keep selected visible
    let scroll = if selected >= visible {
        selected - visible + 1
    } else {
        0
    };

    for (vi, entry) in matches.iter().skip(scroll).take(visible).enumerate() {
        let row_y = inner.y + vi as u16;
        if row_y >= inner.bottom() {
            break;
        }

        let is_selected = vi + scroll == selected;
        let entry_style = if is_selected {
            Style::default()
                .fg(theme.primary)
                .bg(theme.overlay)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground).bg(theme.surface)
        };

        // Fill row background
        let bg_style = if is_selected {
            Style::default().bg(theme.overlay)
        } else {
            Style::default().bg(theme.surface)
        };
        for col in inner.x..inner.right() {
            if let Some(cell) = buf.cell_mut((col, row_y)) {
                cell.set_char(' ');
                cell.set_style(bg_style);
            }
        }

        // Write entry text (truncate multi-line to first line + indicator)
        let marker = if is_selected { " \u{25b8} " } else { "   " };
        let first_line = entry.lines().next().unwrap_or("");
        let display = if entry.contains('\n') {
            format!("{marker}{first_line} ...")
        } else {
            format!("{marker}{first_line}")
        };

        let mut col = inner.x;
        for ch in display.chars() {
            if col >= inner.right() {
                break;
            }
            if let Some(cell) = buf.cell_mut((col, row_y)) {
                cell.set_char(ch);
                cell.set_style(entry_style);
            }
            col += 1;
        }
    }
}

/// Render a text input popup for single-line text entry (e.g. session rename).
pub fn render_text_input(
    frame: &mut Frame,
    prompt: &str,
    value: &str,
    cursor: usize,
    theme: &Theme,
) {
    let area = frame.area();

    // Small centered popup: ~50% width, 5 rows (border + prompt + input + hint + border)
    let width = (area.width * 50 / 100).max(30).min(area.width.saturating_sub(4));
    let height: u16 = 5;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    let buf = frame.buffer_mut();
    Clear.render(popup, buf);

    let block = Block::default()
        .title(format!(" {} ", prompt))
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .title_bottom(" Enter to confirm | Esc to cancel ")
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.surface).fg(theme.foreground));

    let inner = block.inner(popup);
    block.render(popup, buf);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Fill inner background
    let bg_style = Style::default().bg(theme.surface).fg(theme.foreground);
    for row in inner.y..inner.bottom() {
        for col in inner.x..inner.right() {
            if let Some(cell) = buf.cell_mut((col, row)) {
                cell.set_char(' ');
                cell.set_style(bg_style);
            }
        }
    }

    // Render value text on first inner row
    let text_y = inner.y;
    let text_style = Style::default().fg(theme.foreground).bg(theme.surface);
    let cursor_style = Style::default().fg(theme.surface).bg(theme.primary);

    let mut col = inner.x;
    for (i, ch) in value.chars().enumerate() {
        if col >= inner.right() {
            break;
        }
        let style = if i == cursor { cursor_style } else { text_style };
        if let Some(cell) = buf.cell_mut((col, text_y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
        col += 1;
    }

    // Show cursor at end if cursor == value length
    if cursor >= value.len() && col < inner.right() {
        if let Some(cell) = buf.cell_mut((col, text_y)) {
            cell.set_char(' ');
            cell.set_style(cursor_style);
        }
    }
}

/// Render an interactive question overlay for AskUserQuestion tool calls.
pub fn render_user_question(
    frame: &mut Frame,
    question: &str,
    options: &[(&str, &str)],
    cursor: usize,
    selected: &[bool],
    multi_select: bool,
    theme: &Theme,
) {
    let area = frame.area();

    // Calculate popup size
    let max_width = (area.width * 70 / 100).max(40).min(area.width.saturating_sub(4));

    // Height: border(1) + question(1) + blank(1) + options + blank(1) + hint(1) + border(1)
    let content_height = 3 + options.len() as u16 + 1;
    let height = (content_height + 2).min(area.height.saturating_sub(2)); // +2 for borders
    let x = area.x + (area.width.saturating_sub(max_width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, max_width, height);

    let buf = frame.buffer_mut();
    Clear.render(popup, buf);

    let title = if multi_select { " Select Multiple " } else { " Select One " };
    let hint = if multi_select {
        " Space to toggle | Enter to confirm | Esc to dismiss "
    } else {
        " Enter to select | Esc to dismiss "
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .title_bottom(hint)
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(theme.border_focused))
        .style(Style::default().bg(theme.surface).fg(theme.foreground));

    let inner = block.inner(popup);
    block.render(popup, buf);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Fill inner background
    let bg_style = Style::default().bg(theme.surface).fg(theme.foreground);
    for row in inner.y..inner.bottom() {
        for col in inner.x..inner.right() {
            if let Some(cell) = buf.cell_mut((col, row)) {
                cell.set_char(' ');
                cell.set_style(bg_style);
            }
        }
    }

    // Render question text (first line, word-wrapped if needed)
    let question_style = Style::default()
        .fg(theme.foreground)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    let mut col = inner.x;
    let mut row = inner.y;
    for ch in question.chars() {
        if col >= inner.right() {
            col = inner.x;
            row += 1;
        }
        if row >= inner.bottom() {
            break;
        }
        if let Some(cell) = buf.cell_mut((col, row)) {
            cell.set_char(ch);
            cell.set_style(question_style);
        }
        col += 1;
    }

    // Render options starting 2 rows after question start
    let options_start_y = inner.y + 2;
    for (i, (label, description)) in options.iter().enumerate() {
        let opt_y = options_start_y + i as u16;
        if opt_y >= inner.bottom() {
            break;
        }

        let is_highlighted = i == cursor;
        let is_selected = selected.get(i).copied().unwrap_or(false);

        // Choose marker
        let marker = if multi_select {
            if is_selected {
                if is_highlighted { " [x] " } else { " [x] " }
            } else if is_highlighted {
                " [ ] "
            } else {
                " [ ] "
            }
        } else if is_highlighted {
            " > "
        } else {
            "   "
        };

        let label_style = if is_highlighted {
            Style::default()
                .fg(theme.primary)
                .bg(theme.overlay)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground).bg(theme.surface)
        };
        let desc_style = if is_highlighted {
            Style::default().fg(theme.info).bg(theme.overlay)
        } else {
            Style::default().fg(theme.info).bg(theme.surface)
        };

        // Fill row background if highlighted
        if is_highlighted {
            let row_bg = Style::default().bg(theme.overlay);
            for c in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut((c, opt_y)) {
                    cell.set_char(' ');
                    cell.set_style(row_bg);
                }
            }
        }

        // Write marker + label
        let mut c = inner.x;
        for ch in marker.chars() {
            if c >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut((c, opt_y)) {
                cell.set_char(ch);
                cell.set_style(label_style);
            }
            c += 1;
        }
        for ch in label.chars() {
            if c >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut((c, opt_y)) {
                cell.set_char(ch);
                cell.set_style(label_style);
            }
            c += 1;
        }

        // Write description (if room)
        if !description.is_empty() && c + 3 < inner.right() {
            // Separator
            for ch in " - ".chars() {
                if c >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut((c, opt_y)) {
                    cell.set_char(ch);
                    cell.set_style(desc_style);
                }
                c += 1;
            }
            for ch in description.chars() {
                if c >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut((c, opt_y)) {
                    cell.set_char(ch);
                    cell.set_style(desc_style);
                }
                c += 1;
            }
        }
    }
}
