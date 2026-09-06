use async_trait::async_trait;
use htmd::HtmlToMarkdown;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::projects::ProjectSkill;

use super::{validate_read_path, Tool, ToolCategory, ToolResult, ToolSpec};

fn schema(props: &[(&str, &str)], required: &[&str]) -> Value {
    let mut properties = serde_json::Map::new();
    for (k, ty) in props {
        properties.insert((*k).to_string(), serde_json::json!({ "type": *ty }));
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

const WEB_USER_AGENT: &str = concat!(
    "AIAssistant/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/AIAssistant)"
);

fn http_client() -> reqwest::Client {
    crate::net::apply(
        reqwest::Client::builder()
            .user_agent(WEB_USER_AGENT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(30)),
    )
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
}

static MD_CONVERTER: std::sync::LazyLock<HtmlToMarkdown> = std::sync::LazyLock::new(|| {
    HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript", "template"])
        .build()
});

static TITLE_RE: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").ok());

/// Realistic desktop browser UA — the polite `WEB_USER_AGENT` triggers
/// bot-challenge walls on most search engines.
const SEARCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

fn search_client() -> reqwest::Client {
    crate::net::apply(
        reqwest::Client::builder()
            .user_agent(SEARCH_USER_AGENT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(20)),
    )
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
}

static LINK_RE: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\((https?://[^)\s]+)\)").ok());

fn ensure_http_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("only http/https URLs are allowed (got '{other}')")),
    }
    if parsed.host_str().map(|h| h.is_empty()).unwrap_or(true) {
        return Err("URL is missing a host".into());
    }
    Ok(())
}

fn extract_title(html: &str) -> Option<String> {
    TITLE_RE
        .as_ref()?
        .captures(html)?
        .get(1)
        .map(|m| m.as_str().trim().to_string())
}

fn html_to_markdown(html: &str) -> String {
    MD_CONVERTER
        .convert(html)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Read a text file (truncated to a sane size).
pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a text file from disk and return its contents (truncated to ~32KB)."
                .into(),
            parameters: schema(&[("path", "string")], &["path"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };

        const MAX: usize = 32 * 1024;
        const SIZE_CAP: u64 = 16 * 1024 * 1024;

        let metadata = match tokio::fs::metadata(&abs).await {
            Ok(m) => m,
            Err(e) => return ToolResult::err(format!("read failed: {e}")),
        };
        if metadata.is_dir() {
            return ToolResult::err(format!(
                "path is a directory, not a file: {}; use list_dir instead",
                abs.display()
            ));
        }
        let file_size = metadata.len();
        if file_size > SIZE_CAP {
            return ToolResult::err(format!(
                "file is too large ({} bytes); read_file returns at most ~32KB. \
                 Use grep or run_command (e.g. head/tail) to inspect it",
                file_size
            ));
        }

        let bytes = match tokio::fs::read(&abs).await {
            Ok(b) => b,
            Err(e) => return ToolResult::err(format!("read failed: {e}")),
        };

        // NUL byte is a strong binary indicator (same heuristic as git).
        if bytes.contains(&0u8) {
            return ToolResult::err(format!(
                "file appears to be binary ({} bytes); not returned as text",
                file_size
            ));
        }

        let mut s = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return ToolResult::err(format!(
                    "file is not valid UTF-8 ({} bytes); reading aborted",
                    file_size
                ));
            }
        };

        if s.len() > MAX {
            let mut cut = MAX;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            s.truncate(cut);
            s.push_str(&format!("\n…[truncated, {} bytes total]", file_size));
        }
        ToolResult::ok(s)
    }
}

/// List entries of a directory.
pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".into(),
            description: "List entries of a directory (name + type).".into(),
            parameters: schema(&[("path", "string")], &["path"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        // Distinguish "not a directory" / "not found" from other read_dir failures
        // so the model gets an actionable message instead of a generic error.
        match tokio::fs::metadata(&abs).await {
            Ok(m) if !m.is_dir() => return ToolResult::err("not a directory"),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolResult::err(format!("no such directory: {abs:?}"));
            }
            Err(_) => {}
        }
        let mut entries = match tokio::fs::read_dir(&abs).await {
            Ok(e) => e,
            Err(e) => {
                let kind = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "permission denied"
                } else {
                    "list failed"
                };
                return ToolResult::err(format!("{kind}: {e}"));
            }
        };
        // (name, type) where type distinguishes symlinks and their resolved target
        let mut out: Vec<(String, &'static str)> = Vec::new();
        while let Ok(Some(e)) = entries.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            let kind = match e.file_type().await {
                Ok(t) if t.is_dir() => "dir",
                Ok(t) if t.is_symlink() => match tokio::fs::metadata(e.path()).await {
                    Ok(m) if m.is_dir() => "symlink→dir",
                    Ok(_) => "symlink→file",
                    Err(_) => "symlink",
                },
                Ok(_) => "file",
                Err(_) => "?",
            };
            out.push((name, kind));
        }
        if out.is_empty() {
            return ToolResult::ok("(empty)");
        }
        // Sort directories first, then case-insensitive by name.
        out.sort_by(|a, b| {
            let dir_a = a.1 == "dir" || a.1 == "symlink→dir";
            let dir_b = b.1 == "dir" || b.1 == "symlink→dir";
            dir_b
                .cmp(&dir_a)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                .then_with(|| a.0.cmp(&b.0))
        });
        let total = out.len();
        const MAX: usize = 1000;
        out.truncate(MAX);
        let mut s = String::with_capacity(out.len() * 40);
        for (i, (name, kind)) in out.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(name);
            s.push_str(" [");
            s.push_str(kind);
            s.push(']');
        }
        if total > MAX {
            s.push_str(&format!("\n…[{} entries truncated]", total - MAX));
        }
        ToolResult::ok(s)
    }
}

/// Recursively search files under a root for a regex pattern (content grep).
pub struct Grep;

const GREP_CAP: usize = 50;
const GREP_MAX_FILE_SIZE: u64 = 1 << 20; // 1 MiB — skip oversized files
const GREP_MAX_LINE_LEN: usize = 500; // truncate matching lines in output

#[async_trait]
impl Tool for Grep {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "grep".into(),
            description: "Recursively search file contents under a root directory for a regex pattern. Returns matching file:line:text (capped at ~50 hits). The pattern is a Rust regex (use (?i) for case-insensitive). Respects .gitignore/.ignore. Optional `include` glob filters by relative path (e.g. \"**/*.rs\"); a pattern without a slash matches the file name anywhere (e.g. \"*.rs\"). Skips .git, node_modules, target and common build/cache dirs, binary and oversized files. A single file path is searched directly.".into(),
            parameters: schema(
                &[("path", "string"), ("pattern", "string"), ("include", "string")],
                &["path", "pattern"],
            ),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(pattern) = arg_str(&args, "pattern") else {
            return ToolResult::err("missing 'pattern'");
        };
        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        let re = match Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("invalid regex: {e}")),
        };
        // Optional include glob: a slash-less pattern matches the file name
        // anywhere in the tree (ripgrep `-g` semantics). Empty string = no filter.
        let include = match arg_str(&args, "include").filter(|g| !g.is_empty()) {
            Some(g) => match globx::Matcher::new(g) {
                Ok(m) => Some((m, g.contains('/'))),
                Err(e) => return ToolResult::err(format!("invalid include glob: {e}")),
            },
            None => None,
        };
        if !abs.is_file() && !abs.is_dir() {
            return ToolResult::err(format!(
                "path not found or not a regular file/directory: {}",
                abs.display()
            ));
        }
        // Walk + file reads are blocking; run them off the async runtime.
        let hits = tokio::task::spawn_blocking(move || {
            grep_sync(abs, re, include, GREP_CAP, GREP_MAX_LINE_LEN)
        })
        .await;
        let hits = match hits {
            Ok(h) => h,
            Err(e) => return ToolResult::err(format!("search task failed: {e}")),
        };
        if hits.is_empty() {
            ToolResult::ok("no matches")
        } else {
            let mut out = hits.join("\n");
            if hits.len() >= GREP_CAP {
                out.push_str(
                    "\n…[stopped at 50 matches — narrow the pattern, path, or include filter to see more]",
                );
            }
            ToolResult::ok(out)
        }
    }
}

/// Directories never descended into by `glob`/`grep`: VCS internals plus common
/// generated/dependency/cache dirs. Unlike a blanket `.`-prefix skip this keeps
/// useful dot-dirs reachable (`.github`, `.vscode`, `.husky`, …); the `ignore`
/// crate handles the rest via `.gitignore`/`.ignore`.
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".bzr"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "__pycache__"
            | ".venv"
            | "venv"
            | ".tox"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | ".cache"
            | ".gradle"
            | ".idea"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | ".svelte-kit"
            | ".parcel-cache"
    )
}

fn is_binary_ext(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|s| s.to_str()),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "bmp"
                | "tiff"
                | "tif"
                | "mp3"
                | "mp4"
                | "wav"
                | "avi"
                | "mov"
                | "mkv"
                | "pdf"
                | "zip"
                | "tar"
                | "gz"
                | "zst"
                | "bz2"
                | "xz"
                | "7z"
                | "rar"
                | "jar"
                | "war"
                | "class"
                | "pyc"
                | "pyo"
                | "o"
                | "a"
                | "so"
                | "dll"
                | "dylib"
                | "exe"
                | "bin"
                | "wasm"
                | "db"
                | "sqlite"
                | "sqlite3"
                | "dat"
        )
    )
}

/// Char-boundary-safe truncation of a matching line for display.
fn truncate_line(line: &str, max: usize) -> String {
    if line.len() <= max {
        line.to_string()
    } else {
        let mut cut = max;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &line[..cut])
    }
}

/// Skip files whose contents must never be surfaced via grep, mirroring the
/// denylist in `validate_path` (`.env*`, `.ssh`, `secrets`). Inspects path
/// components so it works on both Unix and Windows separators.
fn is_sensitive_file(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name == ".env" || name.starts_with(".env.") {
        return true;
    }
    for c in p.components() {
        if let std::path::Component::Normal(s) = c {
            if matches!(s.to_str(), Some(".ssh") | Some("secrets")) {
                return true;
            }
        }
    }
    false
}

/// Synchronous core: walk the tree with the `ignore` crate (respects
/// .gitignore/.ignore, prunes heavy dirs, size-caps files) and collect hits.
fn grep_sync(
    root: std::path::PathBuf,
    re: Regex,
    include: Option<(globx::Matcher, bool)>,
    cap: usize,
    max_line: usize,
) -> Vec<String> {
    let mut hits: Vec<String> = Vec::with_capacity(cap);
    if root.is_file() {
        let parent = root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| root.clone());
        search_file_sync(
            &root,
            &parent,
            &re,
            include.as_ref(),
            &mut hits,
            cap,
            max_line,
        );
        return hits;
    }
    if !root.is_dir() {
        return hits;
    }
    let walker = ignore::WalkBuilder::new(&root)
        // hidden(false): keep hidden FILES searchable (preserves prior behavior);
        // hidden directories are pruned via filter_entry below.
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .max_filesize(Some(GREP_MAX_FILE_SIZE))
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true; // never prune the root, even if hidden
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if is_ignored_dir(name) {
                        return false;
                    }
                }
            }
            true
        })
        .build();
    for result in walker {
        if hits.len() >= cap {
            break;
        }
        let Ok(entry) = result else {
            continue;
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        search_file_sync(
            entry.path(),
            &root,
            &re,
            include.as_ref(),
            &mut hits,
            cap,
            max_line,
        );
    }
    hits
}

fn search_file_sync(
    p: &Path,
    root: &Path,
    re: &Regex,
    include: Option<&(globx::Matcher, bool)>,
    hits: &mut Vec<String>,
    cap: usize,
    max_line: usize,
) {
    if is_sensitive_file(p) {
        return;
    }
    let rel = p.strip_prefix(root).unwrap_or(p).display().to_string();
    if let Some((matcher, has_slash)) = include {
        let matched = if *has_slash {
            matcher.matches(&rel)
        } else {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| matcher.matches(n))
                .unwrap_or(false)
        };
        if !matched {
            return;
        }
    }
    if is_binary_ext(p) {
        return;
    }
    let Ok(bytes) = std::fs::read(p) else {
        return;
    };
    if bytes.contains(&0) {
        return; // binary content
    }
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return; // non-UTF-8
    };
    for (i, line) in content.lines().enumerate() {
        if re.is_match(line) {
            let out = truncate_line(line.trim_end(), max_line);
            hits.push(format!("{rel}:{}: {out}", i + 1));
            if hits.len() >= cap {
                return;
            }
        }
    }
}

/// Write (create/overwrite) a text file.
pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".into(),
            description: "Create or overwrite a file with the given full text content. Parent directories are created automatically. For partial edits to an existing file, prefer edit_file instead of rewriting the entire file.".into(),
            parameters: schema(
                &[("path", "string"), ("content", "string")],
                &["path", "content"],
            ),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(content) = arg_str(&args, "content") else {
            return ToolResult::err("missing 'content'");
        };
        let abs = match super::validate_write_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };

        // Reject if the target is an existing directory — tokio::fs::write
        // would fail with a confusing OS error otherwise.
        if let Ok(meta) = tokio::fs::symlink_metadata(&abs).await {
            if meta.is_dir() {
                return ToolResult::err(format!("cannot write: {} is a directory", abs.display()));
            }
        }

        let existed = tokio::fs::metadata(&abs).await.is_ok();

        // Create parent directories, propagating errors instead of swallowing them.
        if let Some(parent) = abs.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::err(format!("failed to create parent directory: {e}"));
                }
            }
        }

        match atomic_write(&abs, content, existed).await {
            Ok(_) => {
                let lines = content.lines().count();
                let verb = if existed { "overwrote" } else { "created" };
                ToolResult::ok(format!(
                    "{verb} {} ({} bytes, {lines} lines)",
                    abs.display(),
                    content.len()
                ))
            }
            Err(e) => ToolResult::err(format!("write failed: {e}")),
        }
    }
}

/// Atomic write: temp file in the same directory → fsync → preserve perms
/// (Unix, when overwriting) → rename. Cleans up the temp file on failure.
async fn atomic_write(abs: &std::path::Path, content: &str, existed: bool) -> std::io::Result<()> {
    let file_name = abs
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "tmp".into());
    let tmp = abs.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let res: std::io::Result<()> = async {
        let mut file = tokio::fs::File::create(&tmp).await?;
        file.write_all(content.as_bytes()).await?;
        // Best-effort fsync so data is durable before the atomic rename.
        let _ = file.sync_all().await;
        drop(file);
        // Preserve permissions of the original file when overwriting (Unix only);
        // the temp file would otherwise get default 0644, losing e.g. the exec bit.
        #[cfg(unix)]
        if existed {
            if let Ok(meta) = tokio::fs::metadata(abs).await {
                use std::os::unix::fs::PermissionsExt;
                let mode = meta.permissions().mode();
                let _ =
                    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).await;
            }
        }
        tokio::fs::rename(&tmp, abs).await
    }
    .await;
    if res.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    res
}

const MAX_EDIT_FILE_SIZE: u64 = 16 * 1024 * 1024;

