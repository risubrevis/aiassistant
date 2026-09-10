use async_trait::async_trait;
use base64::Engine as _;
use htmd::HtmlToMarkdown;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::ModelRef;
use crate::projects::ProjectSkill;
use crate::providers::{self, ChatMessage, CompleteEvent, CompleteRequest, ContentPart, Provider};

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

/// Chrome-desktop request headers for search-engine navigations; versions are
/// kept in sync with the default `web_search.user_agent` (Chrome 152). They
/// stay static when the user overrides the UA (known limitation).
const SEARCH_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";
const SEARCH_SEC_CH_UA: &str =
    "\"Chromium\";v=\"152\", \"Google Chrome\";v=\"152\", \"Not?A_Brand\";v=\"24\"";

fn search_client(ws: &crate::config::WebSearch) -> reqwest::Client {
    crate::net::apply(
        reqwest::Client::builder()
            .user_agent(&ws.user_agent)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_millis(ws.timeout_ms)),
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
    // Forward-slash relative paths keep tool output consistent across OSes
    // (matches the globx matcher, which normalizes separators).
    let rel = p
        .strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/");
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
        #[cfg(unix)]
        {
            tokio::fs::symlink(&target, dst).await?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            // Windows has no portable async symlink primitive and creating
            // symlinks needs privileges; copy the resolved target instead.
            let abs_target = if target.is_absolute() {
                target
            } else {
                src.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(&target)
            };
            return Box::pin(copy_recursive(&abs_target, dst)).await;
        }
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
        #[cfg(unix)]
        {
            if let Err(e) = tokio::fs::symlink(&target, dst).await {
                return ToolResult::err(format!("recreate symlink failed: {e}"));
            }
        }
        #[cfg(not(unix))]
        {
            // Windows: copy the resolved target instead of recreating the link.
            let abs_target = if target.is_absolute() {
                target
            } else {
                src.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(&target)
            };
            if let Err(e) = copy_recursive(&abs_target, dst).await {
                return ToolResult::err(format!("copy symlink target failed: {e}"));
            }
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
        // Emit forward-slash relative paths so output is consistent across
        // platforms (matches the globx matcher, which normalizes separators).
        let rel = p
            .strip_prefix(root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
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

/// Persistent per-chat notes ("memory") the LLM can save/update/delete/list.
/// Rows live via db::memory; execution is special-cased in chat.rs (emits
/// chat:memory_update). Shared across a project's chats when in a project.
pub struct Memory;

#[async_trait]
impl Tool for Memory {
    fn category(&self) -> ToolCategory {
        ToolCategory::Interaction
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "memory".into(),
            description: "Persistent notes (memory) scoped to the current chat; when the chat \
            belongs to a project, memory is shared across all chats of that project. Actions: \
            `save` (create a new note), `update` (modify an existing note by id), `delete` \
            (remove a note by id), `list` (read all current notes).\n\
            Use it when the user asks you to remember something, and proactively to persist \
            important context, user preferences, facts or instructions worth keeping across \
            turns.\n\
            `content` is the note text — keep it concise and self-contained; `category` is an \
            optional short tag such as \"preference\", \"fact\", \"instruction\" or \"context\"."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["save", "update", "delete", "list"] },
                    "id": { "type": "string", "description": "memory note id (for update/delete)" },
                    "content": { "type": "string", "description": "note text (for save/update)" },
                    "category": { "type": "string", "description": "optional short tag" }
                },
                "required": ["action"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let _ = args;
        ToolResult::err("memory tool requires chat context")
    }
}

/// LLM-callable tool that loads a project skill's full body from the project's
/// `.agents/skills/` folder (or the legacy `.skills/` fallback). The skill list
/// is bound per turn; execution is intercepted in chat.rs.
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
            "Load a project skill from the project's .agents/skills folder. Call this with a skill_id to retrieve the skill's full instructions, then follow them. Available skills:",
        );
        if self.skills.is_empty() {
            desc.push_str(" (none)");
        } else {
            for s in &self.skills {
                desc.push_str(&format!("\n- {}: {}", s.id, s.description));
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

/// Inspect an image and return a textual description. Delegates to a dedicated
/// vision model when one is configured (`vision_ref`); otherwise uses the chat's
/// main model. The model calls this with a file `path` (project image) or
/// `attachment_id` (chat attachment) plus a `prompt`.
pub struct AnalyzeImage {
    vision_ref: Option<ModelRef>,
    main_pcfg: crate::config::Provider,
    main_model: String,
}

impl AnalyzeImage {
    /// `vision_ref` — a dedicated vision model to delegate to (when configured).
    /// Falls back to the turn's main model (`main_pcfg`/`main_model`) when `None`.
    pub fn new(
        vision_ref: Option<ModelRef>,
        main_pcfg: crate::config::Provider,
        main_model: String,
    ) -> Self {
        Self {
            vision_ref,
            main_pcfg,
            main_model,
        }
    }

    fn mime_for_ext(ext: &str) -> &'static str {
        match ext.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    }
}

#[async_trait]
impl Tool for AnalyzeImage {
    fn category(&self) -> ToolCategory {
        ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "analyze_image".into(),
            description: "Analyze an image using a vision-capable model and return its textual \
            description. Use this to inspect images you cannot see directly (e.g. image files in \
            the project, or chat attachments when the main model cannot view them inline). Pass \
            either `path` (a file path within the project/attached directory) or `attachment_id` \
            (for a chat-attached image), plus an optional `prompt` describing what to look for. \
            Exactly one of `path` / `attachment_id` must be provided."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to an image file within the project or attached directory" },
                    "attachment_id": { "type": "string", "description": "ID of a chat-attached image (shown in the attachment annotation)" },
                    "prompt": { "type": "string", "description": "What to ask about the image (defaults to a general description)" }
                }
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let path = arg_str(&args, "path");
        let attachment_id = arg_str(&args, "attachment_id");
        let prompt = arg_str(&args, "prompt")
            .unwrap_or("Describe this image in detail.")
            .to_string();

        // Resolve image bytes + mime type from exactly one source.
        let (media_type, data) = match (path, attachment_id) {
            (Some(p), None) => {
                let abs = match validate_read_path(p) {
                    Ok(a) => a,
                    Err(e) => return ToolResult::err(e),
                };
                let bytes = match tokio::fs::read(&abs).await {
                    Ok(b) => b,
                    Err(e) => return ToolResult::err(format!("read failed: {e}")),
                };
                let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("");
                let mime = Self::mime_for_ext(ext).to_string();
                if !mime.starts_with("image/") {
                    return ToolResult::err(format!("file does not look like an image ({mime})"));
                }
                (mime, bytes)
            }
            (None, Some(id)) => {
                let Some(pool) = super::db_pool() else {
                    return ToolResult::err("database unavailable");
                };
                let att = match crate::db::attachments::get(pool, id).await {
                    Ok(Some(a)) => a,
                    Ok(None) => return ToolResult::err(format!("attachment not found: {id}")),
                    Err(e) => return ToolResult::err(format!("attachment lookup failed: {e}")),
                };
                if !att.is_image {
                    return ToolResult::err(format!(
                        "attachment is not an image: {}",
                        att.file_name
                    ));
                }
                let bytes = match tokio::fs::read(&att.storage_path).await {
                    Ok(b) => b,
                    Err(e) => return ToolResult::err(format!("read failed: {e}")),
                };
                (att.mime_type, bytes)
            }
            (Some(_), Some(_)) => {
                return ToolResult::err("provide either `path` or `attachment_id`, not both");
            }
            (None, None) => {
                return ToolResult::err("missing `path` or `attachment_id`");
            }
        };

        const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
        if data.len() > MAX_IMAGE_BYTES {
            return ToolResult::err(format!(
                "image too large ({} bytes); limit is 20MB",
                data.len()
            ));
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);

        // Resolve the target provider/model: the dedicated vision model when
        // configured, otherwise the turn's main model.
        let (pcfg, model) = match &self.vision_ref {
            Some(vref) => {
                let Some(pool) = super::db_pool() else {
                    return ToolResult::err("database unavailable");
                };
                let pcfg =
                    match crate::db::providers::get_provider(pool, &vref.provider).await {
                        Ok(Some(r)) => r.to_config_provider(),
                        _ => return ToolResult::err(
                            "vision model provider not found (reconfigure in Settings → Models)",
                        ),
                    };
                let model = match crate::db::providers::get_model(pool, &vref.model).await {
                    Ok(Some(m)) => m.name,
                    _ => {
                        return ToolResult::err(
                            "vision model not found (reconfigure in Settings → Models)",
                        )
                    }
                };
                (pcfg, model)
            }
            None => (self.main_pcfg.clone(), self.main_model.clone()),
        };
        if providers::build(&pcfg).is_none() {
            return ToolResult::err(format!("provider kind '{}' not supported", pcfg.kind));
        }

        let req = CompleteRequest {
            model,
            messages: vec![ChatMessage {
                role: "user".into(),
                content: String::new(),
                tool_call_id: None,
                tool_calls: None,
                parts: Some(vec![
                    ContentPart::Text { text: prompt },
                    ContentPart::Image { media_type, data: encoded },
                ]),
            }],
            system: Some(
                "You are a vision analysis assistant. Examine the provided image and answer the \
                request concisely and accurately. Report only what you can determine from the image."
                    .to_string(),
            ),
            temperature: None,
            max_tokens: None,
            tools: None,
            thinking: None,
            thinking_effort: None,
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel::<CompleteEvent>(64);
        let provider: Box<dyn Provider + Send> =
            providers::build(&pcfg).expect("provider build checked above");
        let err_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let err_slot_tx = err_slot.clone();
        let stream_task = tauri::async_runtime::spawn(async move {
            if let Err(e) = provider.stream_complete(req, tx).await {
                let msg = format!("{e}");
                tracing::error!("image analyze stream error: {msg}");
                if let Ok(mut g) = err_slot_tx.lock() {
                    *g = Some(msg);
                }
            }
        });
        let mut text = String::new();
        while let Some(ev) = rx.recv().await {
            if let CompleteEvent::BlockDelta { text: Some(t), .. } = ev {
                text.push_str(&t);
            }
        }
        let _ = stream_task.await;

        let text = text.trim();
        if text.is_empty() {
            let stream_err = err_slot.lock().ok().and_then(|mut g| g.take());
            return ToolResult::err(match stream_err {
                Some(e) => format!(
                    "image analysis failed: {e}. If the main model does not support vision, \
                    enable a separate vision model in Settings → Models."
                ),
                None => "image analysis returned an empty response".to_string(),
            });
        }
        ToolResult::ok(text.to_string())
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

/// Last two DNS labels of a host — a coarse "registrable domain" guess that's
/// good enough for the common search-engine TLDs (.com/.org/…). Used to group
/// a search engine's own subdomains (blog.mojeek.com, brave.com, …) so they're
/// filtered out of result lists and throttled together.
fn registrable_domain(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    match parts.len() {
        n if n >= 2 => parts[n - 2..].join("."),
        _ => host.to_string(),
    }
}

/// True if `url` belongs to the search engine itself (same registrable domain).
/// Engines' own nav/footer/subdomain links must not leak into results.
fn is_engine_self_link(url: &str, engine_host: &str) -> bool {
    match host_of(url) {
        Some(h) => registrable_domain(&h) == registrable_domain(engine_host),
        None => false,
    }
}

static TAG_RE: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").ok());

/// Strip HTML tags and decode a few common entities, collapsing whitespace.
fn strip_tags(s: &str) -> String {
    let no_tags: String = match TAG_RE.as_ref() {
        Some(re) => re.replace_all(s, "").into_owned(),
        None => s.to_string(),
    };
    let decoded = no_tags
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// DuckDuckGo wraps every result URL in a redirect:
/// `//duckduckgo.com/l/?uddg=<percent-encoded real URL>&rut=…`. Unwrap it to
/// the real target; non-DDG hrefs are returned unchanged (with `//` normalized
/// to `https://`).
fn unwrap_ddg_redirect(href: &str) -> String {
    let full = match href.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => href.to_string(),
    };
    let Ok(parsed) = reqwest::Url::parse(&full) else {
        return full;
    };
    let host = parsed.host_str().unwrap_or("").to_lowercase();
    if !(host == "duckduckgo.com" || host.ends_with(".duckduckgo.com"))
        || !parsed.path().starts_with("/l/")
    {
        return full;
    }
    for (k, v) in parsed.query_pairs() {
        if k == "uddg" {
            return v.into_owned();
        }
    }
    full
}

static DDG_ANCHOR_RE: std::sync::LazyLock<Option<Regex>> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*href="([^"]*uddg=[^"]*)"[^>]*>(.*?)</a>"#).ok()
});
static DDG_SNIPPET_HTML_RE: std::sync::LazyLock<Option<Regex>> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*class="result__snippet"[^>]*>(.*?)</a>"#).ok()
});
static DDG_SNIPPET_LITE_RE: std::sync::LazyLock<Option<Regex>> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?is)<td\b[^>]*class="result-snippet"[^>]*>(.*?)</td>"#).ok()
});

