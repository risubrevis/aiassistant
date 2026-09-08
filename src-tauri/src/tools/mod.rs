pub mod builtin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

/// Tool category drives the permission gate (docs/12, docs/17).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Readonly,
    Write,
    Exec,
    Destructive,
    Network,
    Interaction,
}

/// JSON-schema-ish spec of a tool, as exposed to the LLM (OpenAI `tools` shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }
    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn category(&self) -> ToolCategory;
    fn spec(&self) -> ToolSpec;
    async fn execute(&self, args: Value) -> ToolResult;
}

/// Registry of tools visible to the LLM in a chat turn (builtin + MCP, later).
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Build the builtin tool set for a mode (docs/12):
    /// minimal -> none, plan -> readonly + interaction, write -> all.
    /// `in_project` adds cross-chat retrieval tools (search_project_chats, read_chat),
    /// available only inside project chats (docs/08).
    /// Cross-chat tools are gated by the project `cross_chat` setting: only "off"
    /// disables them (summary/retrieval/hybrid keep tool access).
    #[allow(dead_code)]
    pub fn builtin_for_mode(mode: &str, cross_chat: &str) -> Self {
        Self::builtin_for_mode_ctx(mode, false, cross_chat)
    }

    pub fn builtin_for_mode_ctx(mode: &str, in_project: bool, cross_chat: &str) -> Self {
        let mut r = Self::new();
        use builtin::{
            AskUser, FileInfo, Glob, Grep, ListDir, ReadChat, ReadFile, SearchProjectChats,
            TodoWrite, WebFetch, WebSearch,
        };
        match mode {
            "minimal" => {}
            "plan" => {
                r.register(Box::new(ReadFile));
                r.register(Box::new(ListDir));
                r.register(Box::new(Glob));
                r.register(Box::new(Grep));
                r.register(Box::new(FileInfo));
                r.register(Box::new(WebFetch));
                r.register(Box::new(WebSearch));
                r.register(Box::new(AskUser));
                r.register(Box::new(TodoWrite));
                if in_project && cross_chat != "off" {
                    r.register(Box::new(SearchProjectChats));
                    r.register(Box::new(ReadChat));
                }
            }
            _ => {
                // write
                use builtin::{
                    ApplyPatch, DeletePath, EditFile, MakeDir, MovePath, RunCommand, SetFileMode,
                    WriteFile,
                };
                r.register(Box::new(ReadFile));
                r.register(Box::new(ListDir));
                r.register(Box::new(Glob));
                r.register(Box::new(Grep));
                r.register(Box::new(FileInfo));
                r.register(Box::new(WebFetch));
                r.register(Box::new(WebSearch));
                r.register(Box::new(WriteFile));
                r.register(Box::new(EditFile));
                r.register(Box::new(ApplyPatch));
                r.register(Box::new(MakeDir));
                r.register(Box::new(MovePath));
                r.register(Box::new(SetFileMode));
                r.register(Box::new(DeletePath));
                r.register(Box::new(RunCommand));
                r.register(Box::new(AskUser));
                r.register(Box::new(TodoWrite));
                if in_project && cross_chat != "off" {
                    r.register(Box::new(SearchProjectChats));
                    r.register(Box::new(ReadChat));
                }
            }
        }
        r
    }

    /// Remove builtin tools whose name is in `disabled` (per-user toggle in Settings→Tools).
    pub fn remove_disabled(&mut self, disabled: &[String]) {
        self.tools.retain(|t| !disabled.contains(&t.spec().name));
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// OpenAI `tools` array (`[{type:"function", function:{...}}]`).
    pub fn openai_tools(&self) -> Value {
        Value::Array(
            self.tools
                .iter()
                .map(|t| {
                    let spec = t.spec();
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": spec.name,
                            "description": spec.description,
                            "parameters": spec.parameters,
                        }
                    })
                })
                .collect(),
        )
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.spec().name).collect()
    }

    pub fn category_of(&self, name: &str) -> Option<ToolCategory> {
        self.tools
            .iter()
            .find(|t| t.spec().name == name)
            .map(|t| t.category())
    }

    pub async fn call(&self, name: &str, args: Value) -> Option<ToolResult> {
        for t in &self.tools {
            if t.spec().name == name {
                return Some(t.execute(args).await);
            }
        }
        Some(ToolResult::err(format!("unknown tool: {name}")))
    }
}

