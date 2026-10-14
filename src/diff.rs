/// Line-level diff using the Hunt-Szymanski / LCS approach.
/// Produces a list of DiffOp values that can be rendered as a unified diff.

#[derive(Debug, Clone, PartialEq)]
pub enum DiffOp<'a> {
    Equal(&'a str),
    Remove(&'a str),
    Add(&'a str),
}

/// Compute a line-level diff between `old` and `new` text.
/// Returns a sequence of DiffOp operations.
pub fn diff_lines<'a>(old: &'a str, new: &'a str) -> Vec<DiffOp<'a>> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let lcs = lcs_table(&old_lines, &new_lines);
    build_diff(&old_lines, &new_lines, &lcs)
}

/// Build the LCS length table for two sequences of lines.
fn lcs_table(old: &[&str], new: &[&str]) -> Vec<Vec<usize>> {
    let m = old.len();
    let n = new.len();
    let mut table = vec![vec![0usize; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }
    table
}

/// Walk the LCS table backwards to produce diff operations.
fn build_diff<'a>(old: &[&'a str], new: &[&'a str], table: &[Vec<usize>]) -> Vec<DiffOp<'a>> {
    let mut ops = Vec::new();
    let mut i = old.len();
    let mut j = new.len();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            ops.push(DiffOp::Equal(old[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || table[i][j - 1] >= table[i - 1][j]) {
            ops.push(DiffOp::Add(new[j - 1]));
            j -= 1;
        } else {
            ops.push(DiffOp::Remove(old[i - 1]));
            i -= 1;
        }
    }

    ops.reverse();
    ops
}

/// Format a diff as a unified-style string with +/- prefixes.
pub fn format_unified(ops: &[DiffOp<'_>]) -> String {
    let mut out = String::new();
    for op in ops {
        match op {
            DiffOp::Equal(line) => {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
            DiffOp::Remove(line) => {
                out.push_str("- ");
                out.push_str(line);
                out.push('\n');
            }
            DiffOp::Add(line) => {
                out.push_str("+ ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Compute a word-level diff between two lines.
/// Splits on whitespace boundaries, preserving whitespace as separate tokens.
/// Returns a sequence of DiffOp operations at the word level.
pub fn diff_words<'a>(old: &'a str, new: &'a str) -> Vec<DiffOp<'a>> {
    let old_words = tokenize_words(old);
    let new_words = tokenize_words(new);

    let lcs = lcs_table(&old_words, &new_words);
    build_diff(&old_words, &new_words, &lcs)
}

/// Split a string into tokens preserving whitespace as separate entries.
/// "hello  world" → ["hello", "  ", "world"]
fn tokenize_words(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut chars = s.char_indices().peekable();

    while let Some(&(start, ch)) = chars.peek() {
        if ch.is_whitespace() {
            // Consume all consecutive whitespace
            while chars.peek().is_some_and(|&(_, c)| c.is_whitespace()) {
                chars.next();
            }
            let end = chars.peek().map(|&(i, _)| i).unwrap_or(s.len());
            tokens.push(&s[start..end]);
        } else {
            // Consume until whitespace
            chars.next();
            while chars.peek().is_some_and(|&(_, c)| !c.is_whitespace()) {
                chars.next();
            }
            let end = chars.peek().map(|&(i, _)| i).unwrap_or(s.len());
            tokens.push(&s[start..end]);
        }
    }
    tokens
}

/// Return only the changed operations (no Equal), with limited context.
/// Shows `context` equal lines before/after each change group.
pub fn with_context<'a>(ops: &'a [DiffOp<'a>], context: usize) -> Vec<&'a DiffOp<'a>> {
    if ops.is_empty() {
        return Vec::new();
    }

    // Mark which lines should be visible
    let mut visible = vec![false; ops.len()];

    for (i, op) in ops.iter().enumerate() {
        if !matches!(op, DiffOp::Equal(_)) {
            // Mark this change and surrounding context
            let start = i.saturating_sub(context);
            let end = (i + context + 1).min(ops.len());
            for v in &mut visible[start..end] {
                *v = true;
            }
        }
    }

    ops.iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(_, op)| op)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical() {
        let ops = diff_lines("hello\nworld", "hello\nworld");
        assert_eq!(ops, vec![DiffOp::Equal("hello"), DiffOp::Equal("world")]);
    }

    #[test]
    fn test_simple_edit() {
        let ops = diff_lines("hello\nworld", "hello\nearth");
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("hello"),
                DiffOp::Remove("world"),
                DiffOp::Add("earth"),
            ]
        );
    }

    #[test]
    fn test_addition() {
        let ops = diff_lines("a\nc", "a\nb\nc");
        assert_eq!(
            ops,
            vec![DiffOp::Equal("a"), DiffOp::Add("b"), DiffOp::Equal("c"),]
        );
    }

    #[test]
    fn test_removal() {
        let ops = diff_lines("a\nb\nc", "a\nc");
        assert_eq!(
            ops,
            vec![DiffOp::Equal("a"), DiffOp::Remove("b"), DiffOp::Equal("c"),]
        );
    }

    #[test]
    fn test_empty_to_something() {
        let ops = diff_lines("", "hello");
        assert_eq!(ops, vec![DiffOp::Add("hello")]);
    }

    #[test]
    fn test_something_to_empty() {
        let ops = diff_lines("hello", "");
        assert_eq!(ops, vec![DiffOp::Remove("hello")]);
    }

    #[test]
    fn test_format_unified() {
        let ops = diff_lines("hello\nworld", "hello\nearth");
        let output = format_unified(&ops);
        assert_eq!(output, "  hello\n- world\n+ earth\n");
    }

    #[test]
    fn test_with_context() {
        let ops = vec![
            DiffOp::Equal("line1"),
            DiffOp::Equal("line2"),
            DiffOp::Equal("line3"),
            DiffOp::Remove("old"),
            DiffOp::Add("new"),
            DiffOp::Equal("line5"),
            DiffOp::Equal("line6"),
            DiffOp::Equal("line7"),
        ];
        let visible = with_context(&ops, 1);
        // Should show: line3 (1 before), old, new, line5 (1 after)
        assert_eq!(visible.len(), 4);
        assert_eq!(*visible[0], DiffOp::Equal("line3"));
        assert_eq!(*visible[1], DiffOp::Remove("old"));
        assert_eq!(*visible[2], DiffOp::Add("new"));
        assert_eq!(*visible[3], DiffOp::Equal("line5"));
    }

    #[test]
    fn test_duplicate_lines_handled() {
        // This is the case the old naive diff got wrong
        let old = "{\n    a\n}\n{\n    b\n}";
        let new = "{\n    a\n}\n{\n    c\n}";
        let ops = diff_lines(old, new);
        // Should only show b→c change, not match braces incorrectly
        let changes: Vec<_> = ops
            .iter()
            .filter(|o| !matches!(o, DiffOp::Equal(_)))
            .collect();
        assert_eq!(changes.len(), 2); // Remove "    b", Add "    c"
    }

    #[test]
    fn test_tokenize_words() {
        let tokens = tokenize_words("hello  world");
        assert_eq!(tokens, vec!["hello", "  ", "world"]);
    }

    #[test]
    fn test_tokenize_words_leading_space() {
        let tokens = tokenize_words("  hello");
        assert_eq!(tokens, vec!["  ", "hello"]);
    }

    #[test]
    fn test_tokenize_words_empty() {
        let tokens = tokenize_words("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_diff_words_single_word_change() {
        let ops = diff_words("hello world", "hello earth");
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("hello"),
                DiffOp::Equal(" "),
                DiffOp::Remove("world"),
                DiffOp::Add("earth"),
            ]
        );
    }

    #[test]
    fn test_diff_words_identical() {
        let ops = diff_words("foo bar", "foo bar");
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal("foo"),
                DiffOp::Equal(" "),
                DiffOp::Equal("bar"),
            ]
        );
    }

    #[test]
    fn test_diff_words_insertion() {
        let ops = diff_words("a c", "a b c");
        let adds: Vec<_> = ops.iter().filter(|o| matches!(o, DiffOp::Add(_))).collect();
        assert!(!adds.is_empty());
    }
}
