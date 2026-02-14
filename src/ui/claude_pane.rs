use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;

use crate::claude::conversation::{ContentBlock, Conversation, Message, Role};
use crate::theme::Theme;

/// A widget that renders the conversation as a scrollable chat.
pub struct ClaudePane<'a> {
    conversation: &'a Conversation,
    theme: &'a Theme,
    scroll_offset: usize,
}

impl<'a> ClaudePane<'a> {
    pub fn new(conversation: &'a Conversation, theme: &'a Theme, scroll_offset: usize) -> Self {
        Self {
            conversation,
            theme,
            scroll_offset,
        }
    }
}

impl Widget for ClaudePane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let bg = self.theme.background;

        // Fill background
        let bg_style = Style::default().bg(bg);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(bg_style);
                    cell.set_char(' ');
                }
            }
        }

        // Convert conversation to wrapped lines
        let lines = render_conversation(self.conversation, area.width as usize, self.theme);

        // Apply scroll offset
        let visible_lines: Vec<&StyledLine> = lines
            .iter()
            .skip(self.scroll_offset)
            .take(area.height as usize)
            .collect();

        for (row_idx, line) in visible_lines.iter().enumerate() {
            let y = area.top() + row_idx as u16;
            if y >= area.bottom() {
                break;
            }
            let mut x = area.left();
            for span in &line.spans {
                for ch in span.text.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    if ch_width == 0 {
                        continue;
                    }
                    if x + ch_width as u16 > area.right() {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        cell.set_style(span.style.bg(bg));
                    }
                    // For wide chars (emoji etc), blank the next cell so ratatui doesn't clobber
                    if ch_width == 2 {
                        let next_x = x + 1;
                        if next_x < area.right() {
                            if let Some(cell) = buf.cell_mut((next_x, y)) {
                                cell.set_char(' ');
                                cell.set_style(span.style.bg(bg));
                            }
                        }
                    }
                    x += ch_width as u16;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct StyledSpan {
    text: String,
    style: Style,
}

#[derive(Debug, Clone)]
struct StyledLine {
    spans: Vec<StyledSpan>,
}

impl StyledLine {
    fn empty() -> Self {
        Self { spans: Vec::new() }
    }

    fn plain(text: &str, style: Style) -> Self {
        Self {
            spans: vec![StyledSpan {
                text: text.to_string(),
                style,
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const USER_PREFIX: &str = "  You  ";
const ASSISTANT_PREFIX: &str = " Claude ";

fn user_label_style() -> Style {
    Style::default()
        .fg(Color::Rgb(30, 30, 46))
        .bg(Color::Rgb(137, 180, 250))
        .add_modifier(Modifier::BOLD)
}

fn assistant_label_style() -> Style {
    Style::default()
        .fg(Color::Rgb(30, 30, 46))
        .bg(Color::Rgb(166, 227, 161))
        .add_modifier(Modifier::BOLD)
}

fn user_text_style() -> Style {
    Style::default().fg(Color::Rgb(205, 214, 244))
}

fn separator_style() -> Style {
    Style::default().fg(Color::Rgb(69, 71, 90))
}

// ---------------------------------------------------------------------------
// Conversation → lines
// ---------------------------------------------------------------------------

/// Convert the entire conversation into styled, wrapped lines for rendering.
fn render_conversation(conversation: &Conversation, width: usize, theme: &Theme) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    let content_width = width.saturating_sub(2); // 2-char left padding

    for (i, msg) in conversation.messages.iter().enumerate() {
        if i > 0 {
            // Separator line between messages
            let sep = "─".repeat(width.min(120));
            lines.push(StyledLine::plain(&sep, separator_style()));
        }
        render_message(msg, &mut lines, content_width, theme);
    }

    lines
}

fn render_message(msg: &Message, lines: &mut Vec<StyledLine>, content_width: usize, theme: &Theme) {
    // Role label line
    match msg.role {
        Role::User => {
            lines.push(StyledLine {
                spans: vec![StyledSpan {
                    text: USER_PREFIX.to_string(),
                    style: user_label_style(),
                }],
            });
        }
        Role::Assistant => {
            lines.push(StyledLine {
                spans: vec![StyledSpan {
                    text: ASSISTANT_PREFIX.to_string(),
                    style: assistant_label_style(),
                }],
            });
        }
    }

    let text_style = match msg.role {
        Role::User => user_text_style(),
        Role::Assistant => Style::default().fg(theme.secondary),
    };

    for block in &msg.content {
        match block {
            ContentBlock::Text(text) => {
                // Trim leading blank lines to avoid whitespace gap after role label
                let trimmed = text.trim_start_matches('\n');
                render_text_block(trimmed, text_style, lines, content_width);
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let tool_style = Style::default()
                    .fg(Color::Rgb(249, 226, 175))
                    .add_modifier(Modifier::DIM);
                let summary = if input.len() > 60 {
                    format!("  [{name}] {}...", &input[..57])
                } else {
                    format!("  [{name}] {input}")
                };
                lines.push(StyledLine::plain(&summary, tool_style));
            }
        }
    }
}

/// Render a text block with basic formatting and word wrapping.
fn render_text_block(
    text: &str,
    base_style: Style,
    lines: &mut Vec<StyledLine>,
    content_width: usize,
) {
    let bold_style = base_style.add_modifier(Modifier::BOLD);
    let code_style = Style::default().fg(Color::Rgb(166, 227, 161));
    let code_block_style = Style::default().fg(Color::Rgb(180, 190, 220));
    let header_style = Style::default()
        .fg(Color::Rgb(203, 166, 247))
        .add_modifier(Modifier::BOLD);

    let indent = "  "; // 2-char indent for message body
    let mut in_code_block = false;

    for raw_line in text.lines() {
        // Code block fence
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            let fence_style = Style::default()
                .fg(Color::Rgb(127, 132, 156))
                .add_modifier(Modifier::DIM);
            lines.push(StyledLine::plain(&format!("{indent}{raw_line}"), fence_style));
            continue;
        }

        if in_code_block {
            // Code blocks: preserve exact formatting, just indent
            lines.push(StyledLine::plain(
                &format!("{indent}{raw_line}"),
                code_block_style,
            ));
            continue;
        }

        // Headers
        if raw_line.starts_with('#') {
            lines.push(StyledLine::plain(
                &format!("{indent}{raw_line}"),
                header_style,
            ));
            continue;
        }

        // Empty line
        if raw_line.is_empty() {
            lines.push(StyledLine::empty());
            continue;
        }

        // Normal text with inline formatting — parse into spans then wrap
        let parsed_spans = parse_inline_formatting(raw_line, base_style, bold_style, code_style);
        wrap_spans(&parsed_spans, indent, lines, content_width);
    }
}

/// Parse inline formatting (**bold**, `code`) into styled spans.
fn parse_inline_formatting(
    text: &str,
    text_style: Style,
    bold_style: Style,
    code_style: Style,
) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.starts_with("**") {
            if let Some(end) = remaining[2..].find("**") {
                spans.push(StyledSpan {
                    text: remaining[2..2 + end].to_string(),
                    style: bold_style,
                });
                remaining = &remaining[2 + end + 2..];
                continue;
            }
        }
        if remaining.starts_with('`') {
            if let Some(end) = remaining[1..].find('`') {
                spans.push(StyledSpan {
                    text: remaining[1..1 + end].to_string(),
                    style: code_style,
                });
                remaining = &remaining[1 + end + 1..];
                continue;
            }
        }
        let next_special = remaining
            .find(['*', '`'])
            .unwrap_or(remaining.len());
        if next_special > 0 {
            spans.push(StyledSpan {
                text: remaining[..next_special].to_string(),
                style: text_style,
            });
            remaining = &remaining[next_special..];
        } else {
            spans.push(StyledSpan {
                text: remaining[..1].to_string(),
                style: text_style,
            });
            remaining = &remaining[1..];
        }
    }

    spans
}

/// Word-wrap a list of styled spans to fit within `max_width`, prepending `indent` to each line.
fn wrap_spans(
    spans: &[StyledSpan],
    indent: &str,
    lines: &mut Vec<StyledLine>,
    max_width: usize,
) {
    let indent_width = display_width(indent);
    let available = max_width.saturating_sub(indent_width);
    if available == 0 {
        return;
    }

    let mut current_line_spans: Vec<StyledSpan> = vec![StyledSpan {
        text: indent.to_string(),
        style: Style::default(),
    }];
    let mut current_width: usize = 0;

    for span in spans {
        let mut remaining = span.text.as_str();

        while !remaining.is_empty() {
            let rem_width = display_width(remaining);

            // If the remaining text fits on the current line, add it
            if current_width + rem_width <= available {
                current_line_spans.push(StyledSpan {
                    text: remaining.to_string(),
                    style: span.style,
                });
                current_width += rem_width;
                break;
            }

            // Find a word-break point
            let budget = available.saturating_sub(current_width);
            let (chunk, rest) = split_at_width_word_boundary(remaining, budget);

            if chunk.is_empty() {
                // Current line has no room — flush and start new line
                lines.push(StyledLine {
                    spans: std::mem::take(&mut current_line_spans),
                });
                current_line_spans.push(StyledSpan {
                    text: indent.to_string(),
                    style: Style::default(),
                });
                current_width = 0;

                // If the remaining text doesn't fit even on a fresh line, force-break by chars
                if display_width(rest) == display_width(remaining) && !remaining.is_empty() {
                    let (forced, forced_rest) = split_at_width(remaining, available);
                    if !forced.is_empty() {
                        current_line_spans.push(StyledSpan {
                            text: forced.to_string(),
                            style: span.style,
                        });
                    }
                    remaining = forced_rest;
                    lines.push(StyledLine {
                        spans: std::mem::take(&mut current_line_spans),
                    });
                    current_line_spans.push(StyledSpan {
                        text: indent.to_string(),
                        style: Style::default(),
                    });
                    current_width = 0;
                    continue;
                }
                remaining = rest;
            } else {
                current_line_spans.push(StyledSpan {
                    text: chunk.to_string(),
                    style: span.style,
                });
                lines.push(StyledLine {
                    spans: std::mem::take(&mut current_line_spans),
                });
                current_line_spans.push(StyledSpan {
                    text: indent.to_string(),
                    style: Style::default(),
                });
                current_width = 0;
                remaining = rest.trim_start();
            }
        }
    }

    // Flush remaining line
    if current_line_spans.len() > 1 || current_width > 0 {
        lines.push(StyledLine {
            spans: current_line_spans,
        });
    }
}

/// Calculate display width of a string (accounting for wide chars like emoji).
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| c.width().unwrap_or(0))
        .sum()
}

/// Split a string at approximately `max_width` display columns, preferring word boundaries.
/// Returns (chunk_that_fits, remainder).
fn split_at_width_word_boundary(s: &str, max_width: usize) -> (&str, &str) {
    let mut width = 0;
    let mut last_space_byte = 0;
    let mut byte_pos = 0;

    for (i, ch) in s.char_indices() {
        let ch_w = ch.width().unwrap_or(0);
        if width + ch_w > max_width {
            // Use word boundary if we found a space
            if last_space_byte > 0 {
                return (&s[..last_space_byte], s[last_space_byte..].trim_start());
            }
            return (&s[..byte_pos], &s[byte_pos..]);
        }
        if ch == ' ' {
            last_space_byte = i + 1; // include the space in the chunk
        }
        width += ch_w;
        byte_pos = i + ch.len_utf8();
    }

    (s, "")
}

/// Split a string at exactly `max_width` display columns (force break, no word boundary).
fn split_at_width(s: &str, max_width: usize) -> (&str, &str) {
    let mut width = 0;
    for (i, ch) in s.char_indices() {
        let ch_w = ch.width().unwrap_or(0);
        if width + ch_w > max_width {
            return (&s[..i], &s[i..]);
        }
        width += ch_w;
    }
    (s, "")
}

/// Calculate total number of rendered lines for scroll calculations.
pub fn total_lines(conversation: &Conversation, width: usize, theme: &Theme) -> usize {
    render_conversation(conversation, width, theme).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::conversation::{ContentBlock, Conversation, Message, Role};

    #[test]
    fn test_empty_conversation_renders() {
        let conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 0);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }

    #[test]
    fn test_user_message_has_label() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.push_user_message("Hello".to_string());
        let lines = render_conversation(&conv, 80, &theme);
        assert!(!lines.is_empty());
        let label: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(label.contains("You"));
    }

    #[test]
    fn test_assistant_message_has_label() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text("Hi there".to_string())],
        });
        let lines = render_conversation(&conv, 80, &theme);
        assert!(!lines.is_empty());
        let label: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(label.contains("Claude"));
    }

    #[test]
    fn test_code_block_rendering() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(
                "Here is code:\n```rust\nfn main() {}\n```\nDone.".to_string(),
            )],
        });
        let lines = render_conversation(&conv, 80, &theme);
        // label + code fence + code + code fence + "Done." + blank = at least 6 lines
        assert!(lines.len() >= 6);
    }

    #[test]
    fn test_tool_use_rendering() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "Bash".to_string(),
                input: "{\"command\":\"ls\"}".to_string(),
            }],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("[Bash]"));
    }

    #[test]
    fn test_word_wrapping() {
        let long_text = "This is a very long sentence that should be word wrapped when the terminal width is narrow enough to force it onto multiple lines";
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(long_text.to_string())],
        });
        // Narrow width to force wrapping
        let lines = render_conversation(&conv, 40, &theme);
        // Should produce multiple lines (label + wrapped text + blank)
        assert!(lines.len() > 3, "Expected wrapping, got {} lines", lines.len());
    }

    #[test]
    fn test_display_width() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn test_separator_between_messages() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.push_user_message("Hi".to_string());
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text("Hello!".to_string())],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("─"), "Expected separator line");
    }

    #[test]
    fn test_scroll_offset() {
        let mut conv = Conversation::new();
        for i in 0..30 {
            conv.push_user_message(format!("Message {}", i));
        }
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 10);
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }

    #[test]
    fn test_zero_area() {
        let conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }
}