/// DuckDuckGo result anchors use protocol-relative hrefs wrapped in a
/// `uddg=` redirect — the generic markdown-link scanner misses them entirely.
/// Parse them straight from the HTML instead.
fn extract_ddg_results(html: &str, count: usize) -> Vec<String> {
    let Some(anch) = DDG_ANCHOR_RE.as_ref() else {
        return Vec::new();
    };
    let anchors: Vec<(String, String)> = anch
        .captures_iter(html)
        .filter_map(|c| {
            let href = c.get(1)?.as_str().to_string();
            let title = strip_tags(c.get(2)?.as_str());
            Some((href, title))
        })
        .collect();
    if anchors.is_empty() {
        return Vec::new();
    }
    let snippets: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        if let Some(re) = DDG_SNIPPET_HTML_RE.as_ref() {
            v = re
                .captures_iter(html)
                .map(|c| strip_tags(c.get(1).map(|m| m.as_str()).unwrap_or("")))
                .collect();
        }
        if v.is_empty() {
            if let Some(re) = DDG_SNIPPET_LITE_RE.as_ref() {
                v = re
                    .captures_iter(html)
                    .map(|c| strip_tags(c.get(1).map(|m| m.as_str()).unwrap_or("")))
                    .collect();
            }
        }
        v
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, (href, title)) in anchors.iter().enumerate() {
        if out.len() >= count {
            break;
        }
        let title = title.trim();
        if title.is_empty() || is_nav_label(title) {
            continue;
        }
        let url = unwrap_ddg_redirect(href);
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        let snippet = snippets.get(i).map(|s| s.trim()).filter(|s| !s.is_empty());
        out.push(match snippet {
            Some(s) => format!("- [{}]({})\n  {}", title, url, s),
            None => format!("- [{}]({})", title, url),
        });
    }
    out
}

/// Extract result links (title + url + optional snippet) from the page's
/// markdown. Returns empty if fewer than 2 real external links are found.
fn extract_search_results(html: &str, count: usize, engine_host: &str) -> Vec<String> {
    let eng = engine_host.to_lowercase();
    if eng == "duckduckgo.com" || eng.ends_with(".duckduckgo.com") {
        let ddg = extract_ddg_results(html, count);
        if ddg.len() >= 2 {
            return ddg;
        }
    }
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
                && !is_engine_self_link(url, engine_host)
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

/// Minimum gap between consecutive requests to the same search engine
/// (registrable domain). The LLM sometimes fires parallel `web_search` calls;
/// without throttling, engines like Brave reply HTTP 429.
static SEARCH_HOST_THROTTLE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
const SEARCH_HOST_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1100);

