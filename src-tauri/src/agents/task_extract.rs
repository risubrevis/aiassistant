use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::AgentEvent;
use crate::db::tasks::TaskInput;

/// Markdown checkbox line: `- [ ] …`, `* [x] …`, `- [~] …`, etc.
static CHECKBOX_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?m)^\s*[-*]\s+\[( |x|X|~|\-|\.)\]\s+(.+?)\s*$").expect("valid regex")
});

/// How an external agent's task tool mutates its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    FullReplace,
    Add,
    Update,
}

fn classify_tool(name: &str) -> Option<Action> {
    // Normalize casing/separators so "TodoWrite", "todo-write" and
    // "task_update" all hit the same arm.
    match name.to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "todo_write" | "update_todo_list" | "update_todo" => {
            Some(Action::FullReplace)
        }
        "taskcreate" | "task_create" => Some(Action::Add),
        "taskupdate" | "task_update" => Some(Action::Update),
        _ => None, // tasklist/taskget and unknown tools carry no task data
    }
}

/// Map an external agent's status to our canonical set.
fn normalize_status(status: &str) -> Option<&'static str> {
    match status.trim().to_lowercase().as_str() {
        "completed" | "done" | "complete" => Some("completed"),
        "in_progress" | "in-progress" | "in progress" | "running" => Some("in_progress"),
        "pending" | "todo" | "queued" | "planned" => Some("pending"),
        "cancelled" | "canceled" | "deleted" | "skipped" => Some("cancelled"),
        _ => None,
    }
}

fn status_or_pending(status: &str) -> &'static str {
    normalize_status(status).unwrap_or("pending")
}

