/// Input history with JSONL persistence and fuzzy search.
use std::path::PathBuf;

/// Maximum number of entries to keep in history.
const MAX_ENTRIES: usize = 500;

pub struct InputHistory {
    entries: Vec<String>,
    path: PathBuf,
}

impl InputHistory {
    /// Create a new history backed by the default file path.
    pub fn new() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sexy-claude")
            .join("history.jsonl");
        let mut h = Self {
            entries: Vec::new(),
            path,
        };
        h.load();
        h
    }

    /// Load history from disk. Silently ignores errors.
    fn load(&mut self) {
        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(_) => return,
        };
        self.entries.clear();
        for line in content.lines() {
            if let Ok(s) = serde_json::from_str::<String>(line) {
                if !s.is_empty() {
                    self.entries.push(s);
                }
            }
        }
    }

    /// Save history to disk. Creates parent directories if needed.
    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut content = String::new();
        for entry in &self.entries {
            if let Ok(json) = serde_json::to_string(entry) {
                content.push_str(&json);
                content.push('\n');
            }
        }
        let _ = std::fs::write(&self.path, content);
    }

    /// Push a new entry to history. Deduplicates by moving existing matches to the end.
    pub fn push(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        // Remove existing duplicate
        self.entries.retain(|e| e != &text);
        // Add to end (most recent)
        self.entries.push(text);
        // Trim to max
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
        self.save();
    }

    /// Get entry by reverse index (0 = most recent).
    pub fn get_reverse(&self, index: usize) -> Option<&str> {
        if index < self.entries.len() {
            Some(&self.entries[self.entries.len() - 1 - index])
        } else {
            None
        }
    }

    /// Total number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Search entries using fuzzy matching. Returns (reverse_index, entry) pairs,
    /// sorted by match score descending.
    pub fn search(&self, query: &str) -> Vec<(usize, &str)> {
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;

        if query.is_empty() {
            // Return all entries, most recent first
            return self
                .entries
                .iter()
                .rev()
                .enumerate()
                .map(|(i, e)| (i, e.as_str()))
                .collect();
        }

        let matcher = SkimMatcherV2::default();
        let mut matches: Vec<(i64, usize, &str)> = self
            .entries
            .iter()
            .rev()
            .enumerate()
            .filter_map(|(rev_idx, entry)| {
                matcher
                    .fuzzy_match(entry, query)
                    .map(|score| (score, rev_idx, entry.as_str()))
            })
            .collect();

        matches.sort_by(|a, b| b.0.cmp(&a.0));
        matches
            .into_iter()
            .map(|(_, idx, entry)| (idx, entry))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_history() -> InputHistory {
        let dir = tempfile::tempdir().unwrap();
        InputHistory {
            entries: Vec::new(),
            path: dir.into_path().join("history.jsonl"),
        }
    }

    #[test]
    fn test_push_and_get() {
        let mut h = test_history();
        h.push("first".to_string());
        h.push("second".to_string());
        assert_eq!(h.get_reverse(0), Some("second"));
        assert_eq!(h.get_reverse(1), Some("first"));
        assert_eq!(h.get_reverse(2), None);
    }

    #[test]
    fn test_deduplication() {
        let mut h = test_history();
        h.push("hello".to_string());
        h.push("world".to_string());
        h.push("hello".to_string());
        assert_eq!(h.len(), 2);
        assert_eq!(h.get_reverse(0), Some("hello"));
        assert_eq!(h.get_reverse(1), Some("world"));
    }

    #[test]
    fn test_empty_not_pushed() {
        let mut h = test_history();
        h.push("".to_string());
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn test_max_entries() {
        let mut h = test_history();
        for i in 0..600 {
            h.push(format!("entry {i}"));
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        // Most recent should be the last one pushed
        assert_eq!(h.get_reverse(0), Some("entry 599"));
    }

    #[test]
    fn test_search_fuzzy() {
        let mut h = test_history();
        h.push("fix the login bug".to_string());
        h.push("add user authentication".to_string());
        h.push("fix the signup flow".to_string());

        let results = h.search("fix");
        assert_eq!(results.len(), 2);
        // Both "fix" entries should match
        assert!(results.iter().any(|(_, e)| e.contains("login")));
        assert!(results.iter().any(|(_, e)| e.contains("signup")));
    }

    #[test]
    fn test_search_empty_returns_all() {
        let mut h = test_history();
        h.push("first".to_string());
        h.push("second".to_string());
        let results = h.search("");
        assert_eq!(results.len(), 2);
        // Most recent first
        assert_eq!(results[0].1, "second");
    }

    #[test]
    fn test_jsonl_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");

        // Write
        {
            let mut h = InputHistory {
                entries: Vec::new(),
                path: path.clone(),
            };
            h.push("line one".to_string());
            h.push("line\nwith\nnewlines".to_string());
            h.push("line \"with\" quotes".to_string());
        }

        // Read back
        let mut h = InputHistory {
            entries: Vec::new(),
            path,
        };
        h.load();
        assert_eq!(h.len(), 3);
        assert_eq!(h.get_reverse(0), Some("line \"with\" quotes"));
        assert_eq!(h.get_reverse(1), Some("line\nwith\nnewlines"));
        assert_eq!(h.get_reverse(2), Some("line one"));
    }
}