async fn throttle_search_host(reg_domain: &str) {
    if reg_domain.is_empty() {
        return;
    }
    let wait = {
        let mut map = SEARCH_HOST_THROTTLE.lock().unwrap();
        let now = std::time::Instant::now();
        let reserved = map.get(reg_domain).copied().unwrap_or(now);
        let wait = reserved.saturating_duration_since(now);
        map.insert(
            reg_domain.to_string(),
            now + wait + SEARCH_HOST_MIN_INTERVAL,
        );
        wait
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

fn build_search_request(ws: &crate::config::WebSearch, url: &str) -> reqwest::RequestBuilder {
    let mut req = search_client(ws)
        .get(url)
        .header("Accept", SEARCH_ACCEPT)
        .header("sec-ch-ua", SEARCH_SEC_CH_UA)
        .header("sec-ch-ua-mobile", "?0")
        .header("sec-ch-ua-platform", "\"Windows\"")
        .header("sec-fetch-dest", "document")
        .header("sec-fetch-mode", "navigate")
        .header("sec-fetch-site", "none")
        .header("sec-fetch-user", "?1")
        .header("upgrade-insecure-requests", "1");
    let lang = ws.accept_language.trim();
    if !lang.is_empty() {
        req = req.header("Accept-Language", lang);
    }
    for line in ws.extra_headers.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() {
                req = req.header(k, v);
            }
        }
    }
    req
}

async fn fetch_search_html(url: &str) -> Result<String, String> {
    let ws = match crate::config::load() {
        Ok(cfg) => cfg.web_search,
        Err(_) => crate::config::WebSearch::default(),
    };
    // reqwest auto-injects `Accept-Encoding: gzip, deflate, br` and decompresses
    // the response transparently (features enabled in Cargo.toml).
    let reg_domain = host_of(url)
        .map(|h| registrable_domain(&h))
        .unwrap_or_default();
    throttle_search_host(&reg_domain).await;

    let mut attempts = 0u8;
    loop {
        let resp = match tokio::time::timeout(
            std::time::Duration::from_millis(ws.timeout_ms),
            build_search_request(&ws, url).send(),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return Err(format!("request failed: {e}")),
            Err(_) => return Err("request timed out".into()),
        };
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempts == 0 {
            // Honor a short Retry-After once, then fall through to the next engine.
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|&s| s <= 5)
                .map(std::time::Duration::from_secs);
            if let Some(d) = wait {
                tokio::time::sleep(d).await;
                attempts += 1;
                continue;
            }
            return Err("HTTP 429 (rate limited)".into());
        }
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        return resp
            .text()
            .await
            .map_err(|e| format!("read body failed: {e}"));
    }
}

/// Timeout for API search requests: reuses the scrape `web_search.timeout_ms`.
fn search_timeout_ms() -> u64 {
    match crate::config::load() {
        Ok(cfg) => cfg.web_search.timeout_ms,
        Err(_) => crate::config::WebSearch::default().timeout_ms,
    }
}

/// Plain HTTP client for JSON search APIs (no browser impersonation needed).
fn api_client(timeout_ms: u64) -> reqwest::Client {
    crate::net::apply(
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_millis(timeout_ms)),
    )
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
}

