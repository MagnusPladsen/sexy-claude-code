use serde::Deserialize;

/// A single todo item from Claude's TodoWrite tool.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// Tracks the current state of Claude's todo list.
#[derive(Debug, Default)]
pub struct TodoTracker {
    pub items: Vec<TodoItem>,
}

/// Raw shape of TodoWrite input JSON.
#[derive(Deserialize)]
struct RawTodoWrite {
    todos: Vec<RawTodo>,
}

#[derive(Deserialize)]
struct RawTodo {
    id: Option<String>,
    content: Option<String>,
    status: Option<String>,
}

impl TodoTracker {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Update the todo list from a TodoWrite tool_use input JSON string.
    /// TodoWrite replaces the entire list each time it's called.
    pub fn apply_todo_write(&mut self, input_json: &str) {
        let raw: RawTodoWrite = match serde_json::from_str(input_json) {
            Ok(r) => r,
            Err(_) => return,
        };

        self.items = raw
            .todos
            .into_iter()
            .map(|t| TodoItem {
                id: t.id.unwrap_or_default(),
                content: t.content.unwrap_or_default(),
                status: match t.status.as_deref() {
                    Some("in_progress") => TodoStatus::InProgress,
                    Some("completed") => TodoStatus::Completed,
                    _ => TodoStatus::Pending,
                },
            })
            .collect();
    }

    /// Count of pending + in_progress items.
    #[allow(dead_code)]
    pub fn active_count(&self) -> usize {
        self.items
            .iter()
            .filter(|t| t.status != TodoStatus::Completed)
            .count()
    }

    /// Count of completed items.
    pub fn completed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count()
    }

    /// Returns a summary string like "3/5 tasks" or None if no tasks.
    pub fn summary(&self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let done = self.completed_count();
        let total = self.items.len();
        Some(format!("{done}/{total} tasks"))
    }

    /// Returns the currently in-progress task content, if any.
    #[allow(dead_code)]
    pub fn current_task(&self) -> Option<&str> {
        self.items
            .iter()
            .find(|t| t.status == TodoStatus::InProgress)
            .map(|t| t.content.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_todo_write() {
        let mut tracker = TodoTracker::new();
        let json = r#"{
            "todos": [
                {"id": "1", "content": "Read files", "status": "completed"},
                {"id": "2", "content": "Implement feature", "status": "in_progress"},
                {"id": "3", "content": "Write tests", "status": "pending"}
            ]
        }"#;
        tracker.apply_todo_write(json);
        assert_eq!(tracker.items.len(), 3);
        assert_eq!(tracker.completed_count(), 1);
        assert_eq!(tracker.active_count(), 2);
    }

    #[test]
    fn test_summary() {
        let mut tracker = TodoTracker::new();
        assert_eq!(tracker.summary(), None);

        let json = r#"{"todos": [
            {"id": "1", "content": "Task A", "status": "completed"},
            {"id": "2", "content": "Task B", "status": "pending"}
        ]}"#;
        tracker.apply_todo_write(json);
        assert_eq!(tracker.summary(), Some("1/2 tasks".to_string()));
    }

    #[test]
    fn test_current_task() {
        let mut tracker = TodoTracker::new();
        let json = r#"{"todos": [
            {"id": "1", "content": "Read files", "status": "completed"},
            {"id": "2", "content": "Implement feature", "status": "in_progress"}
        ]}"#;
        tracker.apply_todo_write(json);
        assert_eq!(tracker.current_task(), Some("Implement feature"));
    }

    #[test]
    fn test_replaces_previous_list() {
        let mut tracker = TodoTracker::new();
        tracker
            .apply_todo_write(r#"{"todos": [{"id": "1", "content": "Old", "status": "pending"}]}"#);
        assert_eq!(tracker.items.len(), 1);

        tracker.apply_todo_write(r#"{"todos": [{"id": "2", "content": "New A", "status": "pending"}, {"id": "3", "content": "New B", "status": "pending"}]}"#);
        assert_eq!(tracker.items.len(), 2);
        assert_eq!(tracker.items[0].content, "New A");
    }

    #[test]
    fn test_invalid_json_no_crash() {
        let mut tracker = TodoTracker::new();
        tracker.apply_todo_write("not json");
        assert!(tracker.items.is_empty());
    }
}