fn old_string_excerpt(s: &str) -> String {
    let mut e = s.to_string();
    super::truncate_text(&mut e, 200);
    format!("\nold_string: {e:?}")
}

/// Apply multiple string replacements to a file (atomic; each old_string must match uniquely).
pub struct EditFile;

#[async_trait]
impl Tool for EditFile {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description: "Apply multiple exact string replacements to a file. Each old_string must occur exactly once.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": { "old_string": { "type": "string" }, "new_string": { "type": "string" } },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["path", "operations"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(ops) = args.get("operations").and_then(|v| v.as_array()) else {
            return ToolResult::err("missing 'operations'");
        };
        if ops.is_empty() {
            return ToolResult::err("operations is empty");
        }
        let abs = match super::validate_write_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };

        if let Ok(meta) = tokio::fs::symlink_metadata(&abs).await {
            if meta.is_dir() {
                return ToolResult::err(format!("cannot edit: {} is a directory", abs.display()));
            }
        }

        if let Ok(meta) = tokio::fs::metadata(&abs).await {
            if meta.len() > MAX_EDIT_FILE_SIZE {
                return ToolResult::err(format!(
                    "file too large to edit ({} bytes, max 16 MiB) — use write_file or apply_patch",
                    meta.len()
                ));
            }
        }

        let bytes = match tokio::fs::read(&abs).await {
            Ok(b) => b,
            Err(e) => {
                return ToolResult::err(if e.kind() == std::io::ErrorKind::NotFound {
                    not_found_msg(&abs).await
                } else {
                    format!("read failed: {e}")
                });
            }
        };
        let content = match String::from_utf8(bytes) {
            Ok(c) => c,
            Err(_) => return ToolResult::err("file is not valid UTF-8; edit aborted"),
        };

        // Models send LF while Windows files use CRLF: match and replace on an
        // LF-normalized copy, then restore CRLF so line endings stay intact.
        let crlf = content.contains("\r\n");
        let mut work = if crlf {
            content.replace("\r\n", "\n")
        } else {
            content
        };

        for (idx, op) in ops.iter().enumerate() {
            let i = idx + 1;
            let old_raw = op.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
            let new_raw = op.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
            let old = if crlf {
                old_raw.replace("\r\n", "\n")
            } else {
                old_raw.to_string()
            };
            let new = if crlf {
                new_raw.replace("\r\n", "\n")
            } else {
                new_raw.to_string()
            };
            if old.is_empty() {
                return ToolResult::err(format!("operation {i}: old_string is empty"));
            }
            if old == new {
                return ToolResult::err(format!(
                    "operation {i}: old_string equals new_string (no-op)"
                ));
            }
            let count = work.matches(&old).count();
            if count == 0 {
                return ToolResult::err(format!(
                    "operation {i}: old_string not found in file{}",
                    old_string_excerpt(&old)
                ));
            }
            if count > 1 {
                return ToolResult::err(format!(
                    "operation {i}: old_string matches {count} times (must be unique — \
                     include more surrounding context){}",
                    old_string_excerpt(&old)
                ));
            }
            work = work.replacen(&old, &new, 1);
        }

        let final_content = if crlf {
            work.replace('\n', "\r\n")
        } else {
            work
        };
        match atomic_write(&abs, &final_content, true).await {
            Ok(_) => {
                let n = ops.len();
                ToolResult::ok(format!(
                    "edited {} ({} operation{}, {} bytes, {} lines)",
                    abs.display(),
                    n,
                    if n == 1 { "" } else { "s" },
                    final_content.len(),
                    final_content.lines().count()
                ))
            }
            Err(e) => ToolResult::err(format!("write failed: {e}")),
        }
    }
}

/// Create a directory (recursive by default).
pub struct MakeDir;

#[async_trait]
impl Tool for MakeDir {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "make_dir".into(),
            description: "Create a directory. Recursive by default (creates missing parents). \
                Idempotent: returns 'already exists' if the directory is present. \
                Errors if the path points to an existing file or a broken symlink."
                .into(),
            parameters: schema(&[("path", "string"), ("recursive", "boolean")], &["path"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        if path.trim().is_empty() {
            return ToolResult::err("path must not be empty");
        }
        let abs = match super::validate_write_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        // Distinguish "already a directory" (idempotent ok), "exists as a file
        // / symlink" (conflict), and "dangling symlink" (broken entry) before
        // creating, so the model gets an actionable message instead of a
        // generic OS error.
        match tokio::fs::symlink_metadata(&abs).await {
            Ok(_) => match tokio::fs::metadata(&abs).await {
                Ok(m) if m.is_dir() => {
                    return ToolResult::ok(format!("already exists: {}", abs.display()));
                }
                Ok(_) => {
                    return ToolResult::err(format!(
                        "path exists and is not a directory: {}",
                        abs.display()
                    ));
                }
                Err(_) => {
                    return ToolResult::err(format!("path is a broken symlink: {}", abs.display()));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return ToolResult::err(format!("stat failed: {e}")),
        }
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let res = if recursive {
            tokio::fs::create_dir_all(&abs).await
        } else {
            tokio::fs::create_dir(&abs).await
        };
        match res {
            Ok(_) => ToolResult::ok(format!("created {}", abs.display())),
            Err(e) => ToolResult::err(format!("make_dir failed: {e}")),
        }
    }
}

/// Recursively copy `src` into `dst` (preserving symlinks and directory
/// structure). `dst` must not exist. Used for cross-device directory moves.
async fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = tokio::fs::symlink_metadata(src).await?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = tokio::fs::read_link(src).await?;
        tokio::fs::symlink(&target, dst).await?;
        return Ok(());
    }
    if ft.is_dir() {
        tokio::fs::create_dir(dst).await?;
        #[cfg(unix)]
        {
            // best-effort: never abort the copy over permission metadata
            let _ = tokio::fs::set_permissions(dst, meta.permissions()).await;
        }
        let mut entries = tokio::fs::read_dir(src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let child = dst.join(entry.file_name());
            Box::pin(copy_recursive(&entry.path(), &child)).await?;
        }
        return Ok(());
    }
    tokio::fs::copy(src, dst).await?;
    Ok(())
}

fn is_cross_device(e: &std::io::Error) -> bool {
    // EXDEV: `CrossesDevices` where available, raw errno 18 as fallback
    // (same value on Linux and Windows).
    e.kind() == std::io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18)
}

async fn move_cross_device(src: &Path, dst: &Path, ft: std::fs::FileType) -> ToolResult {
    if ft.is_symlink() {
        let target = match tokio::fs::read_link(src).await {
            Ok(t) => t,
            Err(e) => return ToolResult::err(format!("read symlink failed: {e}")),
        };
        if let Err(e) = tokio::fs::symlink(&target, dst).await {
            return ToolResult::err(format!("recreate symlink failed: {e}"));
        }
        return match tokio::fs::remove_file(src).await {
            Ok(_) => ToolResult::ok(format!(
                "moved symlink {} -> {}",
                src.display(),
                dst.display()
            )),
            Err(e) => ToolResult::ok(format!(
                "copied symlink {} -> {} (warning: could not remove source: {e})",
                src.display(),
                dst.display()
            )),
        };
    }
    if ft.is_dir() {
        if let Err(e) = copy_recursive(src, dst).await {
            return ToolResult::err(format!("copy directory failed: {e}"));
        }
        return match tokio::fs::remove_dir_all(src).await {
            Ok(_) => ToolResult::ok(format!(
                "copied+removed directory {} -> {}",
                src.display(),
                dst.display()
            )),
            Err(e) => ToolResult::ok(format!(
                "copied directory {} -> {} (warning: could not remove source: {e})",
                src.display(),
                dst.display()
            )),
        };
    }
    if let Err(e) = tokio::fs::copy(src, dst).await {
        return ToolResult::err(format!("copy failed: {e}"));
    }
    match tokio::fs::remove_file(src).await {
        Ok(_) => ToolResult::ok(format!(
            "copied+removed {} -> {}",
            src.display(),
            dst.display()
        )),
        Err(e) => ToolResult::ok(format!(
            "copied {} -> {} (warning: could not remove source: {e})",
            src.display(),
            dst.display()
        )),
    }
}

/// Move or rename a path.
pub struct MovePath;

#[async_trait]
impl Tool for MovePath {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "move_path".into(),
            description: "Move or rename a file, directory, or symlink from src to dst; missing dst parent directories are created. An existing dst is never overwritten unless overwrite=true is set (the old dst is then removed first). Same-filesystem moves are atomic renames; when src and dst are on different filesystems the source is copied recursively (symlinks stay symlinks) and then removed.".into(),
            parameters: schema(
                &[("src", "string"), ("dst", "string"), ("overwrite", "boolean")],
                &["src", "dst"],
            ),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(src) = arg_str(&args, "src") else {
            return ToolResult::err("missing 'src'");
        };
        let Some(dst) = arg_str(&args, "dst") else {
            return ToolResult::err("missing 'dst'");
        };
        let s = match super::validate_write_path(src) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        let d = match super::validate_write_path(dst) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        let overwrite = args
            .get("overwrite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let sm = match tokio::fs::symlink_metadata(&s).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolResult::err(format!("source not found: {}", s.display()));
            }
            Err(e) => return ToolResult::err(format!("stat source failed: {e}")),
        };
        if s == d {
            return ToolResult::ok(format!(
                "no-op: source and destination are the same: {}",
                s.display()
            ));
        }
        if d.starts_with(&s) {
            return ToolResult::err(format!(
                "cannot move {} into its own descendant {}",
                s.display(),
                d.display()
            ));
        }
        match tokio::fs::symlink_metadata(&d).await {
            Ok(dm) => {
                if !overwrite {
                    return ToolResult::err(format!(
                        "destination exists: {} (set overwrite=true to replace)",
                        d.display()
                    ));
                }
                let removed = if dm.is_dir() {
                    tokio::fs::remove_dir_all(&d).await
                } else {
                    tokio::fs::remove_file(&d).await
                };
                if let Err(e) = removed {
                    return ToolResult::err(format!("overwrite: remove destination failed: {e}"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return ToolResult::err(format!("stat destination failed: {e}")),
        }
        if let Some(parent) = d.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolResult::err(format!("create destination parent failed: {e}"));
                }
            }
        }
        match tokio::fs::rename(&s, &d).await {
            Ok(_) => ToolResult::ok(format!("moved {} -> {}", s.display(), d.display())),
            Err(e) if is_cross_device(&e) => move_cross_device(&s, &d, sm.file_type()).await,
            Err(e) => ToolResult::err(format!("move failed: {e}")),
        }
    }
}

/// Set file mode (chmod). Unix only; on Windows it returns an error.
pub struct SetFileMode;

#[async_trait]
impl Tool for SetFileMode {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "set_file_mode".into(),
            description: "Set file permissions (chmod, Unix only). `mode` is an octal string of \
                up to 4 digits like '644', '755', '2755' (setgid). On Windows this returns an \
                error (no chmod semantics)."
                .into(),
            parameters: schema(&[("path", "string"), ("mode", "string")], &["path", "mode"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(mode) = arg_str(&args, "mode") else {
            return ToolResult::err("missing 'mode'");
        };
        let abs = match super::validate_write_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        let raw = mode
            .trim()
            .strip_prefix("0o")
            .unwrap_or_else(|| mode.trim());
        if raw.is_empty() {
            return ToolResult::err("invalid octal mode: empty value");
        }
        let parsed = match u32::from_str_radix(raw, 8) {
            Ok(m) => m,
            Err(_) => {
                return ToolResult::err(format!(
                    "invalid octal mode '{mode}': use octal digits 0-7 (e.g. '644', '755', '2755')"
                ));
            }
        };
        // Mask off file-type bits (S_IFMT); chmod(2) ignores them. Keep special bits
        // (setuid/setgid/sticky, 0o7000) so '2755' etc. work as expected.
        let m = parsed & 0o7777;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            match tokio::fs::set_permissions(&abs, std::fs::Permissions::from_mode(m)).await {
                Ok(_) => ToolResult::ok(format!("chmod {:o} {}", m, abs.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => ToolResult::err(format!(
                    "chmod failed: no such file or directory: {}",
                    abs.display()
                )),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => ToolResult::err(
                    format!("chmod failed: permission denied: {}", abs.display()),
                ),
                Err(e) => ToolResult::err(format!("chmod failed: {e}")),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = m;
            ToolResult::err("set_file_mode is not supported on Windows (no chmod semantics)")
        }
    }
}

/// Delete a file or directory. Destructive — always requires approval.
pub struct DeletePath;

#[async_trait]
impl Tool for DeletePath {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Destructive
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "delete_path".into(),
            description: "Delete a file, directory (recursively), or symlink. Removes the path \
                itself; a symlink is unlinked, its target is left intact. Refuses to delete a \
                project root or a directory containing denylisted entries (.env*, .ssh, \
                secrets). By default permanent; when 'Move to trash' is enabled in Settings, \
                moves to the OS trash/recycle bin instead."
                .into(),
            parameters: schema(&[("path", "string")], &["path"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        // Set by the approval layer when the user explicitly confirmed a
        // symlink whose target is outside the project roots (unlink the link,
        // target untouched).
        let allow_escape = args
            .get("_unlink_escape_authorized")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let abs = match super::validate_unlink_path(path, allow_escape) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        if super::is_protected_root(&abs) {
            return ToolResult::err(format!(
                "refusing to delete a project root: {}",
                abs.display()
            ));
        }
        // symlink_metadata: don't follow — a dangling symlink still "exists" and
        // must be removable, and we decide file vs dir vs symlink from the link
        // itself (not its target).
        let meta = match tokio::fs::symlink_metadata(&abs).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolResult::err(format!("not found: {}", abs.display()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return ToolResult::err(format!(
                    "stat failed (permission denied): {}",
                    abs.display()
                ));
            }
            Err(e) => return ToolResult::err(format!("stat failed: {e}")),
        };
        let ft = meta.file_type();
        let trash = super::trash_mode();
        if ft.is_symlink() {
            return delete_entry(&abs, trash, "symlink").await;
        }
        if ft.is_dir() {
            // The denylist-children guard is defense-in-depth for permanent
            // removal; trash is reversible, so skip it there.
            if !trash {
                if let Err(e) = check_no_denylisted_children(&abs).await {
                    return ToolResult::err(e);
                }
            }
            return delete_dir(&abs, trash).await;
        }
        delete_entry(&abs, trash, "file").await
    }
}

/// Refuse to delete a directory whose immediate children include a denylisted
/// entry (`.env*`, `.ssh`, `secrets`) — defense-in-depth so the recursive
/// remove does not silently wipe sensitive files nested under the target.
/// Only the top level is scanned: deeper nesting is caught by the project-root
/// guard and `validate_path` for direct access.
async fn check_no_denylisted_children(dir: &Path) -> Result<(), String> {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => return Err(format!("read_dir failed: {e}")),
    };
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("read_dir failed: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name == ".env" || name.starts_with(".env.") || name == ".ssh" || name == "secrets" {
            return Err(format!(
                "refusing to delete directory containing denylisted entry '{}': {}",
                entry.file_name().to_string_lossy(),
                dir.display()
            ));
        }
    }
    Ok(())
}

/// Remove a file or symlink — trash (recoverable) or permanent (`remove_file`).
async fn delete_entry(abs: &Path, trash: bool, kind: &str) -> ToolResult {
    let display = abs.display().to_string();
    if trash {
        let p = abs.to_path_buf();
        return match tokio::task::spawn_blocking(move || trash::delete(&p)).await {
            Ok(Ok(_)) => ToolResult::ok(format!("moved to trash ({kind}): {display}")),
            Ok(Err(e)) => ToolResult::err(format!("trash failed: {e}")),
            Err(e) => ToolResult::err(format!("trash task failed: {e}")),
        };
    }
    match tokio::fs::remove_file(abs).await {
        Ok(_) => ToolResult::ok(format!("deleted {kind} {display}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ToolResult::ok(format!("already gone: {display}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            ToolResult::err(format!("delete failed (permission denied): {display}"))
        }
        Err(e) => ToolResult::err(format!("delete failed: {e}")),
    }
}

/// Remove a directory — trash (recoverable) or permanent (`remove_dir_all`
/// crate, robust on Windows). Only called for real directories (symlinks are
/// handled by `delete_entry`).
async fn delete_dir(abs: &Path, trash: bool) -> ToolResult {
    let display = abs.display().to_string();
    if trash {
        let p = abs.to_path_buf();
        return match tokio::task::spawn_blocking(move || trash::delete(&p)).await {
            Ok(Ok(_)) => ToolResult::ok(format!("moved to trash (directory): {display}")),
            Ok(Err(e)) => ToolResult::err(format!("trash failed: {e}")),
            Err(e) => ToolResult::err(format!("trash task failed: {e}")),
        };
    }
    let p = abs.to_path_buf();
    match tokio::task::spawn_blocking(move || remove_dir_all::remove_dir_all(&p)).await {
        Ok(Ok(_)) => ToolResult::ok(format!("deleted directory {display}")),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            ToolResult::ok(format!("already gone: {display}"))
        }
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
            ToolResult::err(format!(
                "delete failed: directory not empty (concurrent write? partial removal may have \
            occurred): {display}"
            ))
        }
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            ToolResult::err(format!("delete failed (permission denied): {display}"))
        }
        Ok(Err(e)) => ToolResult::err(format!("delete failed: {e}")),
        Err(e) => ToolResult::err(format!("delete task failed: {e}")),
    }
}