/// Fully rendered HTTP request for an API search call.
struct ApiRequestSpec {
    method: &'static str,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

/// Hardcoded request specs for the built-in API presets.
fn build_preset_request(
    kind: &str,
    key: &str,
    query: &str,
    encoded: &str,
    count: usize,
) -> Result<ApiRequestSpec, String> {
    match kind {
        "brave_api" => Ok(ApiRequestSpec {
            method: "GET",
            url: format!(
                "https://api.search.brave.com/res/v1/web/search?q={encoded}&count={count}"
            ),
            headers: vec![
                ("X-Subscription-Token".to_string(), key.to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            body: None,
        }),
        "tavily_api" => Ok(ApiRequestSpec {
            method: "POST",
            url: "https://api.tavily.com/search".to_string(),
            headers: vec![
                ("Authorization".to_string(), format!("Bearer {key}")),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: Some(serde_json::json!({ "query": query, "max_results": count }).to_string()),
        }),
        "serper_api" => Ok(ApiRequestSpec {
            method: "POST",
            url: "https://google.serper.dev/search".to_string(),
            headers: vec![
                ("X-API-KEY".to_string(), key.to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: Some(serde_json::json!({ "q": query, "num": count }).to_string()),
        }),
        "exa_api" => Ok(ApiRequestSpec {
            method: "POST",
            url: "https://api.exa.ai/search".to_string(),
            headers: vec![
                ("x-api-key".to_string(), key.to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: Some(serde_json::json!({ "query": query, "numResults": count }).to_string()),
        }),
        _ => Err(format!("unknown API preset: {kind}")),
    }
}

/// Follow a dotted path (e.g. "web.results") through JSON objects and return
/// the array at the leaf. Empty path = the root itself as an array.
fn json_path_array<'a>(root: &'a Value, path: &str) -> Option<&'a Vec<Value>> {
    let path = path.trim();
    if path.is_empty() {
        return root.as_array();
    }
    let mut parts = path.split('.');
    let last = parts.next_back().unwrap_or("");
    let mut cur = root;
    for part in parts {
        cur = cur.get(part)?;
    }
    cur.get(last).and_then(|v| v.as_array())
}

/// Parse a preset API response body into formatted result lines.
fn parse_preset_results(kind: &str, body: &str, count: usize) -> Vec<String> {
    let (path, title_field, url_field, snippet_field): (&str, &str, &str, &str) = match kind {
        "brave_api" => ("web.results", "title", "url", "description"),
        "tavily_api" => ("results", "title", "url", "content"),
        "serper_api" => ("organic", "title", "link", "snippet"),
        "exa_api" => ("results", "title", "url", "text"),
        _ => return Vec::new(),
    };
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(arr) = json_path_array(&root, path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        if out.len() >= count {
            break;
        }
        let title = item
            .get(title_field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let url = item
            .get(url_field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = match kind {
            // Brave descriptions can contain HTML tags.
            "brave_api" => strip_tags(
                item.get(snippet_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ),
            // Exa: `text` when present, otherwise `summary`.
            "exa_api" => item
                .get("text")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string(),
            _ => item
                .get(snippet_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
        };
        out.push(if snippet.is_empty() {
            format!("- [{title}]({url})")
        } else {
            format!("- [{title}]({url})\n  {snippet}")
        });
    }
    out
}

/// Substitute `"{query}"` / `"{count}"` tokens (including the surrounding
/// quotes) with JSON-encoded values, then validate the result parses as JSON.
fn render_custom_body(template: &str, query: &str, count: usize) -> Result<String, String> {
    let substituted = template
        .replace("\"{query}\"", &serde_json::json!(query).to_string())
        .replace("\"{count}\"", &serde_json::json!(count).to_string());
    let value: Value =
        serde_json::from_str(&substituted).map_err(|e| format!("invalid body template: {e}"))?;
    Ok(value.to_string())
}

/// Parse a custom API response into formatted result lines using the
/// provider's field mapping.
fn parse_custom_results(
    body: &str,
    results_path: &str,
    title_field: &str,
    url_field: &str,
    snippet_field: &str,
) -> Vec<String> {
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(arr) = json_path_array(&root, results_path) else {
        return Vec::new();
    };
    let title_field = title_field.trim();
    let url_field = url_field.trim();
    let snippet_field = snippet_field.trim();
    let mut out = Vec::new();
    for item in arr {
        let title = item
            .get(title_field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let url = item
            .get(url_field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if title.is_empty() || url.is_empty() {
            continue;
        }
        let snippet = item
            .get(snippet_field)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        out.push(if snippet.is_empty() {
            format!("- [{title}]({url})")
        } else {
            format!("- [{title}]({url})\n  {snippet}")
        });
    }
    out
}

/// Send one API request with host throttling and a single 429 retry (short
/// `Retry-After` only), mirroring the scrape path's politeness rules.
async fn send_api_request(
    client: reqwest::Client,
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<String>,
    timeout_ms: u64,
) -> Result<reqwest::Response, String> {
    let reg_domain = host_of(url)
        .map(|h| registrable_domain(&h))
        .unwrap_or_default();
    throttle_search_host(&reg_domain).await;

    let mut attempts = 0u8;
    loop {
        let mut req = match method.to_ascii_uppercase().as_str() {
            "POST" => client.post(url),
            _ => client.get(url),
        };
        for (k, v) in &headers {
            match (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                (Ok(name), Ok(value)) => req = req.header(name, value),
                _ => return Err(format!("invalid header: {k}")),
            }
        }
        if let Some(b) = &body {
            req = req.body(b.clone());
        }
        let resp =
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), req.send())
                .await
            {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return Err(format!("request failed: {e}")),
                Err(_) => return Err("request timed out".into()),
            };
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempts == 0 {
            // Honor a short Retry-After once, then give up for this provider.
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|&s| s <= 5)
                .map(std::time::Duration::from_secs);
            if let Some(d) = wait {
                tokio::time::sleep(d).await;
                attempts += 1;
                continue;
            }
            return Err("HTTP 429 (rate limited)".into());
        }
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        return Ok(resp);
    }
}

/// Call one of the built-in JSON search API presets and return formatted lines.
async fn fetch_api_preset(
    p: &crate::db::models::WebSearchProvider,
    query: &str,
    count: usize,
    encoded: &str,
) -> Result<Vec<String>, String> {
    let kind = p.kind.as_str();
    let key = crate::secrets::get_web_search_api_key(&p.id)
        .ok_or_else(|| "no API key set".to_string())?;
    let count = count.min(20);
    let spec = build_preset_request(kind, &key, query, encoded, count)?;
    let timeout_ms = search_timeout_ms();
    let resp = send_api_request(
        api_client(timeout_ms),
        spec.method,
        &spec.url,
        spec.headers,
        spec.body,
        timeout_ms,
    )
    .await?;
    let body = resp
        .text()
        .await
        .map_err(|e| format!("read body failed: {e}"))?;
    Ok(parse_preset_results(kind, &body, count))
}

/// Call a user-configured JSON search API and return formatted lines.
async fn fetch_api_custom(
    p: &crate::db::models::WebSearchProvider,
    query: &str,
    count: usize,
    encoded: &str,
) -> Result<Vec<String>, String> {
    let method_upper = p.api_method.trim().to_ascii_uppercase();
    let method: &str = if method_upper.is_empty() {
        "GET"
    } else {
        &method_upper
    };
    let url = build_search_url(&p.url, encoded);
    let mut headers: Vec<(String, String)> = Vec::new();
    let scheme = p.auth_scheme.trim().to_ascii_lowercase();
    match scheme.as_str() {
        "bearer" => {
            let key = crate::secrets::get_web_search_api_key(&p.id)
                .ok_or_else(|| "no API key set".to_string())?;
            headers.push(("Authorization".into(), format!("Bearer {key}")));
        }
        "header" => {
            let key = crate::secrets::get_web_search_api_key(&p.id)
                .ok_or_else(|| "no API key set".to_string())?;
            let name = p.auth_header.trim();
            if name.is_empty() {
                return Err("auth header name is empty".into());
            }
            headers.push((name.to_string(), key));
        }
        // "none" (or unset): no auth header.
        _ => {}
    }
    let body = if method == "POST" && !p.body_template.trim().is_empty() {
        Some(render_custom_body(p.body_template.trim(), query, count)?)
    } else {
        None
    };
    if body.is_some() {
        headers.push(("Content-Type".into(), "application/json".into()));
    }
    let timeout_ms = search_timeout_ms();
    let resp = send_api_request(
        api_client(timeout_ms),
        method,
        &url,
        headers,
        body,
        timeout_ms,
    )
    .await?;
    let text = resp
        .text()
        .await
        .map_err(|e| format!("read body failed: {e}"))?;
    let mut results = parse_custom_results(
        &text,
        &p.results_path,
        &p.title_field,
        &p.url_field,
        &p.snippet_field,
    );
    results.truncate(count);
    Ok(results)
}

/// Built-in fallback provider when no providers are configured/enabled.
fn default_scrape_provider() -> crate::db::models::WebSearchProvider {
    crate::db::models::WebSearchProvider {
        id: String::new(),
        title: "DuckDuckGo Lite".into(),
        url: "https://lite.duckduckgo.com/lite/?q={query}".into(),
        enabled: 1,
        position: 0,
        created_at: 0,
        updated_at: 0,
        kind: "scrape".into(),
        api_method: String::new(),
        auth_scheme: String::new(),
        auth_header: String::new(),
        body_template: String::new(),
        results_path: String::new(),
        title_field: String::new(),
        url_field: String::new(),
        snippet_field: String::new(),
        has_key: 0,
    }
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

        let providers: Vec<crate::db::models::WebSearchProvider> = match crate::tools::db_pool() {
            Some(pool) => crate::db::models::list_enabled_web_search_providers(pool)
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        // Fallback to a sane default if the DB is unavailable or nothing is enabled.
        let providers = if providers.is_empty() {
            vec![default_scrape_provider()]
        } else {
            providers
        };

        let mut notes: Vec<String> = Vec::new();
        for p in &providers {
            let title = &p.title;
            let outcome = match p.kind.as_str() {
                "brave_api" | "tavily_api" | "serper_api" | "exa_api" => {
                    fetch_api_preset(p, query, count, &encoded).await
                }
                "custom_api" => fetch_api_custom(p, query, count, &encoded).await,
                _ => {
                    let url = build_search_url(&p.url, &encoded);
                    let host = host_of(&p.url).unwrap_or_default();
                    match fetch_search_html(&url).await {
                        Ok(html) if is_bot_block(&html) => {
                            Err("blocked (bot challenge)".to_string())
                        }
                        Ok(html) => Ok(extract_search_results(&html, count, &host)),
                        Err(e) => Err(e),
                    }
                }
            };
            match outcome {
                Ok(results) if results.len() >= 2 => {
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
                Ok(_) => notes.push(format!("{title}: no results")),
                Err(e) => notes.push(format!("{title}: {e}")),
            }
        }
        ToolResult::err(format!("all search providers failed: {}", notes.join("; ")))
    }
}

static WEB_HOOK_PLACEHOLDER_RE: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"\{\{\s*([A-Za-z0-9_.-]+)\s*\}\}").ok());

/// Render a web hook template. `{{payload}}`, `{{secret}}` and `{{variables.KEY}}`
/// are substituted; any other placeholder renders as empty so template
/// scaffolding never leaks into the outgoing request.
fn render_template(
    tpl: &str,
    payload: Option<&str>,
    variables: Option<&Value>,
    secret: Option<&str>,
) -> String {
    let Some(re) = WEB_HOOK_PLACEHOLDER_RE.as_ref() else {
        return tpl.to_string();
    };
    re.replace_all(tpl, |caps: &regex::Captures| {
        match caps.get(1).unwrap().as_str() {
            "payload" => payload.unwrap_or("").to_string(),
            "secret" => secret.unwrap_or("").to_string(),
            name if name.starts_with("variables.") => variables
                .and_then(|v| v.get(&name["variables.".len()..]))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        }
    })
    .into_owned()
}

/// Host of the hook URL as configured (placeholders sanitized to `x` before
/// parsing, so a placeholder in the path/query does not break the parse while
/// a placeholder in the host yields a sentinel that can never match a real one).
fn configured_host(raw_url: &str) -> Option<String> {
    let re = WEB_HOOK_PLACEHOLDER_RE.as_ref()?;
    let sanitized = re.replace_all(raw_url, "x");
    reqwest::Url::parse(sanitized.trim())
        .ok()?
        .host_str()
        .map(|h| h.to_lowercase())
}

fn url_host(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.to_lowercase())
}

fn rendered_hook_headers(
    row: &crate::db::web_hooks::WebHookRow,
    payload: Option<&str>,
    variables: Option<&Value>,
    secret: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    if row.headers.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&row.headers)
        .map_err(|e| format!("invalid headers JSON for web hook '{}': {e}", row.name))?;
    Ok(parsed
        .into_iter()
        .filter_map(|(k, v)| {
            v.as_str()
                .map(|s| (k, render_template(s, payload, variables, secret)))
        })
        .collect())
}

const WEB_HOOK_MAX_BODY: usize = 32 * 1024;

/// Shared HTTP execution for the `web_hook_run` tool and the Settings test
/// button. Renders URL/headers/body, applies auth (secret from the OS keyring),
/// sends the request and reads the response body (up to 32KB).
pub async fn execute_web_hook(
    row: &crate::db::web_hooks::WebHookRow,
    payload: Option<&str>,
    variables: Option<&Value>,
    extra_headers: Option<&Value>,
) -> Result<(reqwest::StatusCode, String, String), String> {
    let secret = crate::secrets::get_webhook_secret(&row.id);
    let auth_type = row.auth_type.trim().to_lowercase();
    let needs_secret = matches!(
        auth_type.as_str(),
        "bearer" | "basic" | "api_key_header" | "api_key_query"
    );
    if needs_secret && secret.is_none() {
        return Err(format!(
            "web hook '{}' requires a secret (set it in Settings → Web Hooks)",
            row.name
        ));
    }

    let rendered_url = render_template(&row.url, payload, variables, secret.as_deref());
    ensure_http_url(&rendered_url)?;
    let configured = configured_host(&row.url).ok_or_else(|| "invalid hook URL".to_string())?;
    let final_host = url_host(&rendered_url).ok_or_else(|| "invalid URL".to_string())?;
    if configured != final_host {
        return Err("web hook host mismatch: variables must not change the host".into());
    }

    let mut url = reqwest::Url::parse(&rendered_url).map_err(|e| format!("invalid URL: {e}"))?;
    if auth_type == "api_key_query" {
        url.query_pairs_mut()
            .append_pair(&row.auth_param_name, secret.as_deref().unwrap_or(""));
    }

    let mut headers = reqwest::header::HeaderMap::new();
    let reserved_auth_header = match auth_type.as_str() {
        "bearer" => {
            let value = reqwest::header::HeaderValue::from_str(&format!(
                "Bearer {}",
                secret.as_deref().unwrap_or("")
            ))
            .map_err(|e| format!("invalid auth header: {e}"))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            Some("authorization".to_string())
        }
        "basic" => {
            let creds = format!("{}:{}", row.auth_username, secret.as_deref().unwrap_or(""));
            let encoded = base64::engine::general_purpose::STANDARD.encode(creds);
            let value = reqwest::header::HeaderValue::from_str(&format!("Basic {encoded}"))
                .map_err(|e| format!("invalid auth header: {e}"))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            Some("authorization".to_string())
        }
        "api_key_header" => {
            let name = row.auth_header_name.trim();
            if name.is_empty() {
                return Err(format!(
                    "web hook '{}' uses api_key_header auth but has no auth_header_name",
                    row.name
                ));
            }
            let hname = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| format!("invalid auth header name '{name}': {e}"))?;
            let value = reqwest::header::HeaderValue::from_str(secret.as_deref().unwrap_or(""))
                .map_err(|e| format!("invalid auth header value: {e}"))?;
            headers.insert(hname, value);
            Some(name.to_lowercase())
        }
        _ => None,
    };

    let hook_headers = rendered_hook_headers(row, payload, variables, secret.as_deref())?;
    for (k, v) in hook_headers.into_iter().chain(
        extra_headers
            .and_then(|v| v.as_object())
            .map(|extra| {
                extra
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            })
            .into_iter()
            .flatten(),
    ) {
        if reserved_auth_header
            .as_deref()
            .map(|r| k.to_lowercase() == r)
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) else {
            continue;
        };
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&v) {
            headers.insert(name, value);
        }
    }

    let method = row.method.trim().to_uppercase();
    let body: Option<String> = if !row.body_template.trim().is_empty() {
        Some(render_template(
            &row.body_template,
            payload,
            variables,
            secret.as_deref(),
        ))
    } else if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        payload.map(|p| p.to_string())
    } else {
        None
    };

    let timeout_ms = row.timeout_ms.max(1000) as u64;
    let client = crate::net::apply(
        reqwest::Client::builder().timeout(std::time::Duration::from_millis(timeout_ms)),
    )
    .build()
    .unwrap_or_else(|_| reqwest::Client::new());

    let mut req = match method.as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        "HEAD" => client.head(url),
        other => return Err(format!("unsupported HTTP method: {other}")),
    };
    req = req.headers(headers);
    if let Some(b) = body {
        req = req.body(b);
    }

    let mut resp = match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms + 5000),
        req.send(),
    )
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("request failed: {e}")),
        Err(_) => return Err("request timed out".into()),
    };
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (bytes, _) = read_body_limited(&mut resp, WEB_HOOK_MAX_BODY).await?;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    Ok((status, ctype, body))
}

