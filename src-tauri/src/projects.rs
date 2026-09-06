use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use notify::{recommended_watcher, EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::warn;

use crate::db::models::{self, Chat, Project, ProjectPath};

/// Per-project file watcher manager. Spawns one notify watcher per watched
/// path; emits `project:file_changed` events to the UI (ContextPanel, docs/08).
#[derive(Clone, Default)]
pub struct ProjectWatcher {
    handles: Arc<Mutex<HashMap<String, notify::RecommendedWatcher>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChangedEvent {
    pub project_id: String,
    pub path: String,
    pub kind: String,
}

impl ProjectWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)start watchers for all watched paths of a project.
    pub fn restart_for_project(
        &self,
        app: AppHandle,
        pool: SqlitePool,
        changes: ChangeTracker,
        project_id: String,
    ) {
        let pid = project_id.clone();
        let app2 = app.clone();
        let self_c = self.clone();
        tauri::async_runtime::spawn(async move {
            let paths = match models::list_project_paths(&pool, &pid).await {
                Ok(p) => p,
                Err(e) => {
                    warn!("list project paths failed: {e}");
                    return;
                }
            };
            let watched: Vec<ProjectPath> = paths.into_iter().filter(|p| p.watch == 1).collect();
            // Drop previous watchers for this project.
            if let Ok(mut h) = self_c.handles.lock() {
                let keys: Vec<String> = h
                    .keys()
                    .filter(|k| k.starts_with(&format!("{pid}:")))
                    .cloned()
                    .collect();
                for k in keys {
                    h.remove(&k);
                }
            }
            for wp in watched {
                let key = format!("{pid}:{}", wp.id);
                let (tx, mut rx) = mpsc::channel::<notify::Result<notify::Event>>(32);
                let mut watcher = match recommended_watcher(move |res| {
                    let _ = tx.blocking_send(res);
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        warn!("watcher init failed for {}: {e}", wp.path);
                        continue;
                    }
                };
                if let Err(e) =
                    watcher.watch(PathBuf::from(&wp.path).as_path(), RecursiveMode::Recursive)
                {
                    warn!("watch failed for {}: {e}", wp.path);
                    continue;
                }
                if let Ok(mut h) = self_c.handles.lock() {
                    h.insert(key, watcher);
                }
                let app3 = app2.clone();
                let pid3 = pid.clone();
                let wp_root = wp.path.clone();
                let changes3 = changes.clone();
                tauri::async_runtime::spawn(async move {
                    while let Some(res) = rx.recv().await {
                        match res {
                            Ok(ev) => {
                                if matches!(ev.kind, EventKind::Access(_)) {
                                    continue;
                                }
                                for p in ev.paths {
                                    let rel = p
                                        .strip_prefix(&wp_root)
                                        .unwrap_or(&p)
                                        .display()
                                        .to_string();
                                    let kind = match ev.kind {
                                        EventKind::Create(_) => "created",
                                        EventKind::Modify(_) => "modified",
                                        EventKind::Remove(_) => "removed",
                                        _ => "changed",
                                    };
                                    changes3.record(&pid3, &p.display().to_string(), kind);
                                    let _ = app3.emit(
                                        "project:file_changed",
                                        FileChangedEvent {
                                            project_id: pid3.clone(),
                                            path: rel,
                                            kind: kind.into(),
                                        },
                                    );
                                }
                            }
                            Err(e) => warn!("watch event error: {e}"),
                        }
                    }
                });
            }
        });
    }

    /// Stop watchers for a project (e.g. on delete).
    pub fn stop_project(&self, project_id: &str) {
        if let Ok(mut h) = self.handles.lock() {
            let prefix = format!("{project_id}:");
            let keys: Vec<String> = h
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            for k in keys {
                h.remove(&k);
            }
        }
    }
}

/// One tracked change for a project file (absolute path, notify event kind).
#[derive(Debug, Clone, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub kind: String,
    pub ts: i64,
}

/// Max tracked changes kept per project; eviction is oldest-by-ts.
const MAX_TRACKED_CHANGES: usize = 500;

/// Per-project record of file changes fed by the notify watcher. Source for
/// auto-pulling changed files into chat context (docs/08).
#[derive(Clone, Default)]
pub struct ChangeTracker {
    map: Arc<Mutex<HashMap<String, Vec<ChangedFile>>>>,
}

