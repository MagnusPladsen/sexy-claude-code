use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;

use crate::claude::conversation::{ContentBlock, Conversation, Message, Role};
use crate::theme::Theme;
use crate::ui::markdown;

/// Spinner frames for animated progress indicator.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// A widget that renders the conversation as a scrollable chat.
pub struct ClaudePane<'a> {
    conversation: &'a Conversation,
    theme: &'a Theme,
    scroll_offset: usize,
    frame_count: u64,
}

impl<'a> ClaudePane<'a> {
    pub fn new(
        conversation: &'a Conversation,
        theme: &'a Theme,
        scroll_offset: usize,
        frame_count: u64,
    ) -> Self {
        Self {
            conversation,
            theme,
            scroll_offset,
            frame_count,
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
        let mut lines = render_conversation(self.conversation, area.width as usize, self.theme);

        // Show spinner when waiting for tool execution
        if self.conversation.is_awaiting_tool_result() || self.conversation.is_streaming() {
            let spinner_char =
                SPINNER_FRAMES[(self.frame_count as usize / 2) % SPINNER_FRAMES.len()];
            let label = if self.conversation.is_awaiting_tool_result() {
                "Running..."
            } else {
                "Thinking..."
            };
            lines.push(StyledLine {
                spans: vec![StyledSpan {
                    text: format!("  {spinner_char} {label}"),
                    style: Style::default()
                        .fg(self.theme.info)
                        .add_modifier(Modifier::DIM),
                }],
            });
        }

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
pub struct StyledSpan {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

impl StyledLine {
    pub fn empty() -> Self {
        Self { spans: Vec::new() }
    }

    pub fn plain(text: &str, style: Style) -> Self {
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

    let indent = "  ";

    // Build a lookup from tool_use_id → ToolResult for inline rendering
    let tool_results: std::collections::HashMap<&str, &ContentBlock> = msg
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => {
                Some((tool_use_id.as_str(), block))
            }
            _ => None,
        })
        .collect();

    for block in &msg.content {
        match block {
            ContentBlock::Text(text) => {
                // Trim leading blank lines to avoid whitespace gap after role label
                let trimmed = text.trim_start_matches('\n');

                match msg.role {
                    Role::Assistant => {
                        // Use full markdown rendering for assistant messages
                        let md_lines = markdown::render_markdown(trimmed, theme);
                        for md_line in &md_lines {
                            if md_line.spans.is_empty() {
                                lines.push(StyledLine::empty());
                            } else {
                                // Word-wrap each markdown line with indent
                                wrap_spans(&md_line.spans, indent, lines, content_width);
                            }
                        }
                    }
                    Role::User => {
                        // User messages: plain text with wrapping
                        let style = user_text_style();
                        for raw_line in trimmed.lines() {
                            if raw_line.is_empty() {
                                lines.push(StyledLine::empty());
                            } else {
                                let spans = vec![StyledSpan {
                                    text: raw_line.to_string(),
                                    style,
                                }];
                                wrap_spans(&spans, indent, lines, content_width);
                            }
                        }
                    }
                }
            }
            ContentBlock::ToolUse { id, name, input } => {
                // Check if the matching result is an error so we can mark the header
                let result_is_error = matches!(
                    tool_results.get(id.as_str()),
                    Some(ContentBlock::ToolResult { is_error: true, .. })
                );
                render_tool_use(name, input, result_is_error, lines, theme);
                // Render matching tool result inline after the tool use
                if let Some(ContentBlock::ToolResult {
                    content,
                    is_error,
                    collapsed,
                    ..
                }) = tool_results.get(id.as_str())
                {
                    render_tool_result(content, *is_error, *collapsed, lines, theme);
                }
            }
            ContentBlock::ToolResult { .. } => {
                // Rendered inline after the matching ToolUse above
            }
            ContentBlock::Thinking(_text) => {
                // Thinking blocks will be rendered in #18.
                // For now, skip — they're parsed but not yet displayed.
            }
        }
    }
}

/// Render a tool use block with the tool name in accent color and a parsed primary argument.
/// If `is_error` is true, a failure indicator is appended to the header line.
fn render_tool_use(
    name: &str,
    input: &str,
    is_error: bool,
    lines: &mut Vec<StyledLine>,
    theme: &Theme,
) {
    let name_style = if is_error {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    };
    let arg_style = Style::default()
        .fg(theme.foreground)
        .add_modifier(Modifier::DIM);

    // Extract the primary argument from the tool's JSON input
    let primary_arg = extract_primary_arg(name, input);
    let display = primary_arg.as_deref().unwrap_or("");

    // Truncate long arguments
    let truncated = if display.len() > 60 {
        format!("{}...", &display[..57])
    } else {
        display.to_string()
    };

    let mut spans = vec![StyledSpan {
        text: format!("  > {name}"),
        style: name_style,
    }];
    if !truncated.is_empty() {
        spans.push(StyledSpan {
            text: format!(": {truncated}"),
            style: arg_style,
        });
    }
    if is_error {
        spans.push(StyledSpan {
            text: " ✗".to_string(),
            style: Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        });
    }
    lines.push(StyledLine { spans });
}

/// Maximum visible lines before collapsing tool result output.
const TOOL_RESULT_COLLAPSE_PREVIEW: usize = 20;

/// Render a tool result block inline below its tool use.
fn render_tool_result(
    content: &str,
    is_error: bool,
    collapsed: bool,
    lines: &mut Vec<StyledLine>,
    theme: &Theme,
) {
    if content.is_empty() {
        return;
    }

    let content_style = if is_error {
        Style::default().fg(theme.error)
    } else {
        Style::default()
            .fg(theme.foreground)
            .add_modifier(Modifier::DIM)
    };

    // Show error label before content
    if is_error {
        lines.push(StyledLine {
            spans: vec![StyledSpan {
                text: "    ✗ Error".to_string(),
                style: Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            }],
        });
    }

    let total_lines = content.lines().count();

    if collapsed {
        // Show first N lines with a "more lines" indicator
        for line_text in content.lines().take(TOOL_RESULT_COLLAPSE_PREVIEW) {
            lines.push(StyledLine::plain(
                &format!("    {line_text}"),
                content_style,
            ));
        }
        if total_lines > TOOL_RESULT_COLLAPSE_PREVIEW {
            let dim_style = Style::default()
                .fg(theme.info)
                .add_modifier(Modifier::DIM);
            lines.push(StyledLine::plain(
                &format!(
                    "    ... {} more lines",
                    total_lines - TOOL_RESULT_COLLAPSE_PREVIEW
                ),
                dim_style,
            ));
        }
    } else {
        for line_text in content.lines() {
            lines.push(StyledLine::plain(
                &format!("    {line_text}"),
                content_style,
            ));
        }
    }
}

/// Extract the most relevant argument from a tool's JSON input.
fn extract_primary_arg(tool_name: &str, input: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input).ok()?;
    let obj = value.as_object()?;

    // Try tool-specific keys first, then common ones
    let key = match tool_name {
        "Bash" => "command",
        "Read" | "Write" | "Edit" | "Glob" => "file_path",
        "Grep" => "pattern",
        _ => {
            // Try common key names
            for k in ["file_path", "command", "path", "pattern", "query", "url"] {
                if let Some(v) = obj.get(k) {
                    return Some(v.as_str().unwrap_or(&v.to_string()).to_string());
                }
            }
            return None;
        }
    };

    obj.get(key)
        .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
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
        let pane = ClaudePane::new(&conv, &theme, 0, 0);
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
        // label + paragraph + fence + code + fence + "Done." = at least 5 lines
        assert!(lines.len() >= 5, "Got {} lines", lines.len());
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
        assert!(all_text.contains("Bash"), "Expected tool name 'Bash' in output");
        assert!(all_text.contains("ls"), "Expected command 'ls' in output");
    }

    #[test]
    fn test_tool_use_read_rendering() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t2".to_string(),
                name: "Read".to_string(),
                input: "{\"file_path\":\"src/main.rs\"}".to_string(),
            }],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("Read"));
        assert!(all_text.contains("src/main.rs"));
    }

    #[test]
    fn test_tool_result_renders_inline() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "Bash".to_string(),
                    input: "{\"command\":\"echo hi\"}".to_string(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "hi\n".to_string(),
                    is_error: false,
                    collapsed: false,
                },
            ],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("Bash"), "Expected tool name");
        assert!(all_text.contains("hi"), "Expected tool result content");
    }

    #[test]
    fn test_tool_result_collapsed_shows_truncated() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let long_output = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "Bash".to_string(),
                    input: "{\"command\":\"cat big.txt\"}".to_string(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: long_output,
                    is_error: false,
                    collapsed: true,
                },
            ],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("line 0"), "Expected first line");
        assert!(all_text.contains("line 19"), "Expected line 19 (20th line)");
        assert!(!all_text.contains("line 20"), "Line 20 should be hidden");
        assert!(all_text.contains("more lines"), "Expected 'more lines' indicator");
    }

    #[test]
    fn test_tool_result_error_styling() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "Bash".to_string(),
                    input: "{\"command\":\"false\"}".to_string(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "command failed".to_string(),
                    is_error: true,
                    collapsed: false,
                },
            ],
        });
        let lines = render_conversation(&conv, 80, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        // Tool header should show error indicator
        assert!(all_text.contains("Bash"), "Expected tool name");
        assert!(all_text.contains("✗"), "Expected error indicator on tool header");
        // Error label should appear before content
        assert!(all_text.contains("✗ Error"), "Expected error label");
        // Tool header should use error color
        let header_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.text.contains("Bash")))
            .expect("Expected tool header line");
        let name_span = header_line
            .spans
            .iter()
            .find(|s| s.text.contains("Bash"))
            .unwrap();
        assert_eq!(name_span.style.fg, Some(theme.error));
        // Content should use error color
        let content_line = lines.iter().find(|l| {
            l.spans.iter().any(|s| s.text.contains("command failed"))
        });
        assert!(content_line.is_some(), "Expected a line with error content");
        let error_span = content_line
            .unwrap()
            .spans
            .iter()
            .find(|s| s.text.contains("command failed"))
            .unwrap();
        assert_eq!(error_span.style.fg, Some(theme.error));
    }

    #[test]
    fn test_tool_result_empty_content_hidden() {
        let mut conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        conv.messages.push(Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "Edit".to_string(),
                    input: "{\"file_path\":\"test.rs\"}".to_string(),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: String::new(),
                    is_error: false,
                    collapsed: false,
                },
            ],
        });
        let lines = render_conversation(&conv, 80, &theme);
        // Should only have the label + tool use line, no result output
        assert!(lines.len() <= 3, "Empty result should produce no extra lines, got {}", lines.len());
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
        let pane = ClaudePane::new(&conv, &theme, 10, 0);
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }

    #[test]
    fn test_zero_area() {
        let conv = Conversation::new();
        let theme = crate::theme::Theme::default_theme();
        let pane = ClaudePane::new(&conv, &theme, 0, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        pane.render(area, &mut buf);
    }

    #[test]
    fn test_extract_primary_arg_bash() {
        let arg = extract_primary_arg("Bash", r#"{"command":"ls -la"}"#);
        assert_eq!(arg.as_deref(), Some("ls -la"));
    }

    #[test]
    fn test_extract_primary_arg_read() {
        let arg = extract_primary_arg("Read", r#"{"file_path":"src/main.rs"}"#);
        assert_eq!(arg.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn test_extract_primary_arg_invalid_json() {
        let arg = extract_primary_arg("Bash", "not json");
        assert!(arg.is_none());
    }
}