/// List user-configured web hooks so the model knows what it can call.
pub struct WebHookList;

#[async_trait]
impl Tool for WebHookList {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Readonly
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_hook_list".into(),
            description: "List user-configured web hooks (Settings → Web Hooks) available to call via web_hook_run. Returns each hook's name, HTTP method, description and the payload it expects. No network call.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_inactive": { "type": "boolean", "description": "If true, also list inactive hooks. Default: false." }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(pool) = crate::tools::db_pool() else {
            return ToolResult::ok("No web hooks configured.");
        };
        let include_inactive = args
            .get("include_inactive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let hooks = if include_inactive {
            crate::db::web_hooks::list(pool).await.unwrap_or_default()
        } else {
            crate::db::web_hooks::list_active(pool)
                .await
                .unwrap_or_default()
        };
        if hooks.is_empty() {
            return ToolResult::ok("No web hooks configured.");
        }
        let mut out = String::from("Web hooks (call via web_hook_run with `name`):\n");
        for h in &hooks {
            let desc = if h.description.trim().is_empty() {
                "<no description>"
            } else {
                h.description.trim()
            };
            let status = if h.is_active == 0 { " [inactive]" } else { "" };
            out.push_str(&format!(
                "- {} [{}]{}: {}\n",
                h.name, h.method, status, desc
            ));
        }
        out.push_str("Pass `payload` (string) and optional `variables` (object) to web_hook_run. If the hook has a body_template, it is rendered with {{payload}} and {{variables.KEY}}; otherwise payload is sent as the raw body.");
        super::truncate_text(&mut out, 8000);
        ToolResult::ok(out)
    }
}

/// Invoke a user-configured web hook by name.
pub struct WebHookRun;