#[cfg(unix)]
fn status_label(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        format!("exit {code}")
    } else if let Some(sig) = status.signal() {
        format!("signal {sig}")
    } else {
        "unknown".into()
    }
}

#[cfg(not(unix))]
fn status_label(status: &std::process::ExitStatus) -> String {
    format!("exit {}", status.code().unwrap_or(-1))
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

#[cfg(windows)]
fn shell_command(command: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

/// Run a shell command — non-PTY registry fallback; the production chat path
/// runs commands through a PTY in chat.rs (docs/12).
pub struct RunCommand;

#[async_trait]
impl Tool for RunCommand {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Exec
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command".into(),
            description: "Run a shell command (`sh -c` on Unix, `cmd /C` on Windows). Captures stdout+stderr (truncated). Optional cwd (existing directory), env (object of extra variables merged over the parent env) and timeout_ms (default 120000, clamped to 1000-600000).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "env": { "type": "object", "additionalProperties": { "type": "string" } },
                    "timeout_ms": { "type": "number" }
                },
                "required": ["command"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(command) = arg_str(&args, "command") else {
            return ToolResult::err("missing 'command'");
        };
        let mut cmd = shell_command(command);
        if let Some(cwd) = arg_str(&args, "cwd") {
            let p = Path::new(cwd);
            if !p.is_dir() {
                return ToolResult::err(format!("cwd is not a directory: {cwd}"));
            }
            cmd.current_dir(p);
        }
        if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                if let Some(vs) = v.as_str() {
                    cmd.env(k, vs);
                }
            }
        }
        let timeout_ms = super::parse_timeout_ms(&args);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("spawn failed: {e}")),
        };
        // `wait_with_output` consumes the Child, which would prevent killing
        // on timeout. Read both pipes concurrently and wait via `&mut self`.
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf).await;
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            buf
        });
        let status =
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), child.wait())
                .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => return ToolResult::err(format!("wait failed: {e}")),
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return ToolResult::err(format!("timed out after {timeout_ms}ms"));
                }
            };
        let out_stdout = stdout_task.await.unwrap_or_default();
        let out_stderr = stderr_task.await.unwrap_or_default();
        let mut combined = String::from_utf8_lossy(&out_stdout).to_string();
        if !out_stderr.is_empty() {
            combined.push_str("\n[stderr]\n");
            combined.push_str(&String::from_utf8_lossy(&out_stderr));
        }
        super::truncate_text(&mut combined, 32 * 1024);
        ToolResult::ok(format!("[{}]\n{combined}", status_label(&status)))
    }
}

/// Find files matching a glob pattern under a root directory (docs/17).
pub struct Glob;

#[async_trait]
impl Tool for Glob {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "glob".into(),
            description: "Find files under a root directory matching a glob pattern \
            (e.g. **/*.rs, src/**/{test,spec}/*.{ts,js}). Supports `*` (one path \
            segment), `**` (across segments), `?`, `[abc]`, `[!abc]` negation, and \
            `{a,b}` alternation. A pattern without a slash matches file names at any \
            depth (e.g. *.rs). Respects .gitignore/.ignore; skips .git, node_modules, \
            target and common build/cache dirs. Returns up to 100 paths relative to \
            the root, sorted."
                .into(),
            parameters: schema(
                &[("path", "string"), ("pattern", "string")],
                &["path", "pattern"],
            ),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(pattern) = arg_str(&args, "pattern") else {
            return ToolResult::err("missing 'pattern'");
        };
        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        if !abs.is_dir() {
            return ToolResult::err(format!(
                "path not found or not a directory: {}",
                abs.display()
            ));
        }
        let matcher = match globx::Matcher::new(pattern) {
            Ok(m) => m,
            Err(e) => return ToolResult::err(format!("invalid glob: {e}")),
        };
        let root = abs.clone();
        let hits = tokio::task::spawn_blocking(move || glob_collect(&root, &matcher, 100)).await;
        let mut hits = match hits {
            Ok(h) => h,
            Err(e) => return ToolResult::err(format!("glob task failed: {e}")),
        };
        if hits.is_empty() {
            return ToolResult::ok("no matches");
        }
        hits.sort();
        let mut out = hits.join("\n");
        if hits.len() >= 100 {
            out.push_str("\n…[stopped at 100 matches — narrow the pattern or path to see more]");
        }
        ToolResult::ok(out)
    }
}

/// Recursively collect files under `root` whose relative path OR file name
/// matches `matcher`, using the `ignore` walker (gitignore-aware, consistent
/// with `grep`). Symlinks are not followed (safe default, matches `grep`).
fn glob_collect(root: &Path, matcher: &globx::Matcher, cap: usize) -> Vec<String> {
    let mut hits: Vec<String> = Vec::with_capacity(cap);
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if is_ignored_dir(name) {
                        return false;
                    }
                }
            }
            true
        })
        .build();
    for result in walker {
        if hits.len() >= cap {
            break;
        }
        let Ok(entry) = result else {
            continue;
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let p = entry.path();
        let rel = p.strip_prefix(root).unwrap_or(p).display().to_string();
        let name = entry.file_name().to_string_lossy().to_string();
        if matcher.matches(&rel) || matcher.matches(&name) {
            hits.push(rel);
        }
    }
    hits
}

/// Lightweight glob matcher: supports `*`, `**`, `?`, character classes
/// (incl. `[!...]` negation), and `{a,b}` alternation. Segment-aware (`*`
/// matches no `/`); input backslashes are normalized to `/` for cross-platform
/// path matching.
mod globx {
    use anyhow::{anyhow, Result};

    pub struct Matcher {
        re: regex::Regex,
    }

    impl Matcher {
        pub fn new(pattern: &str) -> Result<Self> {
            let re = regex::Regex::new(&glob_to_regex(pattern))
                .map_err(|e| anyhow!("invalid pattern: {e}"))?;
            Ok(Self { re })
        }

        pub fn matches(&self, s: &str) -> bool {
            // Normalize Windows backslashes to '/' so patterns (which use '/')
            // match relative paths on every platform.
            if s.contains('\\') {
                self.re.is_match(&s.replace('\\', "/"))
            } else {
                self.re.is_match(s)
            }
        }
    }

    fn glob_to_regex(pat: &str) -> String {
        let mut out = String::with_capacity(pat.len() + 2);
        out.push('^');
        append_glob_body(&mut out, pat.as_bytes());
        out.push('$');
        out
    }

    /// Append the regex body for a glob fragment (no anchors). Handles `*`,
    /// `**`, `?`, `[...]` (with `[!...]` negation), and `{a,b}` alternation.
    /// Recursed into for each alternative so nested wildcards work.
    fn append_glob_body(out: &mut String, b: &[u8]) {
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'*' => {
                    if i + 1 < b.len() && b[i + 1] == b'*' {
                        // `**/` -> `(.*/)?`; bare `**` -> `.*`.
                        if i + 2 < b.len() && b[i + 2] == b'/' {
                            out.push_str("(.*/)?");
                            i += 3;
                            continue;
                        }
                        out.push_str(".*");
                        i += 2;
                        continue;
                    }
                    out.push_str("[^/]*");
                }
                b'?' => out.push_str("[^/]"),
                b'[' => {
                    i += 1;
                    out.push('[');
                    // glob negation `[!...]` -> regex `[^...]`.
                    if i < b.len() && b[i] == b'!' {
                        out.push('^');
                        i += 1;
                    }
                    // A leading ']' is a literal.
                    if i < b.len() && b[i] == b']' {
                        out.push('\\');
                        out.push(']');
                        i += 1;
                    }
                    let mut closed = false;
                    while i < b.len() {
                        if b[i] == b']' {
                            closed = true;
                            i += 1;
                            break;
                        }
                        if b[i] == b'\\' {
                            out.push_str("\\\\");
                        } else {
                            out.push(b[i] as char);
                        }
                        i += 1;
                    }
                    if closed {
                        out.push(']');
                    }
                    // Unterminated class -> leave '[' open -> invalid regex,
                    // surfaced as an error by Matcher::new.
                    continue;
                }
                b'{' => {
                    // Find the matching '}', accounting for nesting and char classes.
                    let mut j = i + 1;
                    let mut depth = 1u32;
                    let mut in_class = false;
                    while j < b.len() {
                        let c = b[j];
                        if in_class {
                            if c == b']' {
                                in_class = false;
                            }
                        } else {
                            match c {
                                b'[' => in_class = true,
                                b'{' => depth += 1,
                                b'}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        j += 1;
                    }
                    let inner = &b[i + 1..j.min(b.len())];
                    out.push('(');
                    // Split alternatives on top-level commas (ignoring commas
                    // inside char classes or nested braces).
                    let mut seg_start = 0;
                    let mut d = 0u32;
                    let mut in_class = false;
                    let mut k = 0;
                    while k < inner.len() {
                        let c = inner[k];
                        if in_class {
                            if c == b']' {
                                in_class = false;
                            }
                        } else {
                            match c {
                                b'[' => in_class = true,
                                b'{' => d += 1,
                                b'}' => {
                                    d = d.saturating_sub(1);
                                }
                                b',' if d == 0 => {
                                    append_glob_body(out, &inner[seg_start..k]);
                                    out.push('|');
                                    seg_start = k + 1;
                                }
                                _ => {}
                            }
                        }
                        k += 1;
                    }
                    append_glob_body(out, &inner[seg_start..]);
                    out.push(')');
                    i = if j < b.len() { j + 1 } else { b.len() };
                    continue;
                }
                b'.' | b'+' | b'(' | b')' | b'^' | b'$' | b'|' | b'\\' | b'}' | b']' => {
                    out.push('\\');
                    out.push(b[i] as char);
                }
                _ => out.push(b[i] as char),
            }
            i += 1;
        }
    }
}

fn file_type_label(ft: &std::fs::FileType) -> &'static str {
    if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else if ft.is_symlink() {
        "symlink"
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if ft.is_fifo() {
                "fifo"
            } else if ft.is_socket() {
                "socket"
            } else if ft.is_block_device() {
                "block_device"
            } else if ft.is_char_device() {
                "char_device"
            } else {
                "special"
            }
        }
        #[cfg(not(unix))]
        {
            "special"
        }
    }
}