/// All builtin tools (name, category, description) for Settings → Tools.
pub fn builtin_all_specs() -> Vec<(String, String, String)> {
    let mut r = Registry::new();
    use builtin::{
        ApplyPatch, AskUser, DeletePath, EditFile, FileInfo, Glob, Grep, ListDir, MakeDir,
        MovePath, ReadChat, ReadFile, RunCommand, SearchProjectChats, SetFileMode, TodoWrite,
        WebFetch, WebHookAdd, WebHookDelete, WebHookList, WebHookModify, WebHookRun, WebSearch,
        WriteFile,
    };
    r.register(Box::new(ReadFile));
    r.register(Box::new(ListDir));
    r.register(Box::new(Glob));
    r.register(Box::new(Grep));
    r.register(Box::new(FileInfo));
    r.register(Box::new(WriteFile));
    r.register(Box::new(EditFile));
    r.register(Box::new(ApplyPatch));
    r.register(Box::new(MakeDir));
    r.register(Box::new(MovePath));
    r.register(Box::new(SetFileMode));
    r.register(Box::new(DeletePath));
    r.register(Box::new(RunCommand));
    r.register(Box::new(WebFetch));
    r.register(Box::new(WebSearch));
    r.register(Box::new(AskUser));
    r.register(Box::new(TodoWrite));
    r.register(Box::new(SearchProjectChats));
    r.register(Box::new(ReadChat));
    r.register(Box::new(WebHookList));
    r.register(Box::new(WebHookRun));
    r.register(Box::new(WebHookAdd));
    r.register(Box::new(WebHookModify));
    r.register(Box::new(WebHookDelete));
    r.tools
        .iter()
        .map(|t| {
            let s = t.spec();
            (
                s.name,
                format!("{:?}", t.category()).to_lowercase(),
                s.description,
            )
        })
        .collect()
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Path security (docs/12/17). Resolve to absolute; reject `..`-traversal,
/// symlink escapes outside the allowed roots, and a denylist of sensitive
/// paths. When project roots are set, paths must stay within them.
pub fn validate_read_path(path: &str) -> Result<std::path::PathBuf, String> {
    validate_path(path, &PATH_ROOTS.read().unwrap())
}

pub fn validate_write_path(path: &str) -> Result<std::path::PathBuf, String> {
    validate_path(path, &PATH_ROOTS.read().unwrap())
}

/// Like `validate_write_path` but for unlink semantics: the returned path keeps
/// the final component unresolved, so a symlink is removed itself, not its
/// target. The directory entry (leaf) must always be inside the allowed roots.
/// A symlink whose target escapes the roots is rejected unless `allow_escape`
/// is true (granted by an explicit symlink-escape approval).
pub(crate) fn validate_unlink_path(
    path: &str,
    allow_escape: bool,
) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(path);
    let roots = PATH_ROOTS.read().unwrap();
    let abs = if p.is_absolute() {
        p
    } else if let Some(r) = roots.first() {
        r.join(&p)
    } else {
        std::env::current_dir().unwrap_or_default().join(&p)
    };
    let out = leaf_unresolved(&abs);
    // The directory entry itself must be inside the sandbox.
    if !roots.is_empty()
        && !roots.iter().any(|root| {
            let rc = std::fs::canonicalize(root).unwrap_or_else(|_| lexical_canonicalize(root));
            out.starts_with(rc)
        })
    {
        return Err(format!(
            "access denied: path outside project roots: {}",
            out.display()
        ));
    }
    if unlink_denylisted(&out) {
        return Err("access denied: sensitive path".into());
    }
    // For a symlink, decide based on its target.
    if std::fs::symlink_metadata(&out)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        let target = std::fs::read_link(&out).unwrap_or_default();
        let parent = out.parent().unwrap_or_else(|| std::path::Path::new(""));
        let abs_target = if target.is_absolute() {
            target
        } else {
            parent.join(target)
        };
        let canon_target = std::fs::canonicalize(&abs_target)
            .unwrap_or_else(|_| lexical_canonicalize(&abs_target));
        let target_inside = roots.is_empty()
            || roots.iter().any(|root| {
                let rc = std::fs::canonicalize(root).unwrap_or_else(|_| lexical_canonicalize(root));
                canon_target.starts_with(rc)
            });
        if !target_inside {
            if allow_escape {
                return Ok(out);
            }
            return Err(format!(
                "symlink target outside project roots: {} -> {} (requires explicit approval)",
                out.display(),
                canon_target.display()
            ));
        }
        if unlink_denylisted(&canon_target) {
            return Err("access denied: sensitive path".into());
        }
        return Ok(out);
    }
    Ok(out)
}