#[async_trait]
impl Tool for WebHookRun {
    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_hook_run".into(),
            description: "Invoke a user-configured web hook by name (see web_hook_list). Sends an HTTP request to the preconfigured endpoint with auth from the OS keyring. The URL/headers/body_template are defined in Settings → Web Hooks; pass `payload` (string) and optional `variables` (object) to fill template placeholders. Requires user approval.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Hook name (slug) from web_hook_list." },
                    "payload": { "type": "string", "description": "Main content to send (rendered into {{payload}} or used as raw body)." },
                    "variables": { "type": "object", "description": "Named values for {{variables.KEY}} placeholders in URL/headers/body_template.", "additionalProperties": { "type": "string" } },
                    "headers": { "type": "object", "description": "Extra request headers (cannot override the hook's auth header).", "additionalProperties": { "type": "string" } }
                },
                "required": ["name"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(name) = arg_str(&args, "name") else {
            return ToolResult::err("missing 'name'");
        };
        let Some(pool) = crate::tools::db_pool() else {
            return ToolResult::err("web hooks unavailable");
        };
        let row = match crate::db::web_hooks::get_by_name(pool, name).await {
            Ok(Some(r)) => r,
            Ok(None) => return ToolResult::err(format!("unknown web hook: {name}")),
            Err(e) => return ToolResult::err(e.to_string()),
        };
        if row.is_active == 0 {
            return ToolResult::err(format!("web hook '{name}' is inactive"));
        }
        let payload = arg_str(&args, "payload");
        let variables = args.get("variables");
        let extra_headers = args.get("headers");
        match execute_web_hook(&row, payload, variables, extra_headers).await {
            Ok((status, ctype, body)) => {
                if !status.is_success() {
                    let mut snippet = body.trim().to_string();
                    super::truncate_text(&mut snippet, 2000);
                    let mut msg = format!("HTTP {}", status.as_u16());
                    if !snippet.is_empty() {
                        msg.push('\n');
                        msg.push_str(&snippet);
                    }
                    return ToolResult::err(msg);
                }
                if body.trim().is_empty() {
                    return ToolResult::ok(format!("HTTP {} (empty body)", status.as_u16()));
                }
                let mut out = format!("HTTP {}", status.as_u16());
                if !ctype.is_empty() {
                    out.push_str(&format!("\nContent-Type: {ctype}"));
                }
                out.push('\n');
                out.push_str(&body);
                super::truncate_text(&mut out, 16000);
                ToolResult::ok(out)
            }
            Err(e) => ToolResult::err(e),
        }
    }
}

/// Create a new web hook (Settings → Web Hooks) callable via `web_hook_run`.
/// The secret cannot be set through this tool — if auth requires one, tell the
/// user to set it in Settings → Web Hooks.
pub struct WebHookAdd;

#[async_trait]
impl Tool for WebHookAdd {
    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_hook_add".into(),
            description: "Create a new web hook that can be invoked via web_hook_run. \
            The hook is saved to Settings → Web Hooks. The secret (for bearer/basic/api_key auth) \
            cannot be set through this tool — ask the user to set it manually in Settings → Web Hooks. \
            Requires user approval."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique slug identifying the hook (used in web_hook_run)." },
                    "title": { "type": "string", "description": "Human-readable title. Defaults to the name." },
                    "description": { "type": "string", "description": "Description shown to the model — explain what the hook does and what payload it expects." },
                    "method": { "type": "string", "description": "HTTP method. Default: POST.", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] },
                    "url": { "type": "string", "description": "HTTP(S) URL. May contain {placeholder} placeholders." },
                    "headers": { "type": "string", "description": "JSON object of request headers. Values may contain {placeholder} placeholders." },
                    "body_template": { "type": "string", "description": "Request body template with {placeholder} placeholders. Leave empty to send the raw payload." },
                    "auth_type": { "type": "string", "description": "Authentication type. Default: none.", "enum": ["none", "bearer", "basic", "api_key_header", "api_key_query"] },
                    "auth_username": { "type": "string", "description": "Username for Basic auth." },
                    "auth_header_name": { "type": "string", "description": "Header name for api_key_header auth." },
                    "auth_param_name": { "type": "string", "description": "Query parameter name for api_key_query auth." },
                    "timeout_ms": { "type": "integer", "description": "Request timeout in milliseconds. Default: 30000." },
                    "is_active": { "type": "boolean", "description": "Whether the hook is active (callable). Default: true." }
                },
                "required": ["name", "url"],
                "additionalProperties": false
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(pool) = crate::tools::db_pool() else {
            return ToolResult::err("web hooks unavailable");
        };
        let name = match arg_str(&args, "name") {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return ToolResult::err("missing or empty 'name'"),
        };
        let url = match arg_str(&args, "url") {
            Some(u) if !u.trim().is_empty() => u.trim().to_string(),
            _ => return ToolResult::err("missing or empty 'url'"),
        };
        match crate::db::web_hooks::get_by_name(pool, &name).await {
            Ok(Some(_)) => return ToolResult::err(format!("web hook '{name}' already exists")),
            Ok(None) => {}
            Err(e) => return ToolResult::err(e.to_string()),
        }
        if let Err(e) = ensure_http_url(&url) {
            return ToolResult::err(e);
        }
        let method = arg_str(&args, "method")
            .unwrap_or("POST")
            .trim()
            .to_uppercase();
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        ) {
            return ToolResult::err(format!("invalid HTTP method: {method}"));
        }
        let headers = arg_str(&args, "headers").unwrap_or("").to_string();
        if !headers.trim().is_empty() {
            if serde_json::from_str::<serde_json::Map<String, Value>>(&headers).is_err() {
                return ToolResult::err("headers must be a valid JSON object");
            }
        }
        let auth_type = arg_str(&args, "auth_type")
            .unwrap_or("none")
            .trim()
            .to_lowercase();
        if !matches!(
            auth_type.as_str(),
            "none" | "bearer" | "basic" | "api_key_header" | "api_key_query"
        ) {
            return ToolResult::err(format!("invalid auth_type: {auth_type}"));
        }
        let input = crate::db::web_hooks::WebHookInput {
            title: arg_str(&args, "title").unwrap_or(&name).to_string(),
            name: name.clone(),
            description: arg_str(&args, "description").unwrap_or("").to_string(),
            method,
            url,
            headers,
            body_template: arg_str(&args, "body_template").unwrap_or("").to_string(),
            auth_type,
            auth_username: arg_str(&args, "auth_username").unwrap_or("").to_string(),
            auth_header_name: arg_str(&args, "auth_header_name").unwrap_or("").to_string(),
            auth_param_name: arg_str(&args, "auth_param_name").unwrap_or("").to_string(),
            timeout_ms: args
                .get("timeout_ms")
                .and_then(|v| v.as_i64())
                .unwrap_or(30_000),
            is_active: args
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        };
        match crate::db::web_hooks::create(pool, input).await {
            Ok(row) => {
                let mut msg = format!(
                    "Created web hook '{}' [{}] {}.\nname: {}\ntitle: {}\ndescription: {}\nurl: {}\nauth: {}",
                    row.name,
                    row.method,
                    if row.is_active != 0 { "(active)" } else { "(inactive)" },
                    row.name,
                    row.title,
                    row.description,
                    row.url,
                    row.auth_type
                );
                let needs_secret = matches!(
                    row.auth_type.as_str(),
                    "bearer" | "basic" | "api_key_header" | "api_key_query"
                );
                if needs_secret {
                    msg.push_str("\n\nNote: this hook uses authentication that requires a secret. The secret cannot be set via this tool — ask the user to set it in Settings → Web Hooks.");
                }
                ToolResult::ok(msg)
            }
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}

/// Modify an existing web hook by name. Only provided fields are updated.
pub struct WebHookModify;

