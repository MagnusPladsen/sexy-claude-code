use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::theme::Theme;

use super::claude_pane::{StyledLine, StyledSpan};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert a markdown string into styled lines ready for rendering.
///
/// Lines are NOT wrapped — the caller should run them through `wrap_spans()`.
pub fn render_markdown(text: &str, theme: &Theme) -> Vec<StyledLine> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax_theme_name = theme.syntax_theme_name();
    let syntax_theme = ts
        .themes
        .get(syntax_theme_name)
        .unwrap_or_else(|| ts.themes.values().next().unwrap());

    let base_style = Style::default().fg(theme.secondary);

    let mut ctx = RenderContext {
        lines: Vec::new(),
        current_spans: Vec::new(),
        style_stack: vec![base_style],
        list_stack: Vec::new(),
        blockquote_depth: 0,
        in_code_block: false,
        code_block_lang: String::new(),
        code_block_buf: String::new(),
        ss: &ss,
        syntax_theme,
        theme,
        base_style,
    };

    let opts = Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(text, opts);

    for event in parser {
        ctx.process_event(event);
    }

    // Flush any remaining spans
    ctx.flush_line();

    ctx.lines
}

// ---------------------------------------------------------------------------
// Render context
// ---------------------------------------------------------------------------

struct RenderContext<'a> {
    lines: Vec<StyledLine>,
    current_spans: Vec<StyledSpan>,
    style_stack: Vec<Style>,
    /// Stack tracking list nesting: Some(n) = ordered starting at n, None = unordered
    list_stack: Vec<Option<u64>>,
    /// Nesting depth of blockquotes (> 0 means we're inside a blockquote)
    blockquote_depth: usize,

    in_code_block: bool,
    code_block_lang: String,
    code_block_buf: String,

    ss: &'a SyntaxSet,
    syntax_theme: &'a syntect::highlighting::Theme,
    theme: &'a Theme,
    base_style: Style,
}