/// Resolve `abs` to an absolute path with the final component left
/// unresolved (a symlink is returned as itself, not its target).
fn leaf_unresolved(abs: &std::path::Path) -> std::path::PathBuf {
    let (leaf, parent) = match (abs.file_name(), abs.parent()) {
        (Some(l), Some(p)) => (l, p),
        _ => return lexical_canonicalize(abs),
    };
    let parent_canon =
        std::fs::canonicalize(parent).unwrap_or_else(|_| lexical_canonicalize(parent));
    parent_canon.join(leaf)
}

/// Denylist used by unlink semantics (same rules as `validate_path`).
fn unlink_denylisted(canon: &std::path::Path) -> bool {
    let s = canon.to_string_lossy().replace('\\', "/").to_lowercase();
    let b = canon
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    b == ".env" || b.starts_with(".env.") || s.contains("/.ssh/") || s.contains("/secrets/")
}

/// Display-only path info for the approval prompt: the absolute path to show,
/// the resolved target if it is a symlink, and whether that target escapes the
/// allowed roots. Does NOT enforce scope/denylist — that is `validate_*_path`'s
/// responsibility; this only enriches the approval card shown to the user.
#[derive(Debug, Clone)]
pub(crate) struct ApprovalPathInfo {
    /// Absolute path to display. For unlink of a symlink: the link itself.
    /// Otherwise: the canonicalized target.
    pub display: std::path::PathBuf,
    /// If `display` is a symlink (unlink case), its resolved target.
    pub symlink_target: Option<std::path::PathBuf>,
    /// True when a symlink target resolves outside the allowed roots.
    pub escapes: bool,
}

pub(crate) fn approval_path(path: &str, unlink: bool) -> ApprovalPathInfo {
    let p = std::path::PathBuf::from(path);
    let roots = PATH_ROOTS.read().unwrap();
    let abs = if p.is_absolute() {
        p
    } else if let Some(r) = roots.first() {
        r.join(&p)
    } else {
        std::env::current_dir().unwrap_or_default().join(&p)
    };
    let leaf_abs = leaf_unresolved(&abs);
    let is_symlink = std::fs::symlink_metadata(&leaf_abs)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if is_symlink {
        let target = std::fs::read_link(&leaf_abs).unwrap_or_default();
        let parent = leaf_abs
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let abs_target = if target.is_absolute() {
            target
        } else {
            parent.join(target)
        };
        let canon_target = std::fs::canonicalize(&abs_target)
            .unwrap_or_else(|_| lexical_canonicalize(&abs_target));
        let escapes = !roots.is_empty()
            && !roots.iter().any(|root| {
                let rc = std::fs::canonicalize(root).unwrap_or_else(|_| lexical_canonicalize(root));
                canon_target.starts_with(rc)
            });
        let display = if unlink {
            leaf_abs
        } else {
            canon_target.clone()
        };
        ApprovalPathInfo {
            display,
            symlink_target: if unlink { Some(canon_target) } else { None },
            escapes,
        }
    } else {
        let display = std::fs::canonicalize(&leaf_abs).unwrap_or(leaf_abs);
        ApprovalPathInfo {
            display,
            symlink_target: None,
            escapes: false,
        }
    }
}

/// Char-boundary-safe truncation: never splits a multi-byte UTF-8 sequence.
pub(crate) fn truncate_text(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push_str("\n…[truncated]");
}