#[async_trait]
impl Tool for WebHookModify {
    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_hook_modify".into(),
            description: "Modify an existing web hook by its current name. Only the fields you provide are updated; omitted fields keep their current values. To rename the hook, set `new_name`. Requires user approval."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Current name (slug) of the hook to modify." },
                    "new_name": { "type": "string", "description": "New slug name (optional, for renaming)." },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "method": { "type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"] },
                    "url": { "type": "string" },
                    "headers": { "type": "string", "description": "JSON object of request headers." },
                    "body_template": { "type": "string" },
                    "auth_type": { "type": "string", "enum": ["none", "bearer", "basic", "api_key_header", "api_key_query"] },
                    "auth_username": { "type": "string" },
                    "auth_header_name": { "type": "string" },
                    "auth_param_name": { "type": "string" },
                    "timeout_ms": { "type": "integer" },
                    "is_active": { "type": "boolean" }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(pool) = crate::tools::db_pool() else {
            return ToolResult::err("web hooks unavailable");
        };
        let name = match arg_str(&args, "name") {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return ToolResult::err("missing 'name' (current name of the hook to modify)"),
        };
        let existing = match crate::db::web_hooks::get_by_name(pool, &name).await {
            Ok(Some(r)) => r,
            Ok(None) => return ToolResult::err(format!("web hook '{name}' not found")),
            Err(e) => return ToolResult::err(e.to_string()),
        };
        let new_name = arg_str(&args, "new_name")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(existing.name.clone());
        if new_name != existing.name {
            match crate::db::web_hooks::get_by_name(pool, &new_name).await {
                Ok(Some(other)) if other.id != existing.id => {
                    return ToolResult::err(format!("web hook '{new_name}' already exists"));
                }
                Ok(_) => {}
                Err(e) => return ToolResult::err(e.to_string()),
            }
        }
        let method = arg_str(&args, "method")
            .map(|s| s.trim().to_uppercase())
            .unwrap_or(existing.method.clone());
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        ) {
            return ToolResult::err(format!("invalid HTTP method: {method}"));
        }
        let url = arg_str(&args, "url")
            .map(|s| s.trim().to_string())
            .unwrap_or(existing.url.clone());
        if let Err(e) = ensure_http_url(&url) {
            return ToolResult::err(e);
        }
        let headers = arg_str(&args, "headers")
            .unwrap_or(&existing.headers)
            .to_string();
        if !headers.trim().is_empty() {
            if serde_json::from_str::<serde_json::Map<String, Value>>(&headers).is_err() {
                return ToolResult::err("headers must be a valid JSON object");
            }
        }
        let auth_type = arg_str(&args, "auth_type")
            .map(|s| s.trim().to_lowercase())
            .unwrap_or(existing.auth_type.clone());
        if !matches!(
            auth_type.as_str(),
            "none" | "bearer" | "basic" | "api_key_header" | "api_key_query"
        ) {
            return ToolResult::err(format!("invalid auth_type: {auth_type}"));
        }
        let input = crate::db::web_hooks::WebHookInput {
            title: arg_str(&args, "title")
                .unwrap_or(&existing.title)
                .to_string(),
            name: new_name,
            description: arg_str(&args, "description")
                .unwrap_or(&existing.description)
                .to_string(),
            method,
            url,
            headers,
            body_template: arg_str(&args, "body_template")
                .unwrap_or(&existing.body_template)
                .to_string(),
            auth_type,
            auth_username: arg_str(&args, "auth_username")
                .unwrap_or(&existing.auth_username)
                .to_string(),
            auth_header_name: arg_str(&args, "auth_header_name")
                .unwrap_or(&existing.auth_header_name)
                .to_string(),
            auth_param_name: arg_str(&args, "auth_param_name")
                .unwrap_or(&existing.auth_param_name)
                .to_string(),
            timeout_ms: args
                .get("timeout_ms")
                .and_then(|v| v.as_i64())
                .unwrap_or(existing.timeout_ms),
            is_active: args
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(existing.is_active != 0),
        };
        match crate::db::web_hooks::update(pool, &existing.id, input).await {
            Ok(row) => {
                let mut msg = format!(
                    "Updated web hook '{}' → name='{}' [{}] {}.\nurl: {}\nauth: {}",
                    name,
                    row.name,
                    row.method,
                    if row.is_active != 0 {
                        "(active)"
                    } else {
                        "(inactive)"
                    },
                    row.url,
                    row.auth_type
                );
                let needs_secret = matches!(
                    row.auth_type.as_str(),
                    "bearer" | "basic" | "api_key_header" | "api_key_query"
                );
                if needs_secret && row.has_secret == 0 {
                    msg.push_str("\n\nNote: this hook uses authentication that requires a secret, but no secret is set. Ask the user to set it in Settings → Web Hooks.");
                }
                ToolResult::ok(msg)
            }
            Err(e) => ToolResult::err(e.to_string()),
        }
    }
}

/// Delete a web hook by name. Also clears its secret from the OS keyring.
pub struct WebHookDelete;

