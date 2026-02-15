use std::path::PathBuf;
use std::time::SystemTime;

/// Metadata for a single Claude session, discovered from disk.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub project_path: String,
    pub last_modified: SystemTime,
    pub preview: String,
}

impl SessionInfo {
    /// Human-readable relative time like "2h ago", "3d ago".
    pub fn age_string(&self) -> String {
        let elapsed = self
            .last_modified
            .elapsed()
            .unwrap_or(std::time::Duration::ZERO);
        let secs = elapsed.as_secs();
        if secs < 60 {
            "just now".to_string()
        } else if secs < 3600 {
            format!("{}m ago", secs / 60)
        } else if secs < 86400 {
            format!("{}h ago", secs / 3600)
        } else {
            format!("{}d ago", secs / 86400)
        }
    }
}

/// Discover all sessions across all projects, sorted by most recent first.
pub fn discover_sessions() -> Vec<SessionInfo> {
    let projects_dir = match dirs::home_dir() {
        Some(home) => home.join(".claude/projects"),
        None => return Vec::new(),
    };

    let entries = match std::fs::read_dir(&projects_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();

    for entry in entries.flatten() {
        let project_dir = entry.path();
        if !project_dir.is_dir() {
            continue;
        }

        let project_slug = entry.file_name().to_string_lossy().to_string();
        let project_path = slug_to_path(&project_slug);

        scan_project_sessions(&project_dir, &project_path, &mut sessions);
    }

    // Sort by most recent first
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    sessions
}

/// Scan a single project directory for session JSONL files.
fn scan_project_sessions(
    project_dir: &PathBuf,
    project_path: &str,
    sessions: &mut Vec<SessionInfo>,
) {
    let entries = match std::fs::read_dir(project_dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let session_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        let last_modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let preview = extract_preview(&path);

        sessions.push(SessionInfo {
            session_id,
            project_path: project_path.to_string(),
            last_modified,
            preview,
        });
    }
}

/// Extract the first user message text from a session JSONL file as a preview.
fn extract_preview(path: &PathBuf) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    for line in content.lines().take(20) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.get("type").and_then(|t| t.as_str()) == Some("user") {
                if let Some(message) = value.get("message") {
                    // Content can be a string or array of content blocks
                    if let Some(content) = message.get("content") {
                        if let Some(text) = content.as_str() {
                            return truncate_preview(text);
                        }
                        if let Some(arr) = content.as_array() {
                            for block in arr {
                                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str())
                                    {
                                        return truncate_preview(text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    String::new()
}

/// Truncate preview text to a reasonable length.
fn truncate_preview(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text);
    let trimmed = first_line.trim();
    if trimmed.len() > 80 {
        format!("{}...", &trimmed[..77])
    } else {
        trimmed.to_string()
    }
}

/// Convert a project directory slug back to a readable path.
///
/// Slug format: `-Users-magnuspladsen-git-sexy-claude-code`
/// becomes: `/Users/magnuspladsen/git/sexy-claude-code`
fn slug_to_path(slug: &str) -> String {
    // The slug uses `-` as separator, but the original path also has `-` in names.
    // We can't perfectly reverse this, so just show the last 2-3 segments.
    let parts: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return parts.join("/");
    }
    // Show last 2 path components as a best-effort display name
    let tail: Vec<&str> = parts.iter().rev().take(2).copied().collect();
    tail.into_iter().rev().collect::<Vec<_>>().join("/")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_preview_short() {
        assert_eq!(truncate_preview("Hello world"), "Hello world");
    }

    #[test]
    fn test_truncate_preview_long() {
        let long = "a".repeat(100);
        let result = truncate_preview(&long);
        assert!(result.len() <= 80);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_preview_multiline() {
        assert_eq!(
            truncate_preview("First line\nSecond line"),
            "First line"
        );
    }

    #[test]
    fn test_slug_to_path() {
        assert_eq!(
            slug_to_path("-Users-magnuspladsen-git-sexy-claude-code"),
            "claude/code"
        );
    }

    #[test]
    fn test_slug_to_path_short() {
        assert_eq!(slug_to_path("-Users-magnuspladsen"), "Users/magnuspladsen");
    }

    #[test]
    fn test_age_string_just_now() {
        let info = SessionInfo {
            session_id: "test".to_string(),
            project_path: "test".to_string(),
            last_modified: SystemTime::now(),
            preview: String::new(),
        };
        assert_eq!(info.age_string(), "just now");
    }

    #[test]
    fn test_discover_sessions_no_crash() {
        // Should not crash even if ~/.claude doesn't exist
        let sessions = discover_sessions();
        let _ = sessions;
    }

    #[test]
    fn test_extract_preview_from_json_string_content() {
        // Create a temp file with JSONL content
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"user","message":{"role":"user","content":"Hello Claude"},"timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(extract_preview(&path.to_path_buf()), "Hello Claude");
    }

    #[test]
    fn test_extract_preview_from_json_array_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Array content"}]},"timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(extract_preview(&path.to_path_buf()), "Array content");
    }

    #[test]
    fn test_extract_preview_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, "").unwrap();
        assert_eq!(extract_preview(&path.to_path_buf()), "");
    }
}