fn first_string(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(serde_json::Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn task_input(v: &serde_json::Value, status: &str) -> Option<TaskInput> {
    let content = first_string(v, &["content", "subject"])?;
    if content.is_empty() {
        return None;
    }
    Some(TaskInput {
        content,
        active_form: first_string(v, &["activeForm", "active_form"]),
        status: status.to_string(),
    })
}

/// One agent run's accumulated task state.
#[derive(Default)]
pub struct RunTaskState {
    /// Current normalized list (flattened from `task_ids` on the Task* path).
    tasks: Vec<TaskInput>,
    /// True once a structured task tool was seen; disables the checkbox fallback.
    seen_structured: bool,
    /// Accumulated text for the markdown checkbox fallback.
    text_acc: String,
    /// Last list returned to the caller, for change detection.
    last_snapshot: Vec<TaskInput>,
    /// Task* tools address items by their own opaque id, which TaskInput does
    /// not carry; keep (their id, item) pairs and flatten into `tasks`.
    task_ids: Vec<(String, TaskInput)>,
    next_synthetic_id: usize,
}

impl RunTaskState {
    fn ingest_tool(&mut self, name: &str, args: &str) -> Option<Vec<TaskInput>> {
        match classify_tool(name)? {
            Action::FullReplace => self.ingest_full_replace(args)?,
            Action::Add => self.ingest_task_create(args)?,
            Action::Update => self.ingest_task_update(args)?,
        }
        self.seen_structured = true;
        self.diff_or_none()
    }

    fn ingest_full_replace(&mut self, args: &str) -> Option<()> {
        let parsed = serde_json::from_str::<serde_json::Value>(args).ok()?;
        let todos = parsed.get("todos")?.as_array()?;
        self.task_ids = todos
            .iter()
            .enumerate()
            .filter_map(|(i, todo)| {
                let status = todo
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .map(status_or_pending)
                    .unwrap_or("pending");
                task_input(todo, status).map(|item| (format!("t{i}"), item))
            })
            .collect();
        self.rebuild();
        Some(())
    }

    fn ingest_task_create(&mut self, args: &str) -> Option<()> {
        let v = serde_json::from_str::<serde_json::Value>(args).ok()?;
        let item = task_input(&v, "pending")?;
        let id = first_string(&v, &["id", "taskId", "task_id", "taskIdStr"])
            .unwrap_or_else(|| format!("t{}", self.next_synthetic_id));
        self.next_synthetic_id += 1;
        self.task_ids.push((id, item));
        self.rebuild();
        Some(())
    }

    fn ingest_task_update(&mut self, args: &str) -> Option<()> {
        let v = serde_json::from_str::<serde_json::Value>(args).ok()?;
        let id = first_string(&v, &["taskId", "id", "task_id", "taskIdStr"])?;
        let pos = self
            .task_ids
            .iter()
            .position(|(their_id, _)| *their_id == id)?;

        // "deleted" removes the item; any other status patches the matched one.
        let raw_status = v.get("status").and_then(serde_json::Value::as_str);
        if raw_status.is_some_and(|s| s.eq_ignore_ascii_case("deleted")) {
            self.task_ids.remove(pos);
            self.rebuild();
            return Some(());
        }

        let item = &mut self.task_ids[pos].1;
        if let Some(c) = first_string(&v, &["content", "subject"]) {
            item.content = c;
        }
        if let Some(a) = first_string(&v, &["activeForm", "active_form"]) {
            item.active_form = Some(a);
        }
        if let Some(s) = raw_status {
            item.status = status_or_pending(s).to_string();
        }
        self.rebuild();
        Some(())
    }

    fn ingest_text(&mut self, text: &str) -> Option<Vec<TaskInput>> {
        self.text_acc.push_str(text);
        if self.seen_structured {
            return None;
        }
        self.tasks = parse_checkboxes(&self.text_acc);
        if self.tasks.is_empty() {
            return None;
        }
        self.diff_or_none()
    }

    fn rebuild(&mut self) {
        self.tasks = self.task_ids.iter().map(|(_, item)| item.clone()).collect();
    }

    /// Return the new list only when it differs from the last emitted one.
    fn diff_or_none(&mut self) -> Option<Vec<TaskInput>> {
        if self.tasks == self.last_snapshot {
            None
        } else {
            self.last_snapshot = self.tasks.clone();
            Some(self.tasks.clone())
        }
    }
}

/// Maintains per-run task state and normalizes agent events into task lists.
#[derive(Default)]
pub struct TaskExtractor {
    states: Mutex<HashMap<String, RunTaskState>>,
}

impl TaskExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process an event for a run; returns `Some(list)` when the normalized
    /// list changed since the previous call, else `None`.
    pub fn ingest(&self, run_id: &str, event: &AgentEvent) -> Option<Vec<TaskInput>> {
        match event {
            AgentEvent::Text { text } => {
                let mut states = self.states.lock().unwrap();
                states
                    .entry(run_id.to_string())
                    .or_default()
                    .ingest_text(text)
            }
            AgentEvent::ToolAction { name, args } => {
                let mut states = self.states.lock().unwrap();
                states
                    .entry(run_id.to_string())
                    .or_default()
                    .ingest_tool(name, args)
            }
            // Diff/Progress/Error carry no task information.
            _ => None,
        }
    }

    /// Drop the run's state and return its final list, if any.
    pub fn finalize(&self, run_id: &str) -> Option<Vec<TaskInput>> {
        self.states
            .lock()
            .unwrap()
            .remove(run_id)
            .map(|s| s.tasks)
            .filter(|tasks| !tasks.is_empty())
    }
}

/// Parse markdown checkboxes, skipping fenced code blocks.
fn parse_checkboxes(text: &str) -> Vec<TaskInput> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(m) = CHECKBOX_RE.captures(line) {
            let status = match &m[1] {
                "x" | "X" => "completed",
                " " => "pending",
                _ => "in_progress",
            };
            out.push(TaskInput {
                content: m[2].to_string(),
                active_form: None,
                status: status.into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, args: &str) -> AgentEvent {
        AgentEvent::ToolAction {
            name: name.into(),
            args: args.into(),
        }
    }

    #[test]
    fn full_replace_parses_claude_code_todo_write() {
        let ex = TaskExtractor::new();
        let args = r#"{"todos":[{"content":"Reproduce","status":"completed"},{"content":"Fix bug","status":"in_progress","activeForm":"Fixing bug"}]}"#;
        let items = ex.ingest("r1", &tool("TodoWrite", args)).expect("changed");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].status, "completed");
        assert_eq!(items[1].active_form.as_deref(), Some("Fixing bug"));

        // Same list again → no change; finalize returns the list once.
        assert!(ex.ingest("r1", &tool("TodoWrite", args)).is_none());
        assert_eq!(ex.finalize("r1").unwrap().len(), 2);
        assert!(ex.finalize("r1").is_none());
    }

    #[test]
    fn task_create_and_update_by_their_id() {
        let ex = TaskExtractor::new();
        let items = ex
            .ingest(
                "r2",
                &tool("TaskCreate", r#"{"subject":"Wire UI","id":"t-9"}"#),
            )
            .expect("created");
        assert_eq!(items[0].content, "Wire UI");
        assert_eq!(items[0].status, "pending");

        let items = ex
            .ingest(
                "r2",
                &tool("TaskUpdate", r#"{"taskId":"t-9","status":"completed"}"#),
            )
            .expect("updated");
        assert_eq!(items[0].status, "completed");

        let items = ex
            .ingest(
                "r2",
                &tool("task_update", r#"{"taskId":"t-9","status":"deleted"}"#),
            )
            .expect("removed");
        assert!(items.is_empty());
    }

    #[test]
    fn checkbox_fallback_and_fence_skipping() {
        let ex = TaskExtractor::new();
        let first = ex
            .ingest(
                "r3",
                &AgentEvent::Text {
                    text: "Intro\n- [ ] One\n- [x] Two\n".into(),
                },
            )
            .expect("checkboxes found");
        assert_eq!(first.len(), 2);
        assert_eq!(first[1].status, "completed");

        let items = ex
            .ingest(
                "r3",
                &AgentEvent::Text {
                    text: "- [~] Three\n```\n- [ ] not a task\n```\n".into(),
                },
            )
            .expect("third added");
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].status, "in_progress");
    }

    #[test]
    fn status_normalization() {
        assert_eq!(normalize_status("DONE"), Some("completed"));
        assert_eq!(normalize_status("in-progress"), Some("in_progress"));
        assert_eq!(normalize_status("In Progress"), Some("in_progress"));
        assert_eq!(normalize_status("queued"), Some("pending"));
        assert_eq!(normalize_status("Canceled"), Some("cancelled"));
        assert_eq!(normalize_status("weird"), None);
    }
}