/// Parse the optional `timeout_ms` tool arg, tolerating int or float JSON
/// numbers. Default 120_000ms, clamped to [1_000, 600_000].
pub(crate) fn parse_timeout_ms(args: &serde_json::Value) -> u64 {
    let raw = args.get("timeout_ms");
    let v = raw
        .and_then(|v| v.as_u64())
        .or_else(|| raw.and_then(|v| v.as_f64()).map(|f| f as u64));
    v.unwrap_or(120_000).clamp(1_000, 600_000)
}

/// Allowed roots for file tools (project_paths + chat_paths + config allowed_paths).
/// Set per turn from chat.rs. Empty = unrestricted (legacy/MVP behavior).
static PATH_ROOTS: std::sync::LazyLock<std::sync::RwLock<Vec<std::path::PathBuf>>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(Vec::new()));

pub fn set_path_roots(roots: Vec<std::path::PathBuf>) {
    if let Ok(mut w) = PATH_ROOTS.write() {
        *w = roots;
    }
}

pub fn clear_path_roots() {
    if let Ok(mut w) = PATH_ROOTS.write() {
        w.clear();
    }
}

static DB_POOL: std::sync::OnceLock<SqlitePool> = std::sync::OnceLock::new();

/// Install the app's SQLite pool so built-in tools can read config tables.
pub fn set_db_pool(pool: SqlitePool) {
    let _ = DB_POOL.set(pool);
}

pub(crate) fn db_pool() -> Option<&'static SqlitePool> {
    DB_POOL.get()
}

/// Whether `delete_path` moves to the OS trash/recycle bin instead of permanent
/// removal. Set per turn from chat.rs (mirrors `PATH_ROOTS`). Default: permanent.
static TRASH_MODE: std::sync::LazyLock<std::sync::atomic::AtomicBool> =
    std::sync::LazyLock::new(|| std::sync::atomic::AtomicBool::new(false));