impl ChangeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a change; dedup by path (update kind+ts in place), cap per
    /// project at 500 entries (evict oldest by ts).
    pub fn record(&self, project_id: &str, path: &str, kind: &str) {
        let ts = chrono::Utc::now().timestamp_millis();
        let mut map = self.map.lock().unwrap();
        let list = map.entry(project_id.to_string()).or_default();
        match list.iter_mut().find(|c| c.path == path) {
            Some(existing) => {
                existing.kind = kind.to_string();
                existing.ts = ts;
            }
            None => list.push(ChangedFile {
                path: path.to_string(),
                kind: kind.to_string(),
                ts,
            }),
        }
        if list.len() > MAX_TRACKED_CHANGES {
            list.sort_by_key(|c| c.ts);
            let excess = list.len() - MAX_TRACKED_CHANGES;
            list.drain(..excess);
        }
    }

    /// Snapshot without clearing (for on-request UI).
    pub fn snapshot(&self, project_id: &str) -> Vec<ChangedFile> {
        self.map
            .lock()
            .unwrap()
            .get(project_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Take and clear (used at turn start to inject only new-since-last-turn).
    pub fn drain(&self, project_id: &str) -> Vec<ChangedFile> {
        self.map
            .lock()
            .unwrap()
            .remove(project_id)
            .unwrap_or_default()
    }

    pub fn clear(&self, project_id: &str) {
        self.map.lock().unwrap().remove(project_id);
    }
}

/// Resolve a relative path against project + chat paths (docs/08 @file/@dir).
/// Returns the first matching absolute path, or None.
pub async fn resolve_rel(
    pool: &SqlitePool,
    project_id: Option<&str>,
    chat_id: &str,
    rel: &str,
) -> Option<PathBuf> {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        return Some(p);
    }
    // chat_paths first (more specific), then project_paths.
    if let Ok(chat_paths) = models::list_chat_paths(pool, chat_id).await {
        for cp in chat_paths {
            let root = PathBuf::from(&cp.path);
            let candidate = if root.is_dir() {
                root.join(&p)
            } else {
                root.clone()
            };
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    if let Some(pid) = project_id {
        if let Ok(proj_paths) = models::list_project_paths(pool, pid).await {
            for pp in proj_paths {
                let root = PathBuf::from(&pp.path);
                let candidate = if root.is_dir() {
                    root.join(&p)
                } else {
                    root.clone()
                };
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Collect allowed roots (project_paths + chat_paths) for path security.
pub async fn allowed_roots(
    pool: &SqlitePool,
    project_id: Option<&str>,
    chat_id: &str,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(pid) = project_id {
        if let Ok(pp) = models::list_project_paths(pool, pid).await {
            roots.extend(pp.iter().map(|p| PathBuf::from(&p.path)));
        }
    }
    if let Ok(cp) = models::list_chat_paths(pool, chat_id).await {
        roots.extend(cp.iter().map(|p| PathBuf::from(&p.path)));
    }
    roots
}

/// Expand `@file`/`@dir` tokens in user text by inlining file contents / dir
/// listings (docs/08). Unknown tokens are left as-is.
pub async fn expand_refs(
    pool: &SqlitePool,
    project_id: Option<&str>,
    chat_id: &str,
    text: &str,
) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@file ") {
            let rel = rest.trim();
            if let Some(abs) = resolve_rel(pool, project_id, chat_id, rel).await {
                match tokio::fs::read_to_string(&abs).await {
                    Ok(mut s) => {
                        let max = 32 * 1024;
                        if s.len() > max {
                            s.truncate(max);
                            s.push_str("\n…[truncated]");
                        }
                        out.push_str(&format!("`@file {rel}`:\n```\n{s}\n```\n"));
                        continue;
                    }
                    Err(e) => {
                        out.push_str(&format!("`@file {rel}`: (read failed: {e})\n"));
                        continue;
                    }
                }
            } else {
                out.push_str(&format!(
                    "`@file {rel}`: (not found in project/chat paths)\n"
                ));
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("@dir ") {
            let rel = rest.trim();
            if let Some(abs) = resolve_rel(pool, project_id, chat_id, rel).await {
                match list_tree(&abs, 200).await {
                    Ok(tree) => {
                        out.push_str(&format!("`@dir {rel}`:\n```\n{tree}\n```\n"));
                        continue;
                    }
                    Err(e) => {
                        out.push_str(&format!("`@dir {rel}`: (list failed: {e})\n"));
                        continue;
                    }
                }
            } else {
                out.push_str(&format!("`@dir {rel}`: (not found)\n"));
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // Remove trailing newline added by per-line join, keep parity with input.
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

async fn list_tree(root: &std::path::Path, cap: usize) -> anyhow::Result<String> {
    let mut out = String::new();
    let mut count = 0usize;
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 3 || count >= cap {
            continue;
        }
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        let mut items: Vec<(String, PathBuf, bool)> = Vec::new();
        while let Ok(Some(e)) = entries.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git"
            {
                continue;
            }
            let is_dir = e.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            items.push((name, e.path(), is_dir));
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path, is_dir) in items.into_iter().rev() {
            if count >= cap {
                out.push_str("…\n");
                break;
            }
            let indent = "  ".repeat(depth);
            out.push_str(&format!(
                "{indent}{}{}\n",
                name,
                if is_dir { "/" } else { "" }
            ));
            count += 1;
            if is_dir {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(out)
}

/// Load a project (or None) for a chat.
pub async fn project_for_chat(pool: &SqlitePool, chat: &Chat) -> Option<Project> {
    let pid = chat.project_id.as_ref()?;
    models::get_project(pool, pid).await.ok().flatten()
}

const PER_FILE_CAP: usize = 16_384;
const TOTAL_SECTION_CAP: usize = 65_536;

/// All project path roots (potential prefixes for resolving absolute paths).
async fn project_roots(pool: &SqlitePool, project: &Project) -> Vec<PathBuf> {
    models::list_project_paths(pool, &project.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|pp| PathBuf::from(pp.path))
        .collect()
}

/// Resolve an absolute path relative to the first matching project root,
/// falling back to the full absolute path.
fn project_rel(roots: &[PathBuf], abs: &std::path::Path) -> String {
    for root in roots {
        if let Ok(rel) = abs.strip_prefix(root) {
            return rel.display().to_string();
        }
    }
    abs.display().to_string()
}

/// Truncate to `cap` bytes on the nearest char boundary.
fn truncate_char_safe(text: String, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text, false);
    }
    let mut cut = cap;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_string(), true)
}

async fn is_dir(path: &std::path::Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

/// Build a context section listing changed files with their current contents
/// (capped), for injection into the system prompt at turn start. Returns None
/// when there is nothing to inject.
pub async fn changed_files_section(
    pool: &SqlitePool,
    project: &Project,
    changes: Vec<ChangedFile>,
) -> Option<String> {
    if changes.is_empty() {
        return None;
    }
    let roots = project_roots(pool, project).await;
    let mut section = String::from("Recently changed project files (since the last turn):\n");
    let mut total = 0usize;
    let mut added = 0usize;
    let mut more = 0usize;
    for cf in &changes {
        let abs = PathBuf::from(&cf.path);
        if is_dir(&abs).await {
            continue;
        }
        if total >= TOTAL_SECTION_CAP {
            more += 1;
            continue;
        }
        let rel = project_rel(&roots, &abs);
        let exists = tokio::fs::metadata(&abs).await.is_ok();
        let block = if cf.kind == "removed" || !exists {
            format!("--- {rel} (removed) ---\n(removed)\n")
        } else {
            let Ok(bytes) = tokio::fs::read(&abs).await else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let (text, truncated) = truncate_char_safe(text, PER_FILE_CAP);
            let marker = if truncated {
                "\n\u{2026}[truncated]"
            } else {
                ""
            };
            format!("--- {rel} ({}) ---\n{text}{marker}\n", cf.kind)
        };
        total += block.len();
        section.push_str(&block);
        added += 1;
    }
    if more > 0 {
        section.push_str(&format!("\u{2026} ({more} more files changed)\n"));
    }
    (added > 0).then_some(section)
}

/// View of one tracked change for the on-request UI (project_changed_files).
#[derive(Debug, Clone, Serialize)]
pub struct ChangedFileView {
    pub path: String,
    pub rel: String,
    pub kind: String,
    pub content: Option<String>,
    pub truncated: bool,
}

/// Materialize tracked changes into per-file views (no aggregate cap, per-file
/// cap 16 KiB). Directories are skipped; removed/binary files get `content: None`.
pub async fn changed_file_views(
    pool: &SqlitePool,
    project: &Project,
    changes: Vec<ChangedFile>,
) -> Vec<ChangedFileView> {
    let roots = project_roots(pool, project).await;
    let mut out = Vec::new();
    for cf in changes {
        let abs = PathBuf::from(&cf.path);
        if is_dir(&abs).await {
            continue;
        }
        let rel = project_rel(&roots, &abs);
        let (content, truncated) = if cf.kind == "removed" || !abs.exists() {
            (None, false)
        } else {
            match tokio::fs::read(&abs).await {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => {
                        let (content, truncated) = truncate_char_safe(text, PER_FILE_CAP);
                        (Some(content), truncated)
                    }
                    Err(_) => (None, false),
                },
                Err(_) => (None, false),
            }
        };
        out.push(ChangedFileView {
            path: cf.path.clone(),
            rel,
            kind: cf.kind.clone(),
            content,
            truncated,
        });
    }
    out
}

/// Filenames searched for auto-import as project rules when a folder is attached.
pub const RULE_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".cursorrules",
    ".windsurfrules",
    "cursor.rules",
    ".github/copilot-instructions.md",
];

/// Scan `dir` (non-recursively, except `.github/`) for known rule files.
/// Returns (marker, filename, full_path, content) for each found readable file.
pub fn detect_rule_files(
    dir: &std::path::Path,
) -> Vec<(&'static str, String, std::path::PathBuf, String)> {
    let mut out = Vec::new();
    for name in RULE_FILES {
        let p = dir.join(name);
        if p.is_file() {
            if let Ok(content) = std::fs::read_to_string(&p) {
                if !content.trim().is_empty() {
                    out.push((*name, name.to_string(), p, content));
                }
            }
        }
    }
    out
}

/// A skill discovered from a project's `.skills/` folder (docs/IDEAS: auto-connect skills).
/// Each `*.md` file is one skill. id = filename stem; title = first `# ` heading or the stem.
/// Consumed by chat.rs (skill discovery per turn).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProjectSkill {
    pub id: String,
    pub title: String,
    pub body: String,
}

/// Scan `<root>/.skills/*.md` for each root (non-recursive within `.skills`).
/// Deduplicates by id (first occurrence wins). Skips empty files.
#[allow(dead_code)]
pub fn discover_project_skills(roots: &[std::path::PathBuf]) -> Vec<ProjectSkill> {
    let mut out: Vec<ProjectSkill> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for root in roots {
        let skills_dir = root.join(".skills");
        let entries = match std::fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let id = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            let body = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if body.trim().is_empty() {
                continue;
            }
            let title = body
                .lines()
                .find_map(|l| {
                    let t = l.trim_start();
                    t.strip_prefix("# ").map(|s| s.trim().to_string())
                })
                .unwrap_or_else(|| id.clone());
            out.push(ProjectSkill { id, title, body });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedups_by_path_updating_in_place() {
        let t = ChangeTracker::new();
        t.record("p1", "/a/b.rs", "created");
        t.record("p1", "/a/c.rs", "modified");
        t.record("p1", "/a/b.rs", "modified");
        let snap = t.snapshot("p1");
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].path, "/a/b.rs");
        assert_eq!(snap[0].kind, "modified");
        assert!(snap[0].ts > 0);
    }

    #[test]
    fn record_caps_per_project_evicting_oldest() {
        let t = ChangeTracker::new();
        for i in 0..(MAX_TRACKED_CHANGES + 100) {
            t.record("p1", &format!("/f{i}.rs"), "created");
        }
        let snap = t.snapshot("p1");
        assert_eq!(snap.len(), MAX_TRACKED_CHANGES);
        assert!(!snap.iter().any(|c| c.path == "/f0.rs"));
        assert!(!snap.iter().any(|c| c.path == "/f99.rs"));
        assert!(snap.iter().any(|c| c.path == "/f599.rs"));
    }

    #[test]
    fn snapshot_does_not_clear() {
        let t = ChangeTracker::new();
        t.record("p1", "/a.rs", "created");
        t.record("p2", "/b.rs", "created");
        assert_eq!(t.snapshot("p1").len(), 1);
        assert_eq!(t.snapshot("p1").len(), 1);
        assert_eq!(t.snapshot("p2").len(), 1);
    }

    #[test]
    fn drain_takes_and_clears() {
        let t = ChangeTracker::new();
        t.record("p1", "/a.rs", "created");
        let drained = t.drain("p1");
        assert_eq!(drained.len(), 1);
        assert!(t.drain("p1").is_empty());
        t.record("p1", "/b.rs", "modified");
        t.clear("p1");
        assert!(t.snapshot("p1").is_empty());
    }
}
