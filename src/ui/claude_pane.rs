use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

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

        // Convert conversation to lines
        let lines = render_conversation(self.conversation, area.width as usize);

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
                    if x >= area.right() {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(ch);
                        cell.set_style(span.style.bg(bg));
                    }
                    x += 1;
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

/// Convert the entire conversation into styled lines for rendering.
fn render_conversation(conversation: &Conversation, _width: usize) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    for (i, msg) in conversation.messages.iter().enumerate() {
        if i > 0 {
            lines.push(StyledLine::empty());
        }
        render_message(msg, &mut lines);
    }

    lines
}

fn render_message(msg: &Message, lines: &mut Vec<StyledLine>) {
    let (prefix, prefix_style) = match msg.role {
        Role::User => (
            "> ",
            Style::default()
                .fg(Color::Rgb(137, 180, 250))
                .add_modifier(Modifier::BOLD),
        ),
        Role::Assistant => (
            "",
            Style::default().fg(Color::Rgb(166, 227, 161)),
        ),
    };

    for block in &msg.content {
        match block {
            ContentBlock::Text(text) => {
                render_text_block(text, prefix, prefix_style, lines);
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let tool_style = Style::default()
                    .fg(Color::Rgb(249, 226, 175))
                    .add_modifier(Modifier::DIM);
                let summary = if input.len() > 60 {
                    format!("[{}] {}...", name, &input[..57])
                } else {
                    format!("[{}] {}", name, input)
                };
                lines.push(StyledLine::plain(&summary, tool_style));
            }
        }
    }
}

/// Render a text block with basic formatting: **bold**, `code`, ```code blocks```, # headers.
fn render_text_block(
    text: &str,
    prefix: &str,
    prefix_style: Style,
    lines: &mut Vec<StyledLine>,
) {
    let text_style = Style::default().fg(Color::Rgb(205, 214, 244));
    let bold_style = text_style.add_modifier(Modifier::BOLD);
    let code_style = Style::default().fg(Color::Rgb(166, 227, 161));
    let code_block_style = Style::default().fg(Color::Rgb(180, 190, 220));
    let header_style = Style::default()
        .fg(Color::Rgb(203, 166, 247))
        .add_modifier(Modifier::BOLD);

    let mut in_code_block = false;

    for (line_idx, raw_line) in text.lines().enumerate() {
        // Code block fence
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            let fence_style = Style::default()
                .fg(Color::Rgb(127, 132, 156))
                .add_modifier(Modifier::DIM);
            lines.push(StyledLine::plain(raw_line, fence_style));
            continue;
        }

        if in_code_block {
            lines.push(StyledLine::plain(raw_line, code_block_style));
            continue;
        }

        // Headers
        if raw_line.starts_with('#') {
            lines.push(StyledLine::plain(raw_line, header_style));
            continue;
        }

        // Normal text with inline formatting
        let effective_prefix = if line_idx == 0 { prefix } else { "" };
        let mut spans = Vec::new();

        if !effective_prefix.is_empty() {
            spans.push(StyledSpan {
                text: effective_prefix.to_string(),
                style: prefix_style,
            });
        }

        // Simple inline parsing: **bold** and `code`
        let mut remaining = raw_line;
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
                .find(|c: char| c == '*' || c == '`')
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

        lines.push(StyledLine { spans });
    }
}

/// Calculate total number of rendered lines for scroll calculations.
pub fn total_lines(conversation: &Conversation, width: usize) -> usize {
    render_conversation(conversation, width).len()
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
    fn test_user_message_has_prefix() {
        let mut conv = Conversation::new();
        conv.push_user_message("Hello".to_string());
        let lines = render_conversation(&conv, 80);
        assert!(!lines.is_empty());
        let first_line = &lines[0];
        let text: String = first_line.spans.iter().map(|s| s.text.as_str()).collect();
        assert!(text.starts_with("> "));
        assert!(text.contains("Hello"));
    }

    #[test]
    fn test_code_block_rendering() {
        let mut conv = Conversation::new();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(
                "Here is code:\n```rust\nfn main() {}\n```\nDone.".to_string(),
            )],
        });
        let lines = render_conversation(&conv, 80);
        assert!(lines.len() >= 5);
    }

    #[test]
    fn test_tool_use_rendering() {
        let mut conv = Conversation::new();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t1".to_string(),
                name: "Bash".to_string(),
                input: "{\"command\":\"ls\"}".to_string(),
            }],
        });
        let lines = render_conversation(&conv, 80);
        let text: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("[Bash]"));
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
