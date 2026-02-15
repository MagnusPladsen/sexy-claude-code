use std::path::{Path, PathBuf};

/// A custom command loaded from a `.md` file in `.claude/commands/` or `~/.claude/commands/`.
#[derive(Debug, Clone)]
pub struct CustomCommand {
    pub name: String,
    pub description: String,
    pub body: String,
    pub accepts_args: bool,
}

impl CustomCommand {
    /// Build the final prompt text, substituting `$ARGUMENTS` with the given args.
    pub fn render(&self, args: &str) -> String {
        if self.accepts_args {
            self.body.replace("$ARGUMENTS", args)
        } else {
            self.body.clone()
        }
    }
}

/// Load all custom commands from both project-level and user-level directories.
pub fn load_all_commands() -> Vec<CustomCommand> {
    let mut commands = Vec::new();

    // Project-level: .claude/commands/ relative to CWD
    let project_dir = PathBuf::from(".claude/commands");
    load_commands_from_dir(&project_dir, &mut commands);

    // User-level: ~/.claude/commands/
    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".claude/commands");
        load_commands_from_dir(&user_dir, &mut commands);
    }

    commands
}

/// Scan a directory for `.md` files and parse each as a custom command.
fn load_commands_from_dir(dir: &Path, commands: &mut Vec<CustomCommand>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip if we already have a command with this name (project takes precedence)
        if commands.iter().any(|c| c.name == name) {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(cmd) = parse_command(&name, &content) {
            commands.push(cmd);
        }
    }
}

/// Parse a `.md` file content into a `CustomCommand`.
///
/// Supports optional YAML-style frontmatter delimited by `---`:
/// ```text
/// ---
/// description: Some description
/// allowed-tools: tool1, tool2
/// ---
/// The prompt body here, possibly with $ARGUMENTS.
/// ```
fn parse_command(name: &str, content: &str) -> Option<CustomCommand> {
    let (description, body) = parse_frontmatter(content);
    let body = body.trim().to_string();

    if body.is_empty() {
        return None;
    }

    let accepts_args = body.contains("$ARGUMENTS");

    Some(CustomCommand {
        name: name.to_string(),
        description,
        body,
        accepts_args,
    })
}

/// Extract frontmatter and body from a markdown file.
///
/// Returns `(description, body)`. If no frontmatter, description is empty
/// and body is the entire content.
fn parse_frontmatter(content: &str) -> (String, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), content.to_string());
    }

    // Find the closing ---
    let after_opening = &trimmed[3..];
    let after_opening = after_opening.trim_start_matches(['\r', '\n']);

    if let Some(end_pos) = after_opening.find("\n---") {
        let frontmatter = &after_opening[..end_pos];
        let body_start = end_pos + 4; // "\n---"
        let body = after_opening[body_start..].trim_start_matches(['\r', '\n']);

        let description = extract_field(frontmatter, "description");

        (description, body.to_string())
    } else {
        // No closing ---, treat entire content as body
        (String::new(), content.to_string())
    }
}

/// Extract a simple `key: value` field from frontmatter text.
fn extract_field(frontmatter: &str, key: &str) -> String {
    let prefix = format!("{key}:");
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            return rest.trim().to_string();
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_simple() {
        let content = "Do something useful";
        let cmd = parse_command("test", content).unwrap();
        assert_eq!(cmd.name, "test");
        assert_eq!(cmd.description, "");
        assert_eq!(cmd.body, "Do something useful");
        assert!(!cmd.accepts_args);
    }

    #[test]
    fn test_parse_command_with_frontmatter() {
        let content = "---\ndescription: A helpful command\nallowed-tools: Bash\n---\nDo the thing";
        let cmd = parse_command("helper", content).unwrap();
        assert_eq!(cmd.name, "helper");
        assert_eq!(cmd.description, "A helpful command");
        assert_eq!(cmd.body, "Do the thing");
        assert!(!cmd.accepts_args);
    }

    #[test]
    fn test_parse_command_with_arguments() {
        let content = "---\ndescription: Search for stuff\n---\nSearch the codebase for $ARGUMENTS";
        let cmd = parse_command("search", content).unwrap();
        assert!(cmd.accepts_args);
        assert_eq!(cmd.render("foo bar"), "Search the codebase for foo bar");
    }

    #[test]
    fn test_parse_command_empty_body() {
        let content = "---\ndescription: Empty\n---\n";
        assert!(parse_command("empty", content).is_none());
    }

    #[test]
    fn test_parse_command_no_frontmatter() {
        let content = "Just a plain prompt with $ARGUMENTS placeholder";
        let cmd = parse_command("plain", content).unwrap();
        assert_eq!(cmd.description, "");
        assert!(cmd.accepts_args);
    }

    #[test]
    fn test_parse_frontmatter_no_closing() {
        let content = "---\ndescription: Broken\nThis has no closing delimiter";
        let (desc, body) = parse_frontmatter(content);
        assert_eq!(desc, "");
        assert_eq!(body, content);
    }

    #[test]
    fn test_render_without_args() {
        let cmd = CustomCommand {
            name: "test".to_string(),
            description: String::new(),
            body: "Fixed prompt text".to_string(),
            accepts_args: false,
        };
        assert_eq!(cmd.render("ignored"), "Fixed prompt text");
    }

    #[test]
    fn test_render_with_args() {
        let cmd = CustomCommand {
            name: "test".to_string(),
            description: String::new(),
            body: "Find $ARGUMENTS in the code".to_string(),
            accepts_args: true,
        };
        assert_eq!(cmd.render("bug #42"), "Find bug #42 in the code");
    }

    #[test]
    fn test_extract_field_missing() {
        assert_eq!(extract_field("description: hello", "missing"), "");
    }

    #[test]
    fn test_extract_field_present() {
        let fm = "description: My desc\nallowed-tools: Bash, Read";
        assert_eq!(extract_field(fm, "description"), "My desc");
        assert_eq!(extract_field(fm, "allowed-tools"), "Bash, Read");
    }

    #[test]
    fn test_load_all_commands_no_crash() {
        // Should not crash even if directories don't exist
        let commands = load_all_commands();
        // We can't assert specific commands since it depends on the environment,
        // but it should not panic
        let _ = commands;
    }
}