#[async_trait]
impl Tool for WebHookDelete {
    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "web_hook_delete".into(),
            description: "Delete a web hook by its name (slug). Also removes its secret from the OS keyring. Requires user approval."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name (slug) of the hook to delete." }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        let Some(pool) = crate::tools::db_pool() else {
            return ToolResult::err("web hooks unavailable");
        };
        let name = match arg_str(&args, "name") {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => return ToolResult::err("missing 'name'"),
        };
        let row = match crate::db::web_hooks::get_by_name(pool, &name).await {
            Ok(Some(r)) => r,
            Ok(None) => return ToolResult::err(format!("web hook '{name}' not found")),
            Err(e) => return ToolResult::err(e.to_string()),
        };
        let _ = crate::secrets::delete_webhook_secret(&row.id);
        match crate::db::web_hooks::delete(pool, &row.id).await {
            Ok(_) => ToolResult::ok(format!("Deleted web hook '{name}'.")),
            Err(e) => ToolResult::err(e.to_string()),
        }
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

    #[test]
    fn registrable_domain_basic() {
        assert_eq!(registrable_domain("blog.mojeek.com"), "mojeek.com");
        assert_eq!(registrable_domain("search.brave.com"), "brave.com");
        assert_eq!(registrable_domain("mojeek.com"), "mojeek.com");
        assert_eq!(registrable_domain("lite.duckduckgo.com"), "duckduckgo.com");
    }

    #[test]
    fn is_engine_self_link_filters_subdomain() {
        assert!(is_engine_self_link(
            "https://blog.mojeek.com/post",
            "mojeek.com"
        ));
        assert!(is_engine_self_link(
            "https://brave.com/about",
            "search.brave.com"
        ));
        assert!(!is_engine_self_link(
            "https://www.python.org/downloads",
            "mojeek.com"
        ));
    }

    #[test]
    fn strip_tags_decodes_entities() {
        assert_eq!(strip_tags("<b>Python &#x2F; tool</b>"), "Python / tool");
        assert_eq!(strip_tags("hello &amp; world"), "hello & world");
    }

    #[test]
    fn unwrap_ddg_redirect_decodes_uddg() {
        assert_eq!(
            unwrap_ddg_redirect(
                "//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.python.org%2Fdownloads%2F&amp;rut=abc"
            ),
            "https://www.python.org/downloads/"
        );
        assert_eq!(
            unwrap_ddg_redirect("https://example.com/foo"),
            "https://example.com/foo"
        );
    }

    #[test]
    fn extract_ddg_results_parses_html() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.python.org%2Fdownloads%2F&amp;rut=abc">Latest Python Release</a><a class="result__snippet">Python 3.13 is the latest.</a><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdevguide.python.org%2Fversions%2F&amp;rut=def">Python Versions</a><a class="result__snippet">Version status.</a>"#;
        let res = extract_ddg_results(html, 8);
        assert!(res.len() >= 2, "got: {res:?}");
        assert!(
            res.iter()
                .any(|r| r.contains("https://www.python.org/downloads/")),
            "missing python.org, got: {res:?}"
        );
        assert!(
            res.iter()
                .any(|r| r.contains("https://devguide.python.org/versions/")),
            "missing devguide, got: {res:?}"
        );
    }

    #[test]
    fn extract_ddg_results_parses_lite() {
        let html = r#"<tr><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.python.org%2Fdownloads%2F&amp;rut=abc" class='result-link'>Latest Python Release</a></td></tr><tr><td class="result-snippet">Python 3.13 is the latest.</td></tr><tr><td><a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdevguide.python.org%2Fversions%2F&amp;rut=def" class='result-link'>Python Versions</a></td></tr><tr><td class="result-snippet">Version status.</td></tr>"#;
        let res = extract_ddg_results(html, 8);
        assert!(res.len() >= 2, "got: {res:?}");
        assert!(
            res.iter()
                .any(|r| r.contains("https://www.python.org/downloads/")),
            "missing python.org, got: {res:?}"
        );
    }

    #[test]
    fn extract_search_results_dispatches_ddg() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.python.org%2Fdownloads%2F&amp;rut=abc">Latest Python Release</a><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdevguide.python.org%2Fversions%2F&amp;rut=def">Python Versions</a>"#;
        let res = extract_search_results(html, 8, "html.duckduckgo.com");
        assert!(res.len() >= 2, "got: {res:?}");
        assert!(res
            .iter()
            .any(|r| r.contains("https://www.python.org/downloads/")));
    }

    #[tokio::test]
    async fn web_hook_list_empty_without_db() {
        let r = WebHookList.execute(json!({})).await;
        assert!(!r.is_error);
        assert!(r.content.contains("No web hooks configured"));
    }

    #[tokio::test]
    async fn web_hook_run_missing_name() {
        let r = WebHookRun.execute(json!({})).await;
        assert!(r.is_error);
        assert!(r.content.contains("missing 'name'"));
    }

    #[tokio::test]
    async fn web_hook_run_unavailable_without_db() {
        let r = WebHookRun.execute(json!({ "name": "anything" })).await;
        assert!(r.is_error);
        assert!(r.content.contains("web hooks unavailable"));
    }

    #[tokio::test]
    async fn web_hook_add_missing_name() {
        let r = WebHookAdd
            .execute(json!({"url": "https://example.com"}))
            .await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_hook_add_missing_url() {
        let r = WebHookAdd.execute(json!({"name": "test"})).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_hook_modify_missing_name() {
        let r = WebHookModify.execute(json!({})).await;
        assert!(r.is_error);
    }

    #[tokio::test]
    async fn web_hook_delete_missing_name() {
        let r = WebHookDelete.execute(json!({})).await;
        assert!(r.is_error);
    }

    #[test]
    fn render_template_substitutes_payload_variables_secret() {
        let vars = json!({ "id": "42" });
        assert_eq!(
            render_template(
                "https://x.test/{{variables.id}}?p={{payload}}&s={{secret}}",
                Some("hello"),
                Some(&vars),
                Some("sk-1")
            ),
            "https://x.test/42?p=hello&s=sk-1"
        );
    }

    #[test]
    fn render_template_missing_and_unknown_render_empty() {
        let vars = json!({ "a": "1" });
        assert_eq!(
            render_template("{{variables.b}}", None, Some(&vars), None),
            ""
        );
        assert_eq!(render_template("{{unknown}}", Some("x"), None, None), "");
        assert_eq!(render_template("{{payload}}", None, None, None), "");
        assert_eq!(render_template("{{secret}}", None, None, None), "");
    }

    #[test]
    fn configured_host_parses_with_placeholders() {
        assert_eq!(
            configured_host("https://api.example.com/hooks/{{variables.id}}").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(configured_host("{{variables.host}}/x"), None);
        assert_eq!(configured_host("not a url"), None);
    }

    #[test]
    fn host_lock_rejects_variable_host() {
        let raw = "https://{{variables.host}}/api";
        let vars = json!({ "host": "evil.example.com" });
        let rendered = render_template(raw, None, Some(&vars), None);
        assert_ne!(
            configured_host(raw).as_deref(),
            url_host(&rendered).as_deref()
        );
    }

    #[test]
    fn json_path_array_descends_nested_path() {
        let root: Value =
            serde_json::from_str(r#"{"web":{"results":[{"title":"a","url":"u"}]}}"#).unwrap();
        let arr = json_path_array(&root, "web.results").unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["title"], "a");
        assert_eq!(arr[0]["url"], "u");
    }

    #[test]
    fn json_path_array_top_level() {
        let root: Value = serde_json::from_str(r#"{"results":[{"title":"a","url":"u"}]}"#).unwrap();
        let arr = json_path_array(&root, "results").unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn json_path_array_empty_path_returns_root_array() {
        let root: Value = serde_json::from_str(r#"[{"title":"a","url":"u"}]"#).unwrap();
        let arr = json_path_array(&root, "").unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn parse_preset_results_brave_strips_html() {
        let body = r#"{"web":{"results":[
            {"title":"Brave One","url":"https://a.example/1","description":"<b>fast</b> &amp; private"},
            {"title":"Brave Two","url":"https://a.example/2","description":"second"}
        ]}}"#;
        let out = parse_preset_results("brave_api", body, 5);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0],
            "- [Brave One](https://a.example/1)\n  fast & private"
        );
        assert_eq!(out[1], "- [Brave Two](https://a.example/2)\n  second");
    }

    #[test]
    fn parse_preset_results_tavily() {
        let body = r#"{"results":[{"title":"Tavily One","url":"https://b.example/1","content":"answer"}]}"#;
        let out = parse_preset_results("tavily_api", body, 5);
        assert_eq!(out, vec!["- [Tavily One](https://b.example/1)\n  answer"]);
    }

    #[test]
    fn parse_preset_results_serper_uses_link_field() {
        let body = r#"{"organic":[
            {"title":"Serper One","link":"https://c.example/1","snippet":"serp text"}
        ]}"#;
        let out = parse_preset_results("serper_api", body, 5);
        assert_eq!(
            out,
            vec!["- [Serper One](https://c.example/1)\n  serp text"]
        );
    }

    #[test]
    fn parse_preset_results_exa_falls_back_to_summary() {
        let body = r#"{"results":[
            {"title":"Exa One","url":"https://d.example/1","summary":"sum text"},
            {"title":"Exa Two","url":"https://d.example/2","text":"main text"}
        ]}"#;
        let out = parse_preset_results("exa_api", body, 5);
        assert_eq!(out[0], "- [Exa One](https://d.example/1)\n  sum text");
        assert_eq!(out[1], "- [Exa Two](https://d.example/2)\n  main text");
    }

    #[test]
    fn parse_preset_results_respects_count() {
        let body = r#"{"results":[
            {"title":"a","url":"https://x/1"},{"title":"b","url":"https://x/2"},
            {"title":"c","url":"https://x/3"}
        ]}"#;
        let out = parse_preset_results("tavily_api", body, 2);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn parse_custom_results_navigates_path_and_skips_incomplete() {
        let body = r#"{"data":{"items":[
            {"name":"Custom One","href":"https://e.example/1","desc":"first"},
            {"name":"No Url","href":""},
            {"href":"https://e.example/3"}
        ]}}"#;
        let out = parse_custom_results(body, "data.items", "name", "href", "desc");
        assert_eq!(out, vec!["- [Custom One](https://e.example/1)\n  first"]);
    }

    #[test]
    fn render_custom_body_substitutes_tokens_with_json_values() {
        let body = render_custom_body(
            r#"{"q":"{query}","num":"{count}"}"#,
            "rust \"async\" & await",
            5,
        )
        .unwrap();
        let val: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(val["q"].as_str(), Some("rust \"async\" & await"));
        assert_eq!(val["num"].as_i64(), Some(5));
    }

    #[test]
    fn render_custom_body_rejects_non_json_template() {
        assert!(render_custom_body("not json", "q", 5).is_err());
    }
}