impl<'a> RenderContext<'a> {
    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or(self.base_style)
    }

    fn push_modifier(&mut self, modifier: Modifier) {
        let current = self.current_style();
        self.style_stack.push(current.add_modifier(modifier));
    }

    fn push_style(&mut self, style: Style) {
        self.style_stack.push(style);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn flush_line(&mut self) {
        if !self.current_spans.is_empty() {
            self.lines.push(StyledLine {
                spans: std::mem::take(&mut self.current_spans),
            });
        }
    }

    fn push_newline(&mut self) {
        self.flush_line();
    }

    fn push_text(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        // Split on newlines to create separate lines
        let mut first = true;
        for chunk in text.split('\n') {
            if !first {
                self.push_newline();
            }
            first = false;
            if !chunk.is_empty() {
                self.current_spans.push(StyledSpan {
                    text: chunk.to_string(),
                    style,
                });
            }
        }
    }

    fn process_event(&mut self, event: Event) {
        match event {
            // --- Code blocks (buffered for syntect) ---
            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush_line();
                self.in_code_block = true;
                self.code_block_buf.clear();
                self.code_block_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                // Render the opening fence line
                let fence_label = if self.code_block_lang.is_empty() {
                    "```".to_string()
                } else {
                    format!("```{}", self.code_block_lang)
                };
                let fence_style = Style::default()
                    .fg(Color::Rgb(127, 132, 156))
                    .add_modifier(Modifier::DIM);
                self.lines
                    .push(StyledLine::plain(&fence_label, fence_style));
            }

            Event::End(TagEnd::CodeBlock) => {
                self.in_code_block = false;
                // Highlight and emit buffered code
                self.emit_highlighted_code();
                // Closing fence
                let fence_style = Style::default()
                    .fg(Color::Rgb(127, 132, 156))
                    .add_modifier(Modifier::DIM);
                self.lines.push(StyledLine::plain("```", fence_style));
            }

            Event::Text(text) if self.in_code_block => {
                self.code_block_buf.push_str(&text);
            }

            // --- Block-level elements ---
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_line();
                let header_style = Style::default()
                    .fg(Color::Rgb(203, 166, 247))
                    .add_modifier(Modifier::BOLD);
                self.push_style(header_style);
                // Add markdown-style prefix
                let prefix = "#".repeat(level as usize);
                self.current_spans.push(StyledSpan {
                    text: format!("{prefix} "),
                    style: header_style,
                });
            }

            Event::End(TagEnd::Heading(_)) => {
                self.pop_style();
                self.flush_line();
            }

            Event::Start(Tag::Paragraph) => {
                // Start a new paragraph — add blank line if we already have content
                if !self.lines.is_empty() {
                    // Only add blank line if the previous line wasn't already empty
                    let prev_empty = self
                        .lines
                        .last()
                        .map(|l| l.spans.is_empty())
                        .unwrap_or(true);
                    if !prev_empty {
                        self.lines.push(StyledLine::empty());
                    }
                }
            }

            Event::End(TagEnd::Paragraph) => {
                self.flush_line();
            }

            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_line();
                self.blockquote_depth += 1;
                let quote_style = Style::default()
                    .fg(self.theme.info)
                    .add_modifier(Modifier::DIM);
                self.push_style(quote_style);
            }

            Event::End(TagEnd::BlockQuote(_)) => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pop_style();
                self.flush_line();
            }

            Event::Start(Tag::List(start_num)) => {
                self.flush_line();
                self.list_stack.push(start_num);
            }

            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                self.flush_line();
            }

            Event::Start(Tag::Item) => {
                self.flush_line();
                let style = self.current_style();
                let prefix = match self.list_stack.last() {
                    Some(Some(n)) => {
                        let num = *n;
                        // Increment for next item
                        if let Some(Some(ref mut counter)) = self.list_stack.last_mut() {
                            *counter += 1;
                        }
                        format!("  {num}. ")
                    }
                    _ => "  - ".to_string(),
                };
                self.current_spans.push(StyledSpan {
                    text: prefix,
                    style,
                });
            }

            Event::End(TagEnd::Item) => {
                self.flush_line();
            }

            // --- Inline elements ---
            Event::Start(Tag::Strong) => {
                self.push_modifier(Modifier::BOLD);
            }

            Event::End(TagEnd::Strong) => {
                self.pop_style();
            }

            Event::Start(Tag::Emphasis) => {
                self.push_modifier(Modifier::ITALIC);
            }

            Event::End(TagEnd::Emphasis) => {
                self.pop_style();
            }

            Event::Start(Tag::Strikethrough) => {
                self.push_modifier(Modifier::CROSSED_OUT);
            }

            Event::End(TagEnd::Strikethrough) => {
                self.pop_style();
            }

            Event::Start(Tag::Link { dest_url, .. }) => {
                let link_style = Style::default()
                    .fg(self.theme.info)
                    .add_modifier(Modifier::UNDERLINED);
                self.push_style(link_style);
                // Store URL for later (we'll show it after the text)
                // For now, just style the text
                let _ = dest_url; // URL available if we want to show it
            }

            Event::End(TagEnd::Link) => {
                self.pop_style();
            }

            // Inline code
            Event::Code(text) => {
                let code_style = Style::default().fg(Color::Rgb(166, 227, 161));
                self.current_spans.push(StyledSpan {
                    text: text.to_string(),
                    style: code_style,
                });
            }

            // Plain text
            Event::Text(text) => {
                let style = self.current_style();
                if self.blockquote_depth > 0 {
                    // In blockquote context, prefix each line with "| "
                    let prefixed = text
                        .split('\n')
                        .map(|l| format!("| {l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.push_text(&prefixed, style);
                } else {
                    self.push_text(&text, style);
                }
            }

            Event::SoftBreak => {
                let style = self.current_style();
                self.current_spans.push(StyledSpan {
                    text: " ".to_string(),
                    style,
                });
            }

            Event::HardBreak => {
                self.push_newline();
            }

            Event::Rule => {
                self.flush_line();
                let sep_style = Style::default().fg(Color::Rgb(69, 71, 90));
                self.lines
                    .push(StyledLine::plain(&"─".repeat(40), sep_style));
            }

            // Ignore everything else (HTML, footnotes, etc.)
            _ => {}
        }
    }

    /// Highlight the buffered code block using syntect and emit styled lines.
    fn emit_highlighted_code(&mut self) {
        let fallback_style = Style::default().fg(Color::Rgb(180, 190, 220));

        let syntax = if !self.code_block_lang.is_empty() {
            self.ss.find_syntax_by_token(&self.code_block_lang)
        } else {
            None
        };

        match syntax {
            Some(syn) => {
                let mut h = HighlightLines::new(syn, self.syntax_theme);
                for line in self.code_block_buf.lines() {
                    let ranges = h.highlight_line(line, self.ss).unwrap_or_default();
                    let spans: Vec<StyledSpan> = ranges
                        .iter()
                        .map(|(style, text)| {
                            let fg = Color::Rgb(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            );
                            StyledSpan {
                                text: text.to_string(),
                                style: Style::default().fg(fg),
                            }
                        })
                        .collect();
                    self.lines.push(StyledLine { spans });
                }
            }
            None => {
                // No syntax found — plain code style
                for line in self.code_block_buf.lines() {
                    self.lines.push(StyledLine::plain(line, fallback_style));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn test_theme() -> Theme {
        Theme::default_theme()
    }

    #[test]
    fn test_plain_text() {
        let lines = render_markdown("Hello world", &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("Hello world"));
    }

    #[test]
    fn test_bold_text() {
        let lines = render_markdown("**bold text**", &test_theme());
        let bold_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.text.contains("bold text"));
        assert!(bold_span.is_some());
        let span = bold_span.unwrap();
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_italic_text() {
        let lines = render_markdown("*italic text*", &test_theme());
        let italic_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.text.contains("italic text"));
        assert!(italic_span.is_some());
        let span = italic_span.unwrap();
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_inline_code() {
        let lines = render_markdown("Use `cargo build` to compile", &test_theme());
        let code_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.text.contains("cargo build"));
        assert!(code_span.is_some());
        let span = code_span.unwrap();
        assert_eq!(span.style.fg, Some(Color::Rgb(166, 227, 161)));
    }

    #[test]
    fn test_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let lines = render_markdown(md, &test_theme());
        // Should have: fence, highlighted code, fence
        assert!(lines.len() >= 3);
        // First and last lines should be fences
        let first_text: String = lines[0].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(first_text.contains("```rust"));
        let last_text: String = lines
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(last_text.contains("```"));
    }

    #[test]
    fn test_code_block_unknown_language() {
        let md = "```\nsome code\n```";
        let lines = render_markdown(md, &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("some code"));
    }

    #[test]
    fn test_headers() {
        let lines = render_markdown("# Title\n## Subtitle", &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("# Title"));
        assert!(all_text.contains("## Subtitle"));
    }

    #[test]
    fn test_unordered_list() {
        let lines = render_markdown("- item one\n- item two", &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("item one"));
        assert!(all_text.contains("item two"));
    }

    #[test]
    fn test_ordered_list() {
        let lines = render_markdown("1. first\n2. second", &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("1."));
        assert!(all_text.contains("first"));
    }

    #[test]
    fn test_empty_input() {
        let lines = render_markdown("", &test_theme());
        assert!(lines.is_empty());
    }

    #[test]
    fn test_horizontal_rule() {
        let lines = render_markdown("---", &test_theme());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(all_text.contains("─"));
    }
}