pub fn set_trash_mode(enabled: bool) {
    TRASH_MODE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn trash_mode() -> bool {
    TRASH_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// True if `abs` (already canonicalized, e.g. returned by `validate_write_path`)
/// is exactly one of the currently allowed roots. Destructive tools use this to
/// refuse wiping a project root itself.
pub(crate) fn is_protected_root(abs: &std::path::Path) -> bool {
    let roots = PATH_ROOTS.read().unwrap();
    roots.iter().any(|root| {
        let rc = std::fs::canonicalize(root).unwrap_or_else(|_| lexical_canonicalize(root));
        abs == rc
    })
}

fn validate_path(path: &str, roots: &[std::path::PathBuf]) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(path);
    let abs = if p.is_absolute() {
        p
    } else if let Some(root) = roots.first() {
        root.join(&p)
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    // Canonicalize the existing prefix and lexically normalize the rest, so
    // `..`-traversal is caught even for paths that do not yet exist (make_dir,
    // write_file targets). See `lexical_canonicalize`.
    let canon = lexical_canonicalize(&abs);
    if !roots.is_empty() {
        let inside = roots.iter().any(|root| {
            let rc = std::fs::canonicalize(root).unwrap_or_else(|_| lexical_canonicalize(root));
            canon.starts_with(rc)
        });
        if !inside {
            return Err(format!(
                "access denied: path outside project roots: {}",
                canon.display()
            ));
        }
    }
    let s = canon.to_string_lossy().replace('\\', "/").to_lowercase();
    let b = canon
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if b == ".env" || b.starts_with(".env.") {
        return Err("access denied: .env".into());
    }
    if s.contains("/.ssh/") || s.contains("/secrets/") {
        return Err("access denied: sensitive path".into());
    }
    Ok(canon)
}

/// Resolve `abs` to a canonical absolute path without requiring the full path
/// to exist. The longest existing ancestor is canonicalized via the OS
/// (resolving symlinks); the remaining non-existent tail is normalized
/// lexically — `.` is dropped and `..` pops a component (clamping at the
/// filesystem root). This prevents `..`-traversal from escaping the sandbox
/// for not-yet-created paths: the scope check in `validate_path` then rejects
/// any result outside the allowed roots.
fn lexical_canonicalize(abs: &std::path::Path) -> std::path::PathBuf {
    let mut anchor = abs.to_path_buf();
    while !anchor.as_os_str().is_empty() && std::fs::symlink_metadata(&anchor).is_err() {
        match anchor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => anchor = parent.to_path_buf(),
            _ => break,
        }
    }
    let canon_anchor = std::fs::canonicalize(&anchor).unwrap_or_else(|_| anchor.clone());
    let tail = abs
        .strip_prefix(&anchor)
        .unwrap_or_else(|_| std::path::Path::new(""));
    let mut out = canon_anchor;
    for c in tail.components() {
        use std::path::Component::*;
        match c {
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            Normal(s) => out.push(s),
            RootDir | Prefix(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ia-vp-{}-{name}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn lexical_canonicalize_resolves_dots_for_nonexistent_path() {
        let base = tmp("base");
        let p = base.join("a").join("..").join("b");
        let c = lexical_canonicalize(&p);
        let base_canon = lexical_canonicalize(&base);
        assert!(c.ends_with(base_canon.join("b")), "got {}", c.display());
        assert!(!c
            .components()
            .any(|x| matches!(x, std::path::Component::ParentDir)));
    }

    #[test]
    fn validate_path_rejects_traversal_escape_when_scoped() {
        let root = tmp("root");
        std::fs::create_dir_all(&root).unwrap();
        let escape = root.join("..").join("escape-dest");
        let r = validate_path(&escape.to_string_lossy(), std::slice::from_ref(&root));
        assert!(r.is_err(), "expected scope rejection, got {:?}", r);
        assert!(r.unwrap_err().contains("outside project roots"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_path_allows_nested_nonexistent_when_scoped() {
        let root = tmp("rootok");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("sub1").join("sub2");
        let r = validate_path(&p.to_string_lossy(), std::slice::from_ref(&root));
        assert!(r.is_ok(), "got {:?}", r);
        let c = r.unwrap();
        assert!(c.ends_with(Path::new("rootok/sub1/sub2")) || c.ends_with(Path::new("sub1/sub2")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_path_unrestricted_allows_dotdot() {
        let base = tmp("unrestricted");
        let p = base.join("x").join("..").join("y");
        let r = validate_path(&p.to_string_lossy(), &[]);
        assert!(r.is_ok(), "got {:?}", r);
        // `base` is never created on disk (this test is pure path logic), so
        // cleanup is a no-op. NEVER remove base.parent() — that is the system
        // temp dir and would wipe concurrent tests' scratch directories.
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_path_env_denied_via_dotdot() {
        let base = tmp("env");
        let p = base.join("foo").join("..").join(".env");
        let r = validate_path(&p.to_string_lossy(), &[]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains(".env"));
    }

    #[test]
    fn validate_path_ssh_denied_when_normalized() {
        let base = tmp("ssh");
        let p = base.join("a").join("..").join(".ssh").join("id_rsa");
        let r = validate_path(&p.to_string_lossy(), &[]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("sensitive"));
    }

    #[cfg(unix)]
    #[test]
    fn validate_path_resolves_symlink_in_prefix() {
        let root = tmp("symroot");
        let real = tmp("symreal");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // Path through the symlink, with a non-existent tail.
        let p = link.join("deep").join("file.txt");
        let r = validate_path(&p.to_string_lossy(), std::slice::from_ref(&real));
        assert!(r.is_ok(), "got {:?}", r);
        let c = r.unwrap();
        // Must resolve to the real target, not the symlink path. Canonicalize
        // `real` so the comparison holds on macOS, where $TMPDIR (`/var/...`)
        // resolves to `/private/var/...`.
        let real_canon = std::fs::canonicalize(&real).unwrap();
        assert!(
            c.starts_with(&real_canon),
            "canon {} not under real {}",
            c.display(),
            real_canon.display()
        );
        assert!(!c.to_string_lossy().contains("link"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&real);
    }

    #[test]
    fn validate_path_existing_path_canonicalized() {
        let d = tmp("existing");
        std::fs::create_dir_all(&d).unwrap();
        let r = validate_path(&d.to_string_lossy(), &[]);
        assert!(r.is_ok());
        let c = r.unwrap();
        assert_eq!(c, std::fs::canonicalize(&d).unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }
}