/// `ls`-style permission string with setuid/setgid/sticky, e.g. `rwxr-xr-x`.
#[cfg(unix)]
fn perm_string(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    s.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    s.push(match (mode & 0o4000 != 0, mode & 0o100 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    s.push(match (mode & 0o2000 != 0, mode & 0o010 != 0) {
        (true, true) => 's',
        (true, false) => 'S',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    s.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    s.push(match (mode & 0o1000 != 0, mode & 0o001 != 0) {
        (true, true) => 't',
        (true, false) => 'T',
        (false, true) => 'x',
        (false, false) => '-',
    });
    s
}

fn fmt_time(t: std::time::SystemTime) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from(t);
    format!("{} ({})", dt.to_rfc3339(), dt.timestamp())
}

/// Suggest up to 3 similarly-named siblings when a path is not found, mirroring
/// opencode's "Did you mean …" pattern to save the model extra round-trips.
async fn not_found_msg(abs: &std::path::Path) -> String {
    let mut msg = format!("no such file or directory: {}", abs.display());
    let Some(parent) = abs.parent() else {
        return msg;
    };
    let base = match abs.file_name() {
        Some(n) => n.to_string_lossy().to_lowercase(),
        None => return msg,
    };
    if base.is_empty() {
        return msg;
    }
    let Ok(mut entries) = tokio::fs::read_dir(parent).await else {
        return msg;
    };
    let mut suggestions: Vec<String> = Vec::new();
    while let Ok(Some(e)) = entries.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        let ln = name.to_lowercase();
        if ln.contains(&base) || base.contains(&ln) {
            suggestions.push(name);
            if suggestions.len() >= 3 {
                break;
            }
        }
    }
    if !suggestions.is_empty() {
        suggestions.sort();
        msg.push_str("\nDid you mean: ");
        msg.push_str(&suggestions.join(", "));
        msg.push('?');
    }
    msg
}

/// Best-effort binary sniff: a NUL byte in the first 4 KB strongly implies a
/// binary file, so the model can skip a doomed `read` call.
async fn sniff_binary(path: &std::path::Path) -> Option<bool> {
    let mut f = tokio::fs::File::open(path).await.ok()?;
    let mut buf = [0u8; 4096];
    let n = f.read(&mut buf).await.ok()?;
    Some(buf[..n].contains(&0))
}

/// stat a path: size, mtime, type, permissions (docs/17).
pub struct FileInfo;

#[async_trait]
impl Tool for FileInfo {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "file_info".into(),
            description: "Return file metadata: type (file/dir/symlink/fifo/socket/\
                block_device/char_device), size, mtime, created time, permissions, and \
                symlink target/broken status. A broken symlink is reported as \
                type=symlink (not an error). Valid symlinks are resolved to their target."
                .into(),
            parameters: schema(&[("path", "string")], &["path"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let abs = match validate_read_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };
        // Operate on the sandbox-validated, canonicalized `abs` only — same as
        // read_file/list_dir. lstat distinguishes a broken symlink from a
        // missing path: `validate_read_path` canonicalizes through valid
        // symlinks (so they reach here already resolved to their in-scope
        // target), but a *broken* symlink cannot be canonicalized and falls
        // back to the link path, which `symlink_metadata` then flags.
        let lmeta = match tokio::fs::symlink_metadata(&abs).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return ToolResult::err(not_found_msg(&abs).await);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return ToolResult::err(format!("permission denied: {}", abs.display()));
            }
            Err(e) => return ToolResult::err(format!("stat failed: {e}")),
        };
        let is_symlink = lmeta.file_type().is_symlink();
        // A symlink reaching here is broken (valid ones are pre-resolved).
        // Following it confirms `broken` without turning it into an error.
        let target_meta: Option<std::fs::Metadata> = if is_symlink {
            match tokio::fs::metadata(&abs).await {
                Ok(m) => Some(m),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    return ToolResult::err(format!(
                        "permission denied (symlink target): {}",
                        abs.display()
                    ));
                }
                Err(e) => return ToolResult::err(format!("stat failed: {e}")),
            }
        } else {
            None
        };
        let broken = is_symlink && target_meta.is_none();
        let report = target_meta.as_ref().unwrap_or(&lmeta);
        let kind = if is_symlink {
            "symlink"
        } else {
            file_type_label(&report.file_type())
        };

        let mut out = String::with_capacity(160);
        out.push_str(&format!("path: {}\n", abs.display()));
        out.push_str(&format!("type: {}\n", kind));
        out.push_str(&format!("size: {} bytes\n", report.len()));
        if is_symlink {
            if let Ok(target) = tokio::fs::read_link(&abs).await {
                out.push_str(&format!("symlink_target: {}\n", target.display()));
            }
            out.push_str(&format!("broken: {}\n", broken));
        }
        if let Ok(t) = report.modified() {
            out.push_str(&format!("mtime: {}\n", fmt_time(t)));
        }
        if let Ok(t) = report.created() {
            out.push_str(&format!("created: {}\n", fmt_time(t)));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = report.permissions().mode();
            out.push_str(&format!("mode: {} ({:o})", perm_string(m), m & 0o7777));
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::fs::MetadataExt;
            let attrs = report.file_attributes();
            let mut parts: Vec<&str> = Vec::new();
            if attrs & 0x1 != 0 {
                parts.push("readonly");
            }
            if attrs & 0x2 != 0 {
                parts.push("hidden");
            }
            if attrs & 0x4 != 0 {
                parts.push("system");
            }
            if parts.is_empty() {
                out.push_str("mode: normal");
            } else {
                out.push_str("mode: ");
                out.push_str(&parts.join(","));
            }
        }
        if kind == "file" && report.len() > 0 {
            if let Some(bin) = sniff_binary(&abs).await {
                out.push_str(&format!("\nbinary: {}", bin));
            }
        }
        ToolResult::ok(out.trim_end().to_string())
    }
}

/// Apply a unified diff to a file (docs/17 — BP cline apply_patch).
pub struct ApplyPatch;

#[async_trait]
impl Tool for ApplyPatch {
    fn category(&self) -> ToolCategory {
        ToolCategory::Write
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "apply_patch".into(),
            description: "Apply a unified diff to a file. The diff must be a standard \
            unified diff with --- and +++ headers and @@ hunks."
                .into(),
            parameters: schema(&[("path", "string"), ("diff", "string")], &["path", "diff"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(path) = arg_str(&args, "path") else {
            return ToolResult::err("missing 'path'");
        };
        let Some(diff) = arg_str(&args, "diff") else {
            return ToolResult::err("missing 'diff'");
        };
        let abs = match super::validate_write_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::err(e),
        };

        if let Ok(meta) = tokio::fs::symlink_metadata(&abs).await {
            if meta.is_dir() {
                return ToolResult::err(format!("cannot patch: {} is a directory", abs.display()));
            }
        }

        if let Ok(meta) = tokio::fs::metadata(&abs).await {
            if meta.len() > MAX_EDIT_FILE_SIZE {
                return ToolResult::err(format!(
                    "file too large to patch ({} bytes, max 16 MiB) — use write_file",
                    meta.len()
                ));
            }
        }

        let mut hunks = match parse_unified_diff(diff) {
            Ok(h) => h,
            Err(e) => return ToolResult::err(e.to_string()),
        };
        // A diff starting with @@ -0,0 @@ targets a file that need not exist yet.
        let creates_file = hunks
            .first()
            .is_some_and(|h| h.old_start == 0 && h.old_lines.is_empty());

        let read = tokio::fs::read(&abs).await;
        let existed = read.is_ok();
        let bytes = match read {
            Ok(b) => b,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    if creates_file {
                        Vec::new()
                    } else {
                        return ToolResult::err(not_found_msg(&abs).await);
                    }
                } else {
                    return ToolResult::err(format!("read failed: {e}"));
                }
            }
        };
        if bytes.iter().take(8192).any(|&b| b == 0) {
            return ToolResult::err("file appears to be binary; patch aborted");
        }
        let content = match String::from_utf8(bytes) {
            Ok(c) => c,
            Err(_) => return ToolResult::err("file is not valid UTF-8; patch aborted"),
        };

        // Models send LF while Windows files use CRLF: match hunks against an
        // LF-normalized copy, then restore CRLF so line endings stay intact.
        let crlf = content.contains("\r\n");
        let normalized = if crlf {
            content.replace("\r\n", "\n")
        } else {
            content
        };
        let mut result: Vec<String> = normalized.lines().map(|s| s.to_string()).collect();

        // Line numbers in hunk headers are only a hint; hunks are located by
        // matching old_lines content, in forward file order.
        hunks.sort_by_key(|h| h.old_start);
        let mut cursor: usize = 0;
        for (i, hunk) in hunks.iter().enumerate() {
            let hunk_index = i + 1;
            if hunk.old_start == 0 && hunk.old_lines.is_empty() {
                result = hunk.new_lines.clone();
                cursor = hunk.new_lines.len();
                continue;
            }
            if hunk.old_lines.is_empty() {
                // Pure insertion: nothing to match, the header line is the anchor.
                let start = hunk.old_start.saturating_sub(1).min(result.len());
                result.splice(start..start, hunk.new_lines.iter().cloned());
                cursor = start + hunk.new_lines.len();
                continue;
            }
            let n = hunk.old_lines.len();
            let hint = hunk.old_start.saturating_sub(1);
            let Some(start) = locate_old_lines(&result, &hunk.old_lines, cursor, hint) else {
                let ambiguous = contains_lines(&result, &hunk.old_lines, false)
                    || contains_lines(&result, &hunk.old_lines, true)
                    || (result.len() <= 50_000 && contains_lines_fuzzy(&result, &hunk.old_lines));
                if ambiguous {
                    return ToolResult::err(format!(
                        "hunk {hunk_index} context matches multiple locations; \
                         add more surrounding context"
                    ));
                }
                let first = hunk.old_lines.first().map(String::as_str).unwrap_or("");
                return ToolResult::err(format!(
                    "hunk {hunk_index} context not found (expected {n} old line(s) starting \
                     near line {}). First expected line: {first:?}. Re-read the file to get \
                     current content.",
                    hint + 1
                ));
            };
            result.splice(start..start + n, hunk.new_lines.iter().cloned());
            cursor = start + hunk.new_lines.len();
        }

        let mut lf = result.join("\n");
        if !result.is_empty() && !hunks.iter().any(|h| h.new_no_newline) {
            lf.push('\n');
        }
        let final_content = if crlf { lf.replace('\n', "\r\n") } else { lf };
        match atomic_write(&abs, &final_content, existed).await {
            Ok(_) => ToolResult::ok(format!(
                "patched {} ({} hunk{}, {} bytes, {} lines)",
                abs.display(),
                hunks.len(),
                if hunks.len() == 1 { "" } else { "s" },
                final_content.len(),
                final_content.lines().count()
            )),
            Err(e) => ToolResult::err(format!("write failed: {e}")),
        }
    }
}

struct Hunk {
    old_start: usize,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    new_no_newline: bool,
}

/// Which side of the hunk the last pushed line belongs to; the no-newline
/// marker attaches to it.
#[derive(PartialEq, Eq)]
enum HunkSide {
    Old,
    New,
    Both,
}

fn parse_unified_diff(diff: &str) -> anyhow::Result<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut lines = diff.lines().peekable();
    // Skip headers (--- / +++) until first @@ hunk.
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") {
            break;
        }
        lines.next();
    }
    while let Some(line) = lines.next() {
        if !line.starts_with("@@") {
            continue;
        }
        let (old_start, _old_len, _new_start, _new_len) = parse_hunk_header(line)?;
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();
        let mut new_no_newline = false;
        let mut last_target = HunkSide::Both;
        while let Some(h) = lines.peek() {
            if h.starts_with("@@") {
                break;
            }
            let h = lines.next().unwrap();
            if let Some(rest) = h.strip_prefix(' ') {
                old_lines.push(rest.to_string());
                new_lines.push(rest.to_string());
                last_target = HunkSide::Both;
            } else if let Some(rest) = h.strip_prefix('-') {
                old_lines.push(rest.to_string());
                last_target = HunkSide::Old;
            } else if let Some(rest) = h.strip_prefix('+') {
                new_lines.push(rest.to_string());
                last_target = HunkSide::New;
            } else if h.trim() == "\\ No newline at end of file" {
                // Attaches to the last pushed line: that side lacks a trailing
                // newline. Only the new side affects the patched output.
                if last_target != HunkSide::Old {
                    new_no_newline = true;
                }
            } else if h.is_empty() {
                old_lines.push(String::new());
                new_lines.push(String::new());
                last_target = HunkSide::Both;
            } else {
                // tolerate stray lines
            }
        }
        hunks.push(Hunk {
            old_start,
            old_lines,
            new_lines,
            new_no_newline,
        });
    }
    if hunks.is_empty() {
        anyhow::bail!("no hunks found in diff");
    }
    Ok(hunks)
}

fn parse_hunk_header(line: &str) -> anyhow::Result<(usize, usize, usize, usize)> {
    // @@ -l,s +l,s @@ [optional section text]; scan tokens, not positions.
    let parts: Vec<&str> = line.split_whitespace().collect();
    let old = parts
        .iter()
        .find(|p| p.starts_with('-'))
        .copied()
        .unwrap_or_default();
    let new = parts
        .iter()
        .find(|p| p.starts_with('+'))
        .copied()
        .unwrap_or_default();
    if old.is_empty() || new.is_empty() {
        anyhow::bail!("invalid hunk header: {line}");
    }
    let (os, ol) = parse_range(old.trim_start_matches('-'));
    let (ns, nl) = parse_range(new.trim_start_matches('+'));
    Ok((os, ol, ns, nl))
}

/// Edit distance between two strings (classic two-row DP). Used for
/// tolerance to minor per-line drift in model-authored diffs.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev = (0..=b.len()).collect::<Vec<usize>>();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Normalized similarity in [0.0, 1.0]: 1.0 = identical, 0.0 = unrelated.
fn normalized_similarity(a: &str, b: &str) -> f64 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(a, b) as f64 / max as f64)
}

/// Locate `old` in `result` by content: exact match at the hint, then forward
/// from `cursor`, then unique/nearest match anywhere, then trim-tolerant, and
/// finally per-line fuzzy similarity.
fn locate_old_lines(
    result: &[String],
    old: &[String],
    cursor: usize,
    hint: usize,
) -> Option<usize> {
    let n = old.len();
    if n == 0 || n > result.len() {
        return None;
    }
    let last = result.len() - n;
    let matches_at = |pos: usize, trim: bool| {
        result[pos..pos + n]
            .iter()
            .zip(old)
            .all(|(a, b)| lines_match(a, b, trim))
    };
    if hint <= last && matches_at(hint, false) {
        return Some(hint);
    }
    for pos in cursor..=last {
        if matches_at(pos, false) {
            return Some(pos);
        }
    }
    let mut candidates: Vec<usize> = (0..=last).filter(|&p| matches_at(p, false)).collect();
    if candidates.is_empty() {
        candidates = (0..=last).filter(|&p| matches_at(p, true)).collect();
    }
    if candidates.is_empty() && result.len() <= 50_000 {
        candidates = (0..=last)
            .filter(|&p| fuzzy_window_match(&result[p..p + n], old))
            .collect();
    }
    nearest_to_hint(&candidates, hint)
}

/// Pick the candidate nearest the hint; an equal distance is ambiguous.
fn nearest_to_hint(candidates: &[usize], hint: usize) -> Option<usize> {
    let dist = |p: usize| (p as i64 - hint as i64).abs();
    let min_dist = candidates.iter().map(|&p| dist(p)).min()?;
    let nearest: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&p| dist(p) == min_dist)
        .collect();
    if nearest.len() > 1 {
        None
    } else {
        nearest.first().copied()
    }
}

fn lines_match(a: &str, b: &str, trim: bool) -> bool {
    if trim {
        a.trim_end() == b.trim_end()
    } else {
        a == b
    }
}

fn contains_lines(haystack: &[String], needle: &[String], trim: bool) -> bool {
    let n = needle.len();
    n > 0
        && n <= haystack.len()
        && haystack
            .windows(n)
            .any(|w| w.iter().zip(needle).all(|(a, b)| lines_match(a, b, trim)))
}

/// Fuzzy window match: mean per-line similarity >= 0.8 with a 0.5 floor on
/// each line, so a single identical short line cannot mask unrelated context.
fn fuzzy_window_match(window: &[String], old: &[String]) -> bool {
    if window.len() != old.len() || old.is_empty() {
        return false;
    }
    let mut sum = 0.0;
    for (a, b) in window.iter().zip(old) {
        let s = normalized_similarity(a, b);
        if s < 0.5 {
            return false;
        }
        sum += s;
    }
    (sum / old.len() as f64) >= 0.8
}

fn contains_lines_fuzzy(haystack: &[String], needle: &[String]) -> bool {
    let n = needle.len();
    n > 0 && n <= haystack.len() && haystack.windows(n).any(|w| fuzzy_window_match(w, needle))
}

