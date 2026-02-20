use std::process::Command;

/// Lightweight snapshot of git repo state.
#[derive(Debug, Clone, Default)]
pub struct GitInfo {
    /// Current branch name (e.g. "main", "feature/foo").
    pub branch: Option<String>,
    /// Number of dirty (modified/untracked) files.
    pub dirty_count: usize,
}

impl GitInfo {
    /// Gather git info from the current working directory.
    /// Returns default (no branch) if not in a git repo or git is not available.
    pub fn gather() -> Self {
        let branch = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            });

        let dirty_count = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.is_empty())
                    .count()
            })
            .unwrap_or(0);

        Self {
            branch,
            dirty_count,
        }
    }

    /// Format for display in status bar: " main" or " main *3"
    pub fn display(&self) -> Option<String> {
        self.branch.as_ref().map(|b| {
            if self.dirty_count > 0 {
                format!(" {b} *{}", self.dirty_count)
            } else {
                format!(" {b}")
            }
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_clean() {
        let info = GitInfo {
            branch: Some("main".to_string()),
            dirty_count: 0,
        };
        assert_eq!(info.display(), Some(" main".to_string()));
        assert!(!info.is_dirty());
    }

    #[test]
    fn test_display_dirty() {
        let info = GitInfo {
            branch: Some("feature/foo".to_string()),
            dirty_count: 3,
        };
        assert_eq!(info.display(), Some(" feature/foo *3".to_string()));
        assert!(info.is_dirty());
    }

    #[test]
    fn test_display_no_branch() {
        let info = GitInfo::default();
        assert_eq!(info.display(), None);
    }

    #[test]
    fn test_gather_runs_in_git_repo() {
        // This test runs in the project repo, so should find a branch
        let info = GitInfo::gather();
        assert!(info.branch.is_some());
    }
}
