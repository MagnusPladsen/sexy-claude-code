#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

use crate::theme::Theme;

pub struct InputEditor {
    content: String,
    cursor: usize,
}

impl InputEditor {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Insert a string at the cursor position (used for paste).
    pub fn insert_str(&mut self, s: &str) {
        self.content.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev = self.content[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.content.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.content.len() {
            let next = self.content[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.content.len());
            self.content.drain(self.cursor..next);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.content[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.content.len() {
            self.cursor = self.content[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.content.len());
        }
    }

    pub fn move_home(&mut self) {
        // Move to start of current line
        self.cursor = self.content[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
    }

    pub fn move_end(&mut self) {
        // Move to end of current line
        self.cursor = self.content[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.content.len());
    }

    pub fn take_content(&mut self) -> String {
        let content = std::mem::take(&mut self.content);
        self.cursor = 0;
        content
    }

    /// Replace the entire content and move cursor to end.
    pub fn set_content(&mut self, text: &str) {
        self.content = text.to_string();
        self.cursor = self.content.len();
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor
    }

    /// Get the (col, row) position of the cursor relative to the text content
    pub fn cursor_xy(&self) -> (u16, u16) {
        let before_cursor = &self.content[..self.cursor];
        let row = before_cursor.matches('\n').count() as u16;
        let col = before_cursor
            .rsplit('\n')
            .next()
            .map(|s| s.len() as u16)
            .unwrap_or(0);
        (col, row)
    }
}

pub struct InputWidget<'a> {
    editor: &'a InputEditor,
    theme: &'a Theme,
}

impl<'a> InputWidget<'a> {
    pub fn new(editor: &'a InputEditor, theme: &'a Theme) -> Self {
        Self { editor, theme }
    }
}

impl<'a> Widget for InputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default()
            .fg(self.theme.input_fg)
            .bg(self.theme.input_bg);
        let cursor_style = Style::default()
            .fg(self.theme.input_bg)
            .bg(self.theme.primary);

        // Fill background
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buf[(x, y)].set_style(style);
                buf[(x, y)].set_symbol(" ");
            }
        }

        if self.editor.is_empty() {
            // Show cursor at position 0
            if let Some(cell) = buf.cell_mut((area.x, area.y)) {
                cell.set_symbol(" ");
                cell.set_style(cursor_style);
            }
            let placeholder_style = Style::default()
                .fg(self.theme.input_placeholder)
                .bg(self.theme.input_bg);
            let placeholder = "Type a message... (Enter to send, Shift+Enter for newline)";
            for (i, ch) in placeholder.chars().enumerate() {
                let x = area.x + 1 + i as u16;
                if x >= area.right() {
                    break;
                }
                buf[(x, area.y)].set_symbol(&ch.to_string());
                buf[(x, area.y)].set_style(placeholder_style);
            }
            return;
        }

        // Render content with cursor
        let cursor_pos = self.editor.cursor_position();
        let mut x = area.x;
        let mut y = area.y;
        let mut byte_offset = 0usize;

        for ch in self.editor.content().chars() {
            if y >= area.bottom() {
                break;
            }
            let is_cursor = byte_offset == cursor_pos;

            if ch == '\n' {
                // Show cursor on the newline position (as a block at end of line)
                if is_cursor && x < area.right() && y < area.bottom() {
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_symbol(" ");
                        cell.set_style(cursor_style);
                    }
                }
                x = area.x;
                y += 1;
                byte_offset += ch.len_utf8();
                continue;
            }
            if x >= area.right() {
                x = area.x;
                y += 1;
                if y >= area.bottom() {
                    break;
                }
            }
            let char_style = if is_cursor { cursor_style } else { style };
            buf[(x, y)].set_symbol(&ch.to_string());
            buf[(x, y)].set_style(char_style);
            x += 1;
            byte_offset += ch.len_utf8();
        }

        // If cursor is at end of content, show cursor block after last char
        if cursor_pos == self.editor.content().len() {
            if x >= area.right() {
                x = area.x;
                y += 1;
            }
            if y < area.bottom() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(cursor_style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_content() {
        let mut editor = InputEditor::new();
        editor.insert_char('H');
        editor.insert_char('i');
        assert_eq!(editor.content(), "Hi");
        assert_eq!(editor.cursor_position(), 2);
    }

    #[test]
    fn test_backspace() {
        let mut editor = InputEditor::new();
        editor.insert_char('A');
        editor.insert_char('B');
        editor.backspace();
        assert_eq!(editor.content(), "A");
        assert_eq!(editor.cursor_position(), 1);
    }

    #[test]
    fn test_backspace_empty() {
        let mut editor = InputEditor::new();
        editor.backspace();
        assert_eq!(editor.content(), "");
        assert_eq!(editor.cursor_position(), 0);
    }

    #[test]
    fn test_move_left_right() {
        let mut editor = InputEditor::new();
        editor.insert_char('A');
        editor.insert_char('B');
        editor.insert_char('C');
        editor.move_left();
        editor.move_left();
        editor.insert_char('X');
        assert_eq!(editor.content(), "AXBC");
    }

    #[test]
    fn test_delete() {
        let mut editor = InputEditor::new();
        editor.insert_char('A');
        editor.insert_char('B');
        editor.insert_char('C');
        editor.move_left();
        editor.move_left();
        editor.delete();
        assert_eq!(editor.content(), "AC");
    }

    #[test]
    fn test_take_content() {
        let mut editor = InputEditor::new();
        editor.insert_char('H');
        editor.insert_char('i');
        let content = editor.take_content();
        assert_eq!(content, "Hi");
        assert!(editor.is_empty());
        assert_eq!(editor.cursor_position(), 0);
    }

    #[test]
    fn test_newline_and_cursor_xy() {
        let mut editor = InputEditor::new();
        editor.insert_char('A');
        editor.insert_char('B');
        editor.insert_newline();
        editor.insert_char('C');
        assert_eq!(editor.cursor_xy(), (1, 1));
    }

    #[test]
    fn test_home_end() {
        let mut editor = InputEditor::new();
        editor.insert_char('H');
        editor.insert_char('e');
        editor.insert_char('l');
        editor.insert_char('l');
        editor.insert_char('o');
        editor.move_home();
        assert_eq!(editor.cursor_position(), 0);
        editor.move_end();
        assert_eq!(editor.cursor_position(), 5);
    }
}