fn parse_range(s: &str) -> (usize, usize) {
    if let Some((start, len)) = s.split_once(',') {
        (start.parse().unwrap_or(1), len.parse().unwrap_or(0))
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

/// Ask the user a clarifying question (docs/17 — BP cline ask_followup).
/// Execution is special-cased in chat.rs (pauses for user input via IPC).
pub struct AskUser;

#[async_trait]
impl Tool for AskUser {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ask_user".into(),
            description: "Ask the user a clarifying question and wait for their answer. Use when you need more info \
            before proceeding. Provide `options` (2-6 concise choices) when the answer likely comes from \
            a small known set; the user can still type a custom answer. Set `multi_select: true` if \
            several answers apply. Keep questions short and specific; ask one question per call but you \
            may call ask_user multiple times in sequence."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The clarifying question to ask the user." },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Suggested answers the user can pick from. Provide 2-6 concise options when the answer likely comes from a small known set. Always allow the user to type a custom answer too."
                    },
                    "multi_select": {
                        "type": "boolean",
                        "description": "If true, the user may select multiple options. Default false (single choice).",
                        "default": false
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional extra context or hint shown to the user above the options."
                    }
                },
                "required": ["question"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        // Handled in chat.rs via the ask registry; this stub is unreachable.
        let q = arg_str(&args, "question").unwrap_or("");
        ToolResult::err(format!("ask_user must be invoked in a chat turn: {q}"))
    }
}

/// Create/replace the persisted per-chat task list shown to the user (docs/17
/// — BP claude TodoWrite). Rows live in the `tasks` table; execution is
/// special-cased in chat.rs (persists via db::tasks + emits chat:tasks_update).
pub struct TodoWrite;

#[async_trait]
impl Tool for TodoWrite {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "todo_write".into(),
            description: "Create or replace the session task list shown to the user. Send the \
            FULL list every time (this replaces the previous list entirely). Use this when a \
            request has 3+ distinct steps, when the user gives multiple tasks, or when new \
            instructions arrive mid-task. Skip it for single trivial actions.\n\
            Rules:\n\
            - Each item: `content` (imperative, e.g. 'Fix auth bug') and `activeForm` (present \
            continuous, e.g. 'Fixing auth bug', shown while in_progress).\n\
            - Exactly ONE item should be `in_progress` at a time.\n\
            - Mark an item `completed` IMMEDIATELY when its work is done — do not batch completions.\n\
            - Only mark `completed` when the work is fully verified; if blocked, keep it \
            `in_progress` and add a follow-up item.\n\
            - Set an item to `cancelled` when it is no longer relevant.\n\
            - Do not include chores (linting, testing, searching) as separate items.\n\
            - You may call this tool in the same turn as other tool calls."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "minItems": 2,
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "activeForm": { "type": "string" },
                                "status": {
                                    "enum": ["pending", "in_progress", "completed", "cancelled"]
                                }
                            },
                            "required": ["content", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("todo_write must be invoked in a chat turn")
    }
}

/// FTS over sibling chats of the current project (docs/08 — Variant C).
/// Execution is special-cased in chat.rs (needs DB access).
pub struct SearchProjectChats;

#[async_trait]
impl Tool for SearchProjectChats {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_project_chats".into(),
            description: "Full-text search across the other chats of the current project. \
            Returns matching snippets with chat ids."
                .into(),
            parameters: schema(&[("query", "string")], &["query"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("search_project_chats must be invoked in a project chat")
    }
}

/// Read messages of a sibling chat in the same project (docs/08).
/// Execution is special-cased in chat.rs (needs DB access).
pub struct ReadChat;

#[async_trait]
impl Tool for ReadChat {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_chat".into(),
            description: "Read the messages of a sibling chat in the same project. \
            Returns the conversation as plain text."
                .into(),
            parameters: schema(&[("chat_id", "string"), ("limit", "number")], &["chat_id"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("read_chat must be invoked in a project chat")
    }
}

/// LLM-callable tool that loads a project skill's full body from the `.skills/`
/// folder. The skill list is bound per turn; execution is intercepted in chat.rs.
#[allow(dead_code)]
pub struct ConnectSkill {
    skills: Vec<ProjectSkill>,
}

#[allow(dead_code)]
impl ConnectSkill {
    pub fn new(skills: Vec<ProjectSkill>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for ConnectSkill {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        let mut desc = String::from(
            "Load a project skill from the project's .skills folder. Call this with a skill_id to retrieve the skill's full instructions, then follow them. Available skills:",
        );
        if self.skills.is_empty() {
            desc.push_str(" (none)");
        } else {
            for s in &self.skills {
                desc.push_str(&format!("\n- {}: {}", s.id, s.title));
            }
        }
        let ids: Vec<String> = self.skills.iter().map(|s| s.id.clone()).collect();
        let params = if ids.is_empty() {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_id": { "type": "string", "description": "The skill id to load" }
                },
                "required": ["skill_id"]
            })
        } else {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_id": { "type": "string", "enum": ids, "description": "The skill id to load" }
                },
                "required": ["skill_id"]
            })
        };
        ToolSpec {
            name: "connect_skill".into(),
            description: desc,
            parameters: params,
        }
    }
    async fn execute(&self, _args: Value) -> ToolResult {
        ToolResult::err("connect_skill must be invoked in a chat turn")
    }
}

/// Add a memory rule (docs/08). Execution is special-cased in chat.rs.
pub struct AddRule;

#[async_trait]
impl Tool for AddRule {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "add_rule".into(),
            description: "Add a memory rule (a short directive the user asks you to remember). \
            scope: chat (default) | project."
                .into(),
            parameters: schema(&[("text", "string"), ("scope", "string")], &["text"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("add_rule must be invoked in a chat turn")
    }
}

/// Update the text of an existing memory rule (docs/08). Execution is special-cased in chat.rs.
pub struct UpdateRule;

#[async_trait]
impl Tool for UpdateRule {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "update_rule".into(),
            description: "Update the text of an existing memory rule by its id. \
            The id is shown alongside each rule in the system prompt rules block."
                .into(),
            parameters: schema(&[("id", "string"), ("text", "string")], &["id", "text"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("update_rule must be invoked in a chat turn")
    }
}

/// Enable or disable a memory rule by its id (docs/08). Execution is special-cased in chat.rs.
pub struct ToggleRule;

#[async_trait]
impl Tool for ToggleRule {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "toggle_rule".into(),
            description: "Enable or disable a memory rule by its id. \
            Set enabled=true to activate, false to deactivate (the rule stays listed but inactive). \
            The id is shown alongside each rule in the system prompt rules block."
                .into(),
            parameters: schema(&[("id", "string"), ("enabled", "boolean")], &["id", "enabled"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("toggle_rule must be invoked in a chat turn")
    }
}

/// Delete a memory rule by its id (docs/08). Execution is special-cased in chat.rs.
pub struct DeleteRule;

#[async_trait]
impl Tool for DeleteRule {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "delete_rule".into(),
            description: "Permanently delete a memory rule by its id. \
            The id is shown alongside each rule in the system prompt rules block."
                .into(),
            parameters: schema(&[("id", "string")], &["id"]),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("delete_rule must be invoked in a chat turn")
    }
}

const WEB_FETCH_MAX_BODY: usize = 2 * 1024 * 1024;

fn is_text_content_type(ctype: &str) -> bool {
    ctype.contains("html")
        || ctype.starts_with("text/")
        || ctype.is_empty()
        || ctype.contains("json")
        || ctype.contains("xml")
        || ctype.contains("javascript")
        || ctype.contains("csv")
        || ctype.contains("yaml")
}

fn looks_binary(buf: &[u8]) -> bool {
    let n = buf.len().min(1024);
    buf[..n].contains(&0u8)
}

async fn read_body_limited(
    resp: &mut reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), String> {
    let mut buf = Vec::new();
    let mut truncated = false;
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => return Err(format!("read body failed: {e}")),
        };
        let remaining = limit.saturating_sub(buf.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        if chunk.len() <= remaining {
            buf.extend_from_slice(&chunk);
        } else {
            buf.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
    }
    Ok((buf, truncated))
}

/// Fetch a URL over HTTP/HTTPS and return its content as Markdown.
pub struct WebFetch;

#[async_trait]
impl Tool for WebFetch {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_fetch".into(),
            description: "Fetch a URL over HTTP/HTTPS and return its content as Markdown. HTML pages are converted to Markdown; other text content is returned as-is; binary content returns metadata only. Requires user approval.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The absolute http(s) URL to fetch." },
                    "max_length": { "type": "number", "description": "Optional max chars to return (default 64000)." },
                    "format": { "type": "string", "enum": ["markdown", "text", "html"], "description": "Output format (default markdown)." }
                },
                "required": ["url"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(url) = arg_str(&args, "url") else {
            return ToolResult::err("missing 'url'");
        };
        if let Err(e) = ensure_http_url(url) {
            return ToolResult::err(e);
        }
        let max_length = args
            .get("max_length")
            .and_then(|v| v.as_f64())
            .unwrap_or(64_000.0)
            .clamp(1024.0, 512_000.0) as usize;
        let format = arg_str(&args, "format").unwrap_or("markdown");

        let req = http_client().get(url).send();
        let mut resp = match tokio::time::timeout(std::time::Duration::from_secs(35), req).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return ToolResult::err(format!("request failed: {e}")),
            Err(_) => return ToolResult::err("request timed out"),
        };
        let status = resp.status();
        if !status.is_success() {
            let (snippet_bytes, _) = read_body_limited(&mut resp, 2000).await.unwrap_or_default();
            let snippet = String::from_utf8_lossy(&snippet_bytes);
            let mut msg = format!("HTTP {}", status.as_u16());
            if !snippet.trim().is_empty() {
                let mut s = snippet.trim().to_string();
                super::truncate_text(&mut s, 600);
                msg.push('\n');
                msg.push_str(&s);
            }
            return ToolResult::err(msg);
        }
        let ctype = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let final_url = resp.url().to_string();

        let mut title: Option<String> = None;
        let content = if is_text_content_type(&ctype) {
            let (body_bytes, body_truncated) =
                match read_body_limited(&mut resp, WEB_FETCH_MAX_BODY).await {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(e),
                };
            if looks_binary(&body_bytes) {
                let len = body_bytes.len();
                format!(
                    "[binary content, content-type: {ctype}, {len} bytes{}]",
                    if body_truncated { " (truncated)" } else { "" }
                )
            } else {
                let body = String::from_utf8_lossy(&body_bytes);
                if ctype.contains("html") {
                    title = extract_title(&body);
                    if format == "html" {
                        body.into_owned()
                    } else {
                        html_to_markdown(&body)
                    }
                } else {
                    body.into_owned()
                }
            }
        } else {
            let len = resp
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("?");
            format!("[binary content, content-type: {ctype}, content-length: {len}]")
        };

        let mut full = format!(
            "URL: {final_url}\nContent-Type: {ctype}\nStatus: {}\n",
            status.as_u16()
        );
        if let Some(t) = title {
            full.push_str(&format!("Title: {t}\n"));
        }
        full.push('\n');
        full.push_str(&content);
        super::truncate_text(&mut full, max_length);
        ToolResult::ok(full)
    }
}

fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url).ok().and_then(|u| {
        u.host_str()
            .map(|h| h.trim_start_matches("www.").to_lowercase())
    })
}

/// Build the request URL from a provider template containing `{query}`.
fn build_search_url(template: &str, encoded_query: &str) -> String {
    if template.contains("{query}") {
        template.replace("{query}", encoded_query)
    } else if template.contains('?') {
        format!("{template}&q={encoded_query}")
    } else {
        format!("{template}?q={encoded_query}")
    }
}

/// Detect bot-challenge / CAPTCHA pages so we can fall back to the next engine.
fn is_bot_block(html: &str) -> bool {
    let low = html.to_lowercase();
    const MARKERS: &[&str] = &[
        "please complete the following challenge",
        "bots use duckduckgo",
        "squares containing a duck",
        "pardon our interruption",
        "attention required",
        "cf-challenge",
        "are you a robot",
        "unusual traffic from your computer",
        "enable javascript and cookies to continue",
        "access denied",
    ];
    MARKERS.iter().any(|m| low.contains(m))
}

fn is_nav_label(title: &str) -> bool {
    matches!(
        title.to_lowercase().as_str(),
        "more"
            | "news"
            | "images"
            | "maps"
            | "videos"
            | "about"
            | "privacy"
            | "settings"
            | "sign in"
            | "help"
            | "login"
            | "log in"
            | "all"
            | "cached"
            | "feedback"
            | "web"
    )
}

/// Extract result links (title + url + optional snippet) from the page's
/// markdown. Returns empty if fewer than 2 real external links are found.
fn extract_search_results(html: &str, count: usize, engine_host: &str) -> Vec<String> {
    let Some(re) = LINK_RE.as_ref() else {
        return Vec::new();
    };
    let md = html_to_markdown(html);
    let lines: Vec<&str> = md.lines().collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while i < lines.len() && out.len() < count {
        let line = lines[i];
        if let Some(caps) = re.captures(line) {
            let title = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let url = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            if !title.is_empty()
                && !url.is_empty()
                && !is_nav_label(title)
                && host_of(url).as_deref() != Some(engine_host)
                && seen.insert(url.to_string())
            {
                let mut snippet = String::new();
                if i + 1 < lines.len() {
                    let next = lines[i + 1].trim();
                    if !next.is_empty() && !re.is_match(next) {
                        snippet = next.to_string();
                    }
                }
                out.push(if snippet.is_empty() {
                    format!("- [{}]({})", title, url)
                } else {
                    format!("- [{}]({})\n  {}", title, url, snippet)
                });
            }
        }
        i += 1;
    }
    out
}

async fn fetch_search_html(url: &str) -> Result<String, String> {
    let req = search_client()
        .get(url)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Accept-Language", "en-US,en;q=0.9")
        .send();
    let resp = match tokio::time::timeout(std::time::Duration::from_secs(20), req).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("request failed: {e}")),
        Err(_) => return Err("request timed out".into()),
    };
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    resp.text()
        .await
        .map_err(|e| format!("read body failed: {e}"))
}

/// Web search via user-configured providers (Settings → Web Search), with a
/// DuckDuckGo Lite fallback (best-effort, no API key).
pub struct WebSearch;

#[async_trait]
impl Tool for WebSearch {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_search".into(),
            description: "Search the web and return result titles, URLs and snippets as Markdown. Tries configured search-engine providers in order (Settings → Web Search), falling back to the next one on failure. Requires user approval.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "count": { "type": "number", "description": "Max results to return (default 8)." }
                },
                "required": ["query"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(query) = arg_str(&args, "query") else {
            return ToolResult::err("missing 'query'");
        };
        let count = args
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 20) as usize;
        let encoded = percent_encode_query(query);

        let providers: Vec<(String, String)> = match crate::tools::db_pool() {
            Some(pool) => crate::db::models::list_enabled_web_search_providers(pool)
                .await
                .map(|v| v.into_iter().map(|p| (p.title, p.url)).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        // Fallback to a sane default if the DB is unavailable or nothing is enabled.
        let providers: Vec<(String, String)> = if providers.is_empty() {
            vec![(
                "DuckDuckGo Lite".into(),
                "https://lite.duckduckgo.com/lite/?q={query}".into(),
            )]
        } else {
            providers
        };

        let mut notes: Vec<String> = Vec::new();
        for (title, url_tpl) in &providers {
            let url = build_search_url(url_tpl, &encoded);
            let host = host_of(url_tpl).unwrap_or_default();
            match fetch_search_html(&url).await {
                Ok(html) => {
                    if is_bot_block(&html) {
                        notes.push(format!("{title}: blocked (bot challenge)"));
                        continue;
                    }
                    let results = extract_search_results(&html, count, &host);
                    if results.len() < 2 {
                        notes.push(format!("{title}: no results"));
                        continue;
                    }
                    let mut out = format!(
                        "Search: {title}\nQuery: {query}\n\n{}",
                        results.join("\n\n")
                    );
                    super::truncate_text(&mut out, (count * 800).clamp(4_000, 32_000));
                    if !notes.is_empty() {
                        out.push_str("\n\n(Previously tried: ");
                        out.push_str(&notes.join("; "));
                        out.push(')');
                    }
                    return ToolResult::ok(out);
                }
                Err(e) => notes.push(format!("{title}: {e}")),
            }
        }
        ToolResult::err(format!("all search providers failed: {}", notes.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn read_file_and_list_dir() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-tools-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("hello.txt");
        tokio::fs::write(&f, "first line\nsecond match line\n")
            .await
            .unwrap();

        let rf = ReadFile;
        let r = rf.execute(json!({ "path": f.to_string_lossy() })).await;
        assert!(!r.is_error);
        assert!(r.content.contains("second match line"));

        let ld = ListDir;
        let r = ld.execute(json!({ "path": dir.to_string_lossy() })).await;
        assert!(!r.is_error);
        assert!(r.content.contains("hello.txt [file]"));

        let g = Grep;
        let r = g
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "match" }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("hello.txt:2: second match line"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_denies_env() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-env-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join(".env");
        tokio::fs::write(&f, "SECRET=1").await.unwrap();
        let r = ReadFile
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(r.is_error);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_truncates_on_char_boundary() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-trunc-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("multi.txt");
        // 16383 * "а" (2 bytes) = 32766, then "€" (3 bytes) straddles the 32KB
        // boundary — a naive byte-truncate at 32768 would split a multi-byte
        // char and panic. The tool must back up to a char boundary.
        let content = "а".repeat(16383) + "€" + &"x".repeat(16);
        tokio::fs::write(&f, content).await.unwrap();
        let r = ReadFile
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("[truncated"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_rejects_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-rdir-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let r = ReadFile
            .execute(json!({ "path": dir.to_string_lossy() }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("directory"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_rejects_binary() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-bin-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("blob.bin");
        tokio::fs::write(&f, [0x41u8, 0x42, 0x00, 0x43, 0x44])
            .await
            .unwrap();
        let r = ReadFile
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("binary"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn read_file_rejects_invalid_utf8() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-utf8-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("latin1.txt");
        // 0xFF is invalid UTF-8 but a valid Latin-1 byte; no NUL byte.
        tokio::fs::write(&f, [0x41u8, 0xFF, 0x42]).await.unwrap();
        let r = ReadFile
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("UTF-8"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_and_file_info() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-glob-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        tokio::fs::write(dir.join("a.rs"), "fn main(){}")
            .await
            .unwrap();
        tokio::fs::write(dir.join("b.txt"), "hi").await.unwrap();

        let g = Glob;
        let r = g
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "**/*.rs" }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("a.rs"), "glob content: {}", r.content);
        assert!(!r.content.contains("b.txt"));

        let fi = FileInfo;
        let r = fi
            .execute(json!({ "path": dir.join("a.rs").to_string_lossy() }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("file"));
        assert!(r.content.contains("size"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_finds_files_in_dotdir() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globdot-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join(".github/workflows"))
            .await
            .unwrap();
        tokio::fs::write(dir.join(".github/workflows/ci.yml"), "on: push\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("top.rs"), "fn main(){}\n")
            .await
            .unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "**/*.yml" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains(".github/workflows/ci.yml"),
            "content: {}",
            r.content
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_respects_gitignore() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globgi-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join(".git")).await.unwrap();
        tokio::fs::write(dir.join(".gitignore"), "ignored.rs\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("ignored.rs"), "fn a(){}\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("keep.rs"), "fn b(){}\n")
            .await
            .unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "**/*.rs" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("keep.rs"), "content: {}", r.content);
        assert!(
            !r.content.contains("ignored.rs"),
            "gitignored file should be skipped: {}",
            r.content
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_brace_alternation() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globbr-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.rs"), "").await.unwrap();
        tokio::fs::write(dir.join("b.ts"), "").await.unwrap();
        tokio::fs::write(dir.join("c.go"), "").await.unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "*.{rs,ts}" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("a.rs"), "{}", r.content);
        assert!(r.content.contains("b.ts"), "{}", r.content);
        assert!(!r.content.contains("c.go"), "{}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_char_class_negation() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globneg-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.rs"), "").await.unwrap();
        tokio::fs::write(dir.join("b.rs"), "").await.unwrap();
        tokio::fs::write(dir.join("c.txt"), "").await.unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "[!ab].rs" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(r.content, "no matches", "{}", r.content);

        let r2 = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "[!a].rs" }))
            .await;
        assert!(!r2.is_error, "err: {}", r2.content);
        assert!(r2.content.contains("b.rs"), "{}", r2.content);
        assert!(!r2.content.contains("a.rs"), "{}", r2.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_sorted_and_truncated() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globtr-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        for i in 0..150 {
            tokio::fs::write(dir.join(format!("f{:03}.txt", i)), "")
                .await
                .unwrap();
        }
        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "*.txt" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains("stopped at 100 matches"),
            "{}",
            r.content
        );
        let names: Vec<&str> = r.content.lines().filter(|l| l.starts_with('f')).collect();
        assert_eq!(names.len(), 100, "{}", r.content);
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "results should be sorted");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_skips_node_modules() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globnm-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join("node_modules/pkg"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("node_modules/pkg/index.js"), "")
            .await
            .unwrap();
        tokio::fs::write(dir.join("real.js"), "").await.unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "**/*.js" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("real.js"), "{}", r.content);
        assert!(!r.content.contains("node_modules"), "{}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_nonexistent_path_is_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globne-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "**/*" }))
            .await;
        assert!(r.is_error, "expected error: {}", r.content);
        assert!(
            r.content.contains("not a directory") || r.content.contains("not found"),
            "{}",
            r.content
        );
    }

    #[tokio::test]
    async fn glob_pattern_matches_recursively_by_name() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globrec-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join("src/sub"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("src/sub/deep.rs"), "")
            .await
            .unwrap();
        tokio::fs::write(dir.join("root.rs"), "").await.unwrap();

        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "*.rs" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("root.rs"), "{}", r.content);
        assert!(
            r.content.contains("deep.rs"),
            "name-only pattern should match recursively: {}",
            r.content
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn glob_unclosed_class_is_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-globuc-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let r = Glob
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "[unclosed" }))
            .await;
        assert!(r.is_error, "expected error: {}", r.content);
        assert!(r.content.contains("invalid glob"), "{}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_info_regular_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("data.txt");
        tokio::fs::write(&f, "hello world").await.unwrap();

        let r = FileInfo
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("type: file"), "content: {}", r.content);
        assert!(
            r.content.contains("size: 11 bytes"),
            "content: {}",
            r.content
        );
        assert!(r.content.contains("mtime:"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_info_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-dir-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;

        let r = FileInfo
            .execute(json!({ "path": dir.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("type: dir"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_info_resolves_valid_symlink() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-sym-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let target = dir.join("real.txt");
        tokio::fs::write(&target, "x").await.unwrap();
        let link = dir.join("link.txt");
        tokio::fs::symlink(&target, &link).await.unwrap();

        let r = FileInfo
            .execute(json!({ "path": link.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        // A valid symlink is canonicalized by validate_read_path to its in-scope
        // target, so it is reported as the target file — consistent with
        // read_file/list_dir. It is not flagged as a symlink.
        assert!(r.content.contains("type: file"), "content: {}", r.content);
        assert!(
            r.content.contains("size: 1 bytes"),
            "content: {}",
            r.content
        );
        assert!(!r.content.contains("broken:"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_info_broken_symlink_is_not_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-brk-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let link = dir.join("dangling.txt");
        tokio::fs::symlink(dir.join("nope.txt"), &link)
            .await
            .unwrap();

        let r = FileInfo
            .execute(json!({ "path": link.to_string_lossy() }))
            .await;
        assert!(
            !r.is_error,
            "broken symlink must not be an error: {}",
            r.content
        );
        assert!(
            r.content.contains("type: symlink"),
            "content: {}",
            r.content
        );
        assert!(r.content.contains("broken: true"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_info_not_found() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-nf2-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;

        let r = FileInfo
            .execute(json!({ "path": dir.join("totally_missing").to_string_lossy() }))
            .await;
        assert!(r.is_error, "should be error: {}", r.content);
        assert!(r.content.contains("no such file"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_info_not_found_suggests_sibling() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-sug-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        tokio::fs::write(dir.join("readme.md"), "x").await.unwrap();

        // "readme" is a substring of "readme.md" -> suggestion offered
        let r = FileInfo
            .execute(json!({ "path": dir.join("readme").to_string_lossy() }))
            .await;
        assert!(r.is_error, "should be error: {}", r.content);
        assert!(r.content.contains("Did you mean"), "content: {}", r.content);
        assert!(r.content.contains("readme.md"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_info_missing_arg() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = FileInfo.execute(json!({})).await;
        assert!(r.is_error);
        assert!(
            r.content.contains("missing 'path'"),
            "content: {}",
            r.content
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_info_socket() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-fi-sock-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join("sock");
        // binding creates a unix socket file
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let r = FileInfo
            .execute(json!({ "path": path.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("type: socket"), "content: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn set_file_mode_applies_and_masks() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ia-chmod-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("script.sh");
        tokio::fs::write(&f, "#!/bin/sh\n").await.unwrap();

        let t = SetFileMode;
        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "0755" }))
            .await;
        assert!(!r.is_error, "chmod err: {}", r.content);
        let mode = tokio::fs::metadata(&f).await.unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o755);

        // 0o prefix is accepted
        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "0o644" }))
            .await;
        assert!(!r.is_error, "0o err: {}", r.content);
        let mode = tokio::fs::metadata(&f).await.unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o644);

        // setgid bit preserved
        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "2755" }))
            .await;
        assert!(!r.is_error, "setgid err: {}", r.content);
        let mode = tokio::fs::metadata(&f).await.unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o2755);

        // file-type bits are masked off (100644 -> 644 applied)
        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "100644" }))
            .await;
        assert!(!r.is_error, "mask err: {}", r.content);
        let mode = tokio::fs::metadata(&f).await.unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o644);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn set_file_mode_rejects_bad_input() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-chmod-bad-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("x.txt");
        tokio::fs::write(&f, "x").await.unwrap();

        let t = SetFileMode;
        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "999" }))
            .await;
        assert!(r.is_error);
        assert!(
            r.content.contains("invalid octal mode"),
            "got: {}",
            r.content
        );

        let r = t
            .execute(json!({ "path": f.to_string_lossy(), "mode": "   " }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("empty"), "got: {}", r.content);

        // nonexistent path -> clear not-found error
        let r = t
            .execute(json!({ "path": dir.join("nope").to_string_lossy(), "mode": "644" }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("no such file"), "got: {}", r.content);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_roundtrip() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("code.txt");
        tokio::fs::write(&f, "line one\nline two\nline three\n")
            .await
            .unwrap();
        let diff = "--- a/code.txt\n+++ b/code.txt\n@@ -1,3 +1,3 @@\n line one\n-line two\n+line TWO\n line three\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert!(after.contains("line TWO"));
        assert!(!after.contains("line two\n"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_wrong_line_numbers_still_applies() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-wrongline-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("w.txt");
        tokio::fs::write(&f, "one\ntwo\nthree\nfour\nfive\n")
            .await
            .unwrap();
        let diff = "--- a/w.txt\n+++ b/w.txt\n@@ -10,3 +10,3 @@\n two\n-three\n+THREE\n four\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "one\ntwo\nTHREE\nfour\nfive\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_crlf_preserved() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-crlf-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("crlf.txt");
        tokio::fs::write(&f, "alpha\r\nbeta\r\ngamma\r\n")
            .await
            .unwrap();
        let diff =
            "--- a/crlf.txt\n+++ b/crlf.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "alpha\r\nBETA\r\ngamma\r\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_creates_new_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-new-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("created.txt");
        let diff = "--- /dev/null\n+++ b/created.txt\n@@ -0,0 +1,2 @@\n+first\n+second\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "first\nsecond\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_no_newline_at_eof_preserved() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-nonl-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("nonl.txt");
        tokio::fs::write(&f, "a\nb\nc").await.unwrap();
        let diff = "@@ -1,3 +1,3 @@\n a\n b\n-c\n+c modified\n\\ No newline at end of file\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "a\nb\nc modified");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-dir-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let sub = dir.join("subdir");
        tokio::fs::create_dir_all(&sub).await.unwrap();
        let diff = "--- a/subdir\n+++ b/subdir\n@@ -1,1 +1,1 @@\n-x\n+x\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": sub.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("directory"), "got: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_binary() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-bin-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("bin.dat");
        tokio::fs::write(&f, b"hello\0world\n").await.unwrap();
        let diff = "@@ -1,2 +1,2 @@\n-hello\n+HELLO\n world\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("binary"), "got: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_context_not_found_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-miss-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("ctx.txt");
        tokio::fs::write(&f, "x\ny\nz\n").await.unwrap();
        let diff = "@@ -1,3 +1,3 @@\n a\n b\n c\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("not found"), "got: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_all_or_nothing() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-atomic-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("atomic.txt");
        tokio::fs::write(&f, "one\ntwo\nthree\n").await.unwrap();
        let diff = "@@ -1,1 +1,1 @@\n-one\n+ONE\n@@ -5,1 +5,1 @@\n-nine\n+NINE\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "one\ntwo\nthree\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_multiple_hunks_forward() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-multi-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("multi.txt");
        let content: String = (1..=10).map(|i| format!("line{i}\n")).collect();
        tokio::fs::write(&f, content).await.unwrap();
        let diff = "@@ -1,3 +1,3 @@\n line1\n-line2\n+line2 edited\n line3\n@@ -7,3 +7,3 @@\n line7\n-line8\n+line8 edited\n line9\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(
            after,
            "line1\nline2 edited\nline3\nline4\nline5\nline6\nline7\nline8 edited\nline9\nline10\n"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_fuzzy_typo_in_context() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-fuzzy-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("f.rs");
        tokio::fs::write(
            &f,
            "fn main() {\n    let value = 42;\n    println!(\"{}\");\n}\n",
        )
        .await
        .unwrap();
        let diff = "--- a/f.rs\n+++ b/f.rs\n@@ -1,4 +1,4 @@\n fn main() {\n-    let value = 42;\n+    let value = 43;\n     prnitln!(\"{}\");\n }\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(!r.is_error, "patch err: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert!(after.contains("let value = 43;"), "got: {after}");
        assert!(after.contains("prnitln!"), "got: {after}");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_fuzzy_too_different_rejected() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-fuzzysim-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("f.txt");
        tokio::fs::write(&f, "alpha\nbeta\ngamma\n").await.unwrap();
        let diff = "@@ -1,1 +1,1 @@\n-zzzzz\n+replaced\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("not found"), "got: {}", r.content);
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "alpha\nbeta\ngamma\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn apply_patch_fuzzy_ambiguous_rejected() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-patch-fuzzyamb-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let f = dir.join("f.txt");
        tokio::fs::write(&f, "alpha one\nmid\nalpha onx\n")
            .await
            .unwrap();
        let diff = "@@ -2,1 +2,1 @@\n-alpha ona\n+alpha onb\n";
        let ap = ApplyPatch;
        let r = ap
            .execute(json!({ "path": f.to_string_lossy(), "diff": diff }))
            .await;
        assert!(r.is_error);
        assert!(
            r.content.contains("multiple locations"),
            "got: {}",
            r.content
        );
        let after = tokio::fs::read_to_string(&f).await.unwrap();
        assert_eq!(after, "alpha one\nmid\nalpha onx\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn web_fetch_rejects_non_http() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WebFetch
            .execute(json!({ "url": "file:///etc/passwd" }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("http"), "err: {}", r.content);
    }

    #[tokio::test]
    async fn web_fetch_missing_url() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WebFetch.execute(json!({})).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_fetch_rejects_no_host() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WebFetch.execute(json!({ "url": "http://" })).await;
        assert!(r.is_error, "content: {}", r.content);
    }

    #[tokio::test]
    async fn web_fetch_rejects_invalid_url() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WebFetch.execute(json!({ "url": "not a url" })).await;
        assert!(r.is_error);
    }

    #[test]
    fn html_to_markdown_basic() {
        let md = html_to_markdown("<h1>Hi</h1><script>x</script><p>there</p>");
        assert!(md.contains("# Hi"), "md: {md}");
        assert!(md.contains("there"));
        assert!(!md.contains("x"));
    }

    #[test]
    fn percent_encode_query_basic() {
        assert_eq!(percent_encode_query("a b&c"), "a%20b%26c");
        assert_eq!(
            percent_encode_query("hello-world_test.value~1"),
            "hello-world_test.value~1"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn web_fetch_live() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WebFetch
            .execute(json!({ "url": "https://example.com" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.to_lowercase().contains("example domain"),
            "content: {}",
            r.content
        );
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ia-mkdir-{}-{name}", uuid::Uuid::new_v4()))
    }

    fn cleanup(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[tokio::test]
    async fn make_dir_creates_new() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("new");
        let r = MakeDir.execute(json!({ "path": p })).await;
        assert!(!r.is_error);
        assert!(p.is_dir());
        cleanup(&p);
    }

    #[tokio::test]
    async fn make_dir_recursive_creates_parents() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("a/b/c");
        let r = MakeDir.execute(json!({ "path": p })).await;
        assert!(!r.is_error);
        assert!(p.is_dir());
        cleanup(p.parent().unwrap());
    }

    #[tokio::test]
    async fn make_dir_already_exists_is_idempotent() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("exists");
        std::fs::create_dir_all(&p).unwrap();
        let r = MakeDir.execute(json!({ "path": p })).await;
        assert!(!r.is_error);
        assert!(r.content.contains("already exists"));
        cleanup(&p);
    }

    #[tokio::test]
    async fn make_dir_conflict_with_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("file").with_extension("txt");
        std::fs::write(&p, b"x").unwrap();
        let r = MakeDir.execute(json!({ "path": p })).await;
        assert!(r.is_error);
        assert!(r.content.contains("not a directory"));
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn make_dir_non_recursive_requires_parent() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("noparent/child");
        let r = MakeDir
            .execute(json!({ "path": p, "recursive": false }))
            .await;
        assert!(r.is_error);
        // parent dir should not have been created
        assert!(!p.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn make_dir_non_recursive_succeeds_with_parent() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let parent = tmp("hasparent");
        std::fs::create_dir_all(&parent).unwrap();
        let p = parent.join("child");
        let r = MakeDir
            .execute(json!({ "path": p, "recursive": false }))
            .await;
        assert!(!r.is_error);
        assert!(p.is_dir());
        cleanup(&parent);
    }

    #[tokio::test]
    async fn make_dir_empty_path_rejected() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = MakeDir.execute(json!({ "path": "   " })).await;
        assert!(r.is_error);
        assert!(r.content.contains("empty"));
    }

    #[tokio::test]
    async fn write_file_creates_new() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_new.txt");
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "hello\nworld\n" }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("created"));
        assert!(r.content.contains("2 lines"));
        let written = std::fs::read_to_string(&p).unwrap();
        assert_eq!(written, "hello\nworld\n");
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn write_file_overwrites_existing() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_over.txt");
        std::fs::write(&p, "old content").unwrap();
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "new content\n" }))
            .await;
        assert!(!r.is_error);
        assert!(r.content.contains("overwrote"));
        let written = std::fs::read_to_string(&p).unwrap();
        assert_eq!(written, "new content\n");
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn write_file_creates_parent_dirs() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_par/a/b/c.txt");
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "nested" }))
            .await;
        assert!(!r.is_error);
        assert!(p.exists());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "nested");
        cleanup(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn write_file_rejects_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_dir");
        std::fs::create_dir_all(&p).unwrap();
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "x" }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("is a directory"));
        cleanup(&p);
    }

    #[tokio::test]
    async fn write_file_empty_content() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_empty.txt");
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "" }))
            .await;
        assert!(!r.is_error);
        assert!(p.exists());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn write_file_missing_args() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = WriteFile.execute(json!({ "path": "/tmp/x" })).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing 'content'"));

        let r2 = WriteFile.execute(json!({ "content": "x" })).await;
        assert!(r2.is_error);
        assert!(r2.content.contains("missing 'path'"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_file_preserves_permissions() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("wf_perm.sh");
        std::fs::write(&p, "#!/bin/sh\necho hi\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "#!/bin/sh\necho bye\n" }))
            .await;
        assert!(!r.is_error);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "executable permissions should be preserved after overwrite"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn write_file_no_temp_left_behind() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp("wf_clean");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("target.txt");
        let r = WriteFile
            .execute(json!({ "path": p.to_string_lossy(), "content": "data" }))
            .await;
        assert!(!r.is_error);
        // No .tmp files should remain in the directory.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        cleanup(&dir);
    }

    #[tokio::test]
    async fn move_path_renames_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mvsrc.txt");
        let dst = tmp("mvdst.txt");
        tokio::fs::write(&src, "payload").await.unwrap();
        let r = MovePath.execute(json!({ "src": &src, "dst": &dst })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(!src.exists());
        assert_eq!(tokio::fs::read_to_string(&dst).await.unwrap(), "payload");
        let _ = std::fs::remove_file(&dst);
    }

    #[tokio::test]
    async fn move_path_renames_dir() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mvdir-src");
        let dst = tmp("mvdir-dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("child.txt"), "inner").unwrap();
        let r = MovePath.execute(json!({ "src": &src, "dst": &dst })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(!src.exists());
        assert_eq!(
            tokio::fs::read_to_string(dst.join("child.txt"))
                .await
                .unwrap(),
            "inner"
        );
        cleanup(&src);
        cleanup(&dst);
    }

    #[tokio::test]
    async fn move_path_creates_parent_dirs() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let base = tmp("mvparents");
        let src = tmp("mvparents-src.txt");
        let dst = base.join("a/b/c.txt");
        tokio::fs::write(&src, "nested").await.unwrap();
        let r = MovePath.execute(json!({ "src": &src, "dst": &dst })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(tokio::fs::read_to_string(&dst).await.unwrap(), "nested");
        cleanup(&base);
        let _ = std::fs::remove_file(&src);
    }

    #[tokio::test]
    async fn move_path_src_not_found() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mv-missing-src");
        let dst = tmp("mv-missing-dst");
        let r = MovePath.execute(json!({ "src": &src, "dst": &dst })).await;
        assert!(r.is_error);
        assert!(
            r.content.contains("source not found"),
            "content: {}",
            r.content
        );
    }

    #[tokio::test]
    async fn move_path_noop_same_path() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp("mv-noop.txt");
        tokio::fs::write(&p, "same").await.unwrap();
        let r = MovePath.execute(json!({ "src": &p, "dst": &p })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("no-op"), "content: {}", r.content);
        assert_eq!(tokio::fs::read_to_string(&p).await.unwrap(), "same");
        let _ = std::fs::remove_file(&p);
    }

    #[tokio::test]
    async fn move_path_refuses_existing_dst_without_overwrite() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mv-conflict-src.txt");
        let dst = tmp("mv-conflict-dst.txt");
        tokio::fs::write(&src, "src").await.unwrap();
        tokio::fs::write(&dst, "dst").await.unwrap();
        let r = MovePath.execute(json!({ "src": &src, "dst": &dst })).await;
        assert!(r.is_error);
        assert!(
            r.content.contains("destination exists"),
            "content: {}",
            r.content
        );
        assert_eq!(tokio::fs::read_to_string(&dst).await.unwrap(), "dst");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[tokio::test]
    async fn move_path_overwrite_replaces_existing() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mv-overwrite-src.txt");
        let dst = tmp("mv-overwrite-dst.txt");
        tokio::fs::write(&src, "new").await.unwrap();
        tokio::fs::write(&dst, "old").await.unwrap();
        let r = MovePath
            .execute(json!({ "src": &src, "dst": &dst, "overwrite": true }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(!src.exists());
        assert_eq!(tokio::fs::read_to_string(&dst).await.unwrap(), "new");
        let _ = std::fs::remove_file(&dst);
    }

    #[tokio::test]
    async fn move_path_refuses_move_into_itself() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp("mv-self");
        std::fs::create_dir_all(&dir).unwrap();
        let r = MovePath
            .execute(json!({ "src": &dir, "dst": dir.join("child") }))
            .await;
        assert!(r.is_error);
        assert!(
            r.content.contains("own descendant"),
            "content: {}",
            r.content
        );
        cleanup(&dir);
    }

    #[tokio::test]
    async fn move_path_copy_recursive_helper() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let src = tmp("mv-tree-src");
        let dst = tmp("mv-tree-dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        tokio::fs::write(src.join("file1.txt"), "one")
            .await
            .unwrap();
        tokio::fs::write(src.join("sub").join("file2.txt"), "two")
            .await
            .unwrap();
        #[cfg(unix)]
        tokio::fs::symlink("file1.txt", src.join("link"))
            .await
            .unwrap();

        copy_recursive(&src, &dst).await.unwrap();

        assert_eq!(
            tokio::fs::read_to_string(dst.join("file1.txt"))
                .await
                .unwrap(),
            "one"
        );
        assert_eq!(
            tokio::fs::read_to_string(dst.join("sub").join("file2.txt"))
                .await
                .unwrap(),
            "two"
        );
        #[cfg(unix)]
        {
            let lm = tokio::fs::symlink_metadata(dst.join("link")).await.unwrap();
            assert!(lm.file_type().is_symlink());
            assert_eq!(
                tokio::fs::read_link(dst.join("link")).await.unwrap(),
                std::path::Path::new("file1.txt")
            );
        }
        cleanup(&src);
        cleanup(&dst);
    }

    #[tokio::test]
    async fn move_path_missing_args() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = MovePath.execute(json!({})).await;
        assert!(r.is_error);
        let r = MovePath.execute(json!({ "src": "a" })).await;
        assert!(r.is_error);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_success_exit_code() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand.execute(json!({ "command": "echo hello" })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("hello"));
        assert!(r.content.contains("[exit 0]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_nonzero_exit() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand.execute(json!({ "command": "exit 7" })).await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("[exit 7]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_captures_stderr() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand
            .execute(json!({ "command": "echo err >&2" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("err"));
        assert!(r.content.contains("[stderr]"));
    }

    #[tokio::test]
    async fn run_command_missing_command_arg() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand.execute(json!({})).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing"));
    }

    // parse_timeout_ms clamps the arg to >= 1000ms, so a smaller value would
    // never appear in the error message.
    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_timeout_kills() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand
            .execute(json!({ "command": "sleep 30", "timeout_ms": 1000 }))
            .await;
        assert!(r.is_error, "content: {}", r.content);
        assert!(r.content.contains("timed out after 1000ms"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_truncates_on_char_boundary() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        // 40000 two-byte 'é' chars = 80000 bytes of stdout.
        let r = RunCommand
            .execute(json!({ "command": "printf 'é%.0s' $(seq 1 40000)" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.ends_with("…[truncated]"),
            "content: {}",
            r.content
        );
        assert!(
            r.content.len() <= 32 * 1024 + 1024,
            "len: {}",
            r.content.len()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_env_applied() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = RunCommand
            .execute(json!({ "command": "echo $MY_VAR", "env": { "MY_VAR": "fromenv" } }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("fromenv"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_command_cwd_must_be_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-cwd-{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        tokio::fs::write(dir.join("marker.txt"), "x").await.unwrap();
        let r = RunCommand
            .execute(json!({ "command": "ls", "cwd": dir.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("marker.txt"));
        let r = RunCommand
            .execute(json!({ "command": "ls", "cwd": dir.join("nope").to_string_lossy() }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("cwd is not a directory"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_finds_matches_in_dir() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("a.rs"), "fn foo() {}\nfn bar() {}\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("b.txt"), "nothing here\nfoo again\n")
            .await
            .unwrap();

        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "foo" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains("a.rs:1: fn foo() {}"),
            "content: {}",
            r.content
        );
        assert!(r.content.contains("b.txt:2: foo again"));
        assert!(!r.content.contains("bar"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_single_file_path() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep1-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("single.txt");
        tokio::fs::write(&f, "alpha\nbeta\ngamma\n").await.unwrap();

        let r = Grep
            .execute(json!({ "path": f.to_string_lossy(), "pattern": "beta" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains("single.txt:2: beta"),
            "content: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_skips_binary_content() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep2-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        // No extension; contains a null byte → treated as binary and skipped.
        tokio::fs::write(dir.join("blob"), "match\x00more match\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("real.txt"), "match here\n")
            .await
            .unwrap();

        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "match" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("real.txt"), "content: {}", r.content);
        assert!(
            !r.content.contains("blob"),
            "binary file should be skipped: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_invalid_regex_is_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep3-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "(unclosed" }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("invalid regex"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_missing_path_is_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = Grep.execute(json!({ "pattern": "x" })).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing 'path'"));
    }

    #[tokio::test]
    async fn grep_truncates_long_line() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep4-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let long = "match ".repeat(200); // ~1000 chars
        tokio::fs::write(dir.join("long.txt"), format!("{long}\n"))
            .await
            .unwrap();
        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "match" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        let line = r.content.lines().next().unwrap();
        assert!(
            line.ends_with('…'),
            "line should end with ellipsis: {}",
            line
        );
        // "long.txt:1: " prefix (12) + 500 + "…" (1) ≤ ~520; allow some slack.
        assert!(line.len() <= 520, "line too long: {} bytes", line.len());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_cap_and_truncation_footer() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grep5-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        // 60 matching lines in one file → capped at 50 hits + footer.
        let content: String = (0..60).map(|i| format!("hit{i}\n")).collect();
        tokio::fs::write(dir.join("many.txt"), content)
            .await
            .unwrap();
        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "hit" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains("stopped at 50 matches"),
            "missing footer: {}",
            r.content
        );
        let count = r
            .content
            .lines()
            .filter(|l| l.contains("many.txt:"))
            .count();
        assert_eq!(count, 50, "expected 50 hits, got {count}: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_include_filters_by_extension() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grepinc-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join("src")).await.unwrap();
        tokio::fs::write(dir.join("src/a.rs"), "findme\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("src/b.txt"), "findme\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("c.rs"), "findme\n")
            .await
            .unwrap();

        // No slash → matches the file name anywhere in the tree.
        let r = Grep
            .execute(
                json!({ "path": dir.to_string_lossy(), "pattern": "findme", "include": "*.rs" }),
            )
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("a.rs"), "content: {}", r.content);
        assert!(r.content.contains("c.rs"));
        assert!(!r.content.contains("b.txt"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_include_with_slash_matches_relative_path() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grepinc2-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join("src")).await.unwrap();
        tokio::fs::write(dir.join("src/a.rs"), "findme\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("c.rs"), "findme\n")
            .await
            .unwrap();

        // Slash present → matched against the relative path; only src/a.rs.
        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "findme", "include": "src/*.rs" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("src/a.rs"), "content: {}", r.content);
        assert!(!r.content.contains("c.rs"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_include_invalid_glob_is_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grepinc3-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        // An unclosed `[` becomes an invalid regex inside globx.
        let r = Grep
            .execute(
                json!({ "path": dir.to_string_lossy(), "pattern": "x", "include": "[unclosed" }),
            )
            .await;
        assert!(r.is_error, "expected error: {}", r.content);
        assert!(r.content.contains("invalid include glob"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_respects_gitignore() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grepgi-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        // A `.git` dir makes this a repo root so .gitignore is honored.
        tokio::fs::create_dir_all(dir.join(".git")).await.unwrap();
        tokio::fs::write(dir.join(".gitignore"), "ignored.txt\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("ignored.txt"), "secretmatch\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("keep.txt"), "secretmatch\n")
            .await
            .unwrap();

        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "secretmatch" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("keep.txt"), "content: {}", r.content);
        assert!(
            !r.content.contains("ignored.txt"),
            "gitignored file should be skipped: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn grep_skips_sensitive_files() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-grepsens-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join(".env"), "secretmatch\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("keep.txt"), "secretmatch\n")
            .await
            .unwrap();

        let r = Grep
            .execute(json!({ "path": dir.to_string_lossy(), "pattern": "secretmatch" }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(r.content.contains("keep.txt"), "content: {}", r.content);
        assert!(
            !r.content.contains(".env"),
            ".env must not be searched: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_single_replacement() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("a.txt");
        tokio::fs::write(&f, "a\nb\nc\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "b", "new_string": "B" }]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(tokio::fs::read_to_string(&f).await.unwrap(), "a\nB\nc\n");
        assert!(r.content.contains("1 operation"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_multiple_operations() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("multi.rs");
        tokio::fs::write(&f, "fn foo()\nfn bar()\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [
                    { "old_string": "foo()", "new_string": "foo(x)" },
                    { "old_string": "bar()", "new_string": "bar(y)" }
                ]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(
            tokio::fs::read_to_string(&f).await.unwrap(),
            "fn foo(x)\nfn bar(y)\n"
        );
        assert!(r.content.contains("2 operations"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_not_found_error() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": dir.join("missing.txt").to_string_lossy(),
                "operations": [{ "old_string": "a", "new_string": "b" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("no such file"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_old_string_not_unique() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("dup.txt");
        tokio::fs::write(&f, "x\nx\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "x", "new_string": "y" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(
            r.content.contains("matches 2 times"),
            "content: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_old_string_not_found() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("nf.txt");
        tokio::fs::write(&f, "hello\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "missing", "new_string": "x" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("not found"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_empty_old_string() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("empty.txt");
        tokio::fs::write(&f, "hi\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "", "new_string": "x" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("empty"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_noop_rejected() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("noop.txt");
        tokio::fs::write(&f, "foo\nbar\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "foo", "new_string": "foo" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("no-op"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_empty_operations() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("noops.txt");
        tokio::fs::write(&f, "x\n").await.unwrap();

        let r = EditFile
            .execute(json!({ "path": f.to_string_lossy(), "operations": [] }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("empty"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_rejects_directory() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": dir.to_string_lossy(),
                "operations": [{ "old_string": "a", "new_string": "b" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(
            r.content.contains("is a directory"),
            "content: {}",
            r.content
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_crlf_normalization() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("crlf.txt");
        tokio::fs::write(&f, "line one\r\nline two\r\nline three\r\n")
            .await
            .unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "line two", "new_string": "line TWO" }]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(
            tokio::fs::read_to_string(&f).await.unwrap(),
            "line one\r\nline TWO\r\nline three\r\n"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_crlf_new_string_normalized() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("crlf_new.txt");
        tokio::fs::write(&f, "a\r\nb\r\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "b", "new_string": "c\nd" }]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert_eq!(
            tokio::fs::read_to_string(&f).await.unwrap(),
            "a\r\nc\r\nd\r\n"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_atomic_no_temp_left() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("tmpcheck.txt");
        tokio::fs::write(&f, "keep\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "keep", "new_string": "kept" }]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);

        let mut entries = tokio::fs::read_dir(&dir).await.unwrap();
        let mut leftovers = Vec::new();
        while let Ok(Some(e)) = entries.next_entry().await {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp") {
                leftovers.push(name);
            }
        }
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_missing_path_arg() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = EditFile.execute(json!({ "operations": [] })).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing 'path'"));
    }

    #[tokio::test]
    async fn edit_file_missing_operations_arg() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let r = EditFile.execute(json!({ "path": "/tmp/x" })).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing 'operations'"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn edit_file_preserves_permissions() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("edit_perm.sh");
        tokio::fs::write(&f, "#!/bin/sh\necho hi\n").await.unwrap();
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "hi", "new_string": "bye" }]
            }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        let mode = tokio::fs::metadata(&f).await.unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "executable permissions should be preserved after edit"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_rejects_non_utf8() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("latin1.txt");
        tokio::fs::write(&f, [0x41u8, 0xFF, 0x42]).await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "A", "new_string": "B" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("UTF-8"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_rejects_large_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("big.txt");
        tokio::fs::write(&f, "a".repeat(17 * 1024 * 1024))
            .await
            .unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [{ "old_string": "a", "new_string": "b" }]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("too large"), "content: {}", r.content);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn edit_file_all_or_nothing() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("ia-edit-{}", uuid::Uuid::new_v4()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("atomic.txt");
        tokio::fs::write(&f, "a\nb\nc\n").await.unwrap();

        let r = EditFile
            .execute(json!({
                "path": f.to_string_lossy(),
                "operations": [
                    { "old_string": "a", "new_string": "A" },
                    { "old_string": "NOPE", "new_string": "X" }
                ]
            }))
            .await;
        assert!(r.is_error);
        assert!(r.content.contains("not found"), "content: {}", r.content);
        assert!(r.content.contains("operation 2"), "content: {}", r.content);
        assert_eq!(
            tokio::fs::read_to_string(&f).await.unwrap(),
            "a\nb\nc\n",
            "file must be unchanged when a later operation fails"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    // PATH_ROOTS / TRASH_MODE are process-wide globals read by every tool that
    // calls validate_*_path / is_protected_root / trash_mode. Tests that mutate
    // them (delete/unlink) and tests that rely on the empty default must all
    // hold this lock, otherwise a concurrent root-setting test makes a reader's
    // /tmp path fail with "path outside project roots".
    static PATH_ROOTS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn tmp_del(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ia-del-{}-{name}", uuid::Uuid::new_v4()))
    }

    fn cleanup_del(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    #[tokio::test]
    async fn delete_file_removes_regular_file() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp_del("file");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let f = dir.join("hello.txt");
        tokio::fs::write(&f, "hello").await.unwrap();

        let r = DeletePath
            .execute(json!({ "path": f.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(!f.exists());
        cleanup_del(&dir);
    }

    #[tokio::test]
    async fn delete_dir_removes_recursively() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp_del("dir");
        let sub = dir.join("sub");
        tokio::fs::create_dir_all(&sub).await.unwrap();
        tokio::fs::write(sub.join("inner.txt"), "inner")
            .await
            .unwrap();

        let r = DeletePath
            .execute(json!({ "path": dir.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(!dir.exists());
    }

    #[tokio::test]
    async fn trash_mode_roundtrip() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        crate::tools::set_trash_mode(true);
        assert!(crate::tools::trash_mode());
        crate::tools::set_trash_mode(false);
        assert!(!crate::tools::trash_mode());
    }

    #[tokio::test]
    async fn delete_permanent_dir_uses_remove_dir_all() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        crate::tools::set_trash_mode(false);
        let dir = tmp_del("perm-dir");
        let sub = dir.join("sub");
        tokio::fs::create_dir_all(&sub).await.unwrap();
        tokio::fs::write(sub.join("inner.txt"), "inner")
            .await
            .unwrap();

        let r = DeletePath
            .execute(json!({ "path": dir.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(
            r.content.contains("deleted directory"),
            "content: {}",
            r.content
        );
        assert!(!dir.exists());

        crate::tools::set_trash_mode(false);
        crate::tools::clear_path_roots();
        cleanup_del(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_symlink_unlinks_link_not_target() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp_del("symlink");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("target.txt");
        tokio::fs::write(&target, "keep").await.unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let r = DeletePath
            .execute(json!({ "path": link.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(link.symlink_metadata().is_err());
        assert!(target.exists());
        cleanup_del(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_dangling_symlink_succeeds() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp_del("dangling");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let link = dir.join("dangling");
        std::os::unix::fs::symlink(dir.join("no-such-target"), &link).unwrap();
        assert!(link.symlink_metadata().is_ok());

        let r = DeletePath
            .execute(json!({ "path": link.to_string_lossy() }))
            .await;
        assert!(!r.is_error, "err: {}", r.content);
        assert!(link.symlink_metadata().is_err());
        cleanup_del(&dir);
    }

    #[tokio::test]
    async fn delete_refuses_project_root() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let root = tmp_del("root");
        tokio::fs::create_dir_all(&root).await.unwrap();
        crate::tools::set_path_roots(vec![root.clone()]);
        let canon = std::fs::canonicalize(&root).unwrap();

        let r = DeletePath
            .execute(json!({ "path": canon.to_string_lossy() }))
            .await;
        assert!(r.is_error, "content: {}", r.content);
        assert!(r.content.contains("project root"), "content: {}", r.content);

        crate::tools::clear_path_roots();
        cleanup_del(&root);
    }

    #[tokio::test]
    async fn delete_refuses_dir_with_env() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let dir = tmp_del("env");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join(".env"), "SECRET=1")
            .await
            .unwrap();

        let r = DeletePath
            .execute(json!({ "path": dir.to_string_lossy() }))
            .await;
        assert!(r.is_error, "content: {}", r.content);
        assert!(r.content.contains("denylisted"), "content: {}", r.content);
        assert!(dir.join(".env").exists());
        cleanup_del(&dir);
    }

    #[tokio::test]
    async fn delete_missing_path_errors() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let p = tmp_del("missing").join("nope.txt");

        let r = DeletePath
            .execute(json!({ "path": p.to_string_lossy() }))
            .await;
        assert!(r.is_error, "content: {}", r.content);
        assert!(r.content.contains("not found"), "content: {}", r.content);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validate_unlink_refuses_escaping_symlink_without_auth() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let root = tmp_del("esc-root");
        let out = tmp_del("esc-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&out).await.unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&out, &link).unwrap();
        crate::tools::set_path_roots(vec![root.clone()]);

        let r = crate::tools::validate_unlink_path(&link.to_string_lossy(), false);
        assert!(r.is_err(), "expected refusal, got {:?}", r);
        assert!(
            r.unwrap_err().contains("outside project roots"),
            "unexpected error"
        );

        crate::tools::clear_path_roots();
        cleanup_del(&root);
        cleanup_del(&out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validate_unlink_allows_escaping_symlink_with_auth() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let root = tmp_del("esc-root");
        let out = tmp_del("esc-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&out).await.unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&out, &link).unwrap();
        crate::tools::set_path_roots(vec![root.clone()]);

        let r = crate::tools::validate_unlink_path(&link.to_string_lossy(), true);
        assert!(r.is_ok(), "expected Ok, got {:?}", r);
        let got = r.unwrap();
        assert!(
            got.ends_with("link"),
            "must return the link itself: {}",
            got.display()
        );
        let still_symlink = std::fs::symlink_metadata(&got)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        assert!(
            still_symlink,
            "returned path must be the symlink, not its target"
        );

        crate::tools::clear_path_roots();
        cleanup_del(&root);
        cleanup_del(&out);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn validate_unlink_leaf_outside_roots_always_rejected() {
        let _lock = PATH_ROOTS_LOCK.lock().await;
        let root = tmp_del("esc-root");
        let out = tmp_del("esc-outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&out).await.unwrap();
        let link = out.join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        crate::tools::set_path_roots(vec![root.clone()]);

        let r = crate::tools::validate_unlink_path(&link.to_string_lossy(), true);
        assert!(
            r.is_err(),
            "leaf outside roots must be rejected, got {:?}",
            r
        );
        assert!(
            r.unwrap_err().contains("outside project roots"),
            "unexpected error"
        );

        crate::tools::clear_path_roots();
        cleanup_del(&root);
        cleanup_del(&out);
    }

    #[test]
    fn build_search_url_replaces_placeholder() {
        assert_eq!(
            build_search_url("https://x.com/?q={query}", "a%20b"),
            "https://x.com/?q=a%20b"
        );
        assert_eq!(
            build_search_url("https://x.com/search", "a"),
            "https://x.com/search?q=a"
        );
        assert_eq!(
            build_search_url("https://x.com/s?lang=en", "a"),
            "https://x.com/s?lang=en&q=a"
        );
    }

    #[test]
    fn is_bot_block_detects_challenge() {
        assert!(is_bot_block(
            "Please complete the following challenge to confirm"
        ));
        assert!(is_bot_block("Unfortunately, bots use DuckDuckGo too."));
        assert!(!is_bot_block(
            "<html><body>normal results page</body></html>"
        ));
    }

    #[test]
    fn extract_search_results_parses_markdown_links() {
        // Block-level wrappers match real engine markup (results are not bare
        // inline anchors), so htmd emits each link on its own line.
        let html = "<div><a class=\"r\" href=\"https://example.com/page1\">First Result</a>\
                    <p>Snippet one text here</p></div>\
                    <div><a class=\"r\" href=\"https://example.com/page2\">Second Result</a>\
                    <p>Snippet two text here</p></div>";
        let md_results = extract_search_results(html, 8, "search.engine");
        // htmd converts anchors to markdown links; we expect at least 2 entries
        // with titles and urls (snippet alignment is best-effort).
        assert!(
            md_results
                .iter()
                .any(|r| r.contains("First Result") && r.contains("https://example.com/page1")),
            "missing first result, got: {md_results:?}"
        );
        assert!(
            md_results
                .iter()
                .any(|r| r.contains("Second Result") && r.contains("https://example.com/page2")),
            "missing second result, got: {md_results:?}"
        );
        assert!(md_results.len() >= 2, "got: {md_results:?}");
    }
}
