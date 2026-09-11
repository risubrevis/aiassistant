pub mod embed;
pub mod store;

use std::path::Path;

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::attachments::Attachment;
use crate::db::models;
use crate::providers::{ChatMessage, ContentPart};

const MAX_FILES_PER_PATH: usize = 600;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_CHUNKS_PER_FILE: usize = 200;
const RAG_TOP_K: usize = 6;
const RAG_MIN_SCORE: f32 = 0.20;
const SNIPPET_CHARS: usize = 900;

pub fn embed_enabled(cfg: &Config) -> bool {
    cfg.defaults.rag_enabled
        && cfg
            .defaults
            .embedding_model
            .as_ref()
            .is_some_and(|m| !m.provider.is_empty() && !m.model.is_empty())
}

/// Index a chat attachment (text files only). No-op for images/binaries.
pub async fn index_attachment(pool: &SqlitePool, cfg: &Config, att: &Attachment) -> Result<usize> {
    if !embed_enabled(cfg) || att.is_image || !embed::is_text_mime(&att.mime_type) {
        return Ok(0);
    }
    let bytes = match std::fs::read(&att.storage_path) {
        Ok(b) => b,
        Err(e) => {
            warn!("rag: read attachment {} failed: {e}", att.storage_path);
            return Ok(0);
        }
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Ok(0);
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    index_text(
        pool,
        cfg,
        "chat",
        &att.chat_id,
        "attachment",
        Some(&att.id),
        &att.file_name,
        &text,
    )
    .await
}

/// Index all text files under a project path root.
pub async fn index_project_path(
    pool: &SqlitePool,
    cfg: &Config,
    project_id: &str,
    root: &str,
) -> Result<usize> {
    if !embed_enabled(cfg) {
        return Ok(0);
    }
    let mut total = 0usize;
    for (rel, abs) in collect_text_files(Path::new(root), MAX_FILES_PER_PATH) {
        match index_file_at(pool, cfg, "project", project_id, "file", None, &rel, &abs).await {
            Ok(n) => total += n,
            Err(e) => warn!("rag: index {rel} failed: {e}"),
        }
    }
    info!("rag: indexed {total} chunks under {root} for project {project_id}");
    Ok(total)
}

/// Clear and re-index all paths of a project. No-op (and preserves existing
/// vectors) when RAG is disabled — the scope is NOT deleted in that case, so
/// re-enabling RAG keeps previously indexed chunks.
pub async fn reindex_project(pool: &SqlitePool, cfg: &Config, project_id: &str) -> Result<usize> {
    if !embed_enabled(cfg) {
        return Ok(0);
    }
    store::delete_scope(pool, "project", project_id).await?;
    let paths = models::list_project_paths(pool, project_id).await?;
    let mut total = 0;
    for p in paths {
        if let Ok(n) = index_project_path(pool, cfg, project_id, &p.path).await {
            total += n;
        }
    }
    Ok(total)
}

/// Incrementally (re)index a single project file. No-op when RAG is disabled,
/// the file is not text, exceeds MAX_FILE_BYTES, or is empty. Used by the
/// project file-watcher for incremental updates.
pub async fn index_single_file(
    pool: &SqlitePool,
    cfg: &Config,
    project_id: &str,
    _root: &str,
    rel: &str,
    abs: &str,
) -> Result<usize> {
    if !embed_enabled(cfg) || !is_text_path(Path::new(abs)) {
        return Ok(0);
    }
    index_file_at(pool, cfg, "project", project_id, "file", None, rel, abs).await
}

/// Remove indexed chunks for a deleted project file. Guarded by embed_enabled
/// to match reindex_project: chunks are preserved while RAG is disabled.
pub async fn delete_file_chunks(
    pool: &SqlitePool,
    cfg: &Config,
    project_id: &str,
    rel: &str,
) -> Result<()> {
    if !embed_enabled(cfg) {
        return Ok(());
    }
    store::delete_file_source(pool, "project", project_id, rel).await
}

pub async fn clear_chat(pool: &SqlitePool, chat_id: &str) -> Result<()> {
    store::delete_scope(pool, "chat", chat_id).await
}

pub async fn clear_project(pool: &SqlitePool, project_id: &str) -> Result<()> {
    store::delete_scope(pool, "project", project_id).await
}

/// Clear project-scoped chunks and all chunks of the project's chats.
pub async fn clear_project_and_chats(pool: &SqlitePool, project_id: &str) -> Result<()> {
    store::delete_project_and_chats(pool, project_id).await
}

/// Clear chunks for a single attachment.
pub async fn clear_attachment(pool: &SqlitePool, att_id: &str) -> Result<()> {
    store::delete_source(pool, "attachment", att_id).await
}

/// Clear every embedding row (used when the embedding model changes or on
/// explicit user request).
pub async fn clear_all(pool: &SqlitePool) -> Result<()> {
    store::delete_all(pool).await
}

pub async fn count_all(pool: &SqlitePool) -> Result<i64> {
    store::count_all(pool).await
}

/// GC attachment chunks whose source attachment no longer exists.
pub async fn gc_orphans(pool: &SqlitePool) -> Result<()> {
    store::delete_orphan_attachments(pool).await
}

pub async fn status(
    pool: &SqlitePool,
    chat_id: Option<&str>,
    project_id: Option<&str>,
) -> Result<(i64, i64)> {
    let chat = if let Some(c) = chat_id {
        store::count_for_scopes(pool, &[("chat", c)]).await?
    } else {
        0
    };
    let project = if let Some(p) = project_id {
        store::count_for_scopes(pool, &[("project", p)]).await?
    } else {
        0
    };
    Ok((chat, project))
}

#[allow(clippy::too_many_arguments)]
async fn index_text(
    pool: &SqlitePool,
    cfg: &Config,
    scope: &str,
    scope_id: &str,
    source_kind: &str,
    source_id: Option<&str>,
    source_path: &str,
    text: &str,
) -> Result<usize> {
    let mut chunks = embed::chunk_text(text);
    if chunks.is_empty() {
        return Ok(0);
    }
    if chunks.len() > MAX_CHUNKS_PER_FILE {
        chunks.truncate(MAX_CHUNKS_PER_FILE);
    }
    let vecs = embed::embed(pool, cfg, &chunks).await?;
    if vecs.len() != chunks.len() {
        anyhow::bail!(
            "embedding count mismatch: {} vs {}",
            vecs.len(),
            chunks.len()
        );
    }
    let records: Vec<(String, String, Vec<f32>)> = chunks
        .into_iter()
        .zip(vecs)
        .map(|(c, v)| {
            let h = hash_str(&c);
            (c, h, v)
        })
        .collect();
    let n = records.len();
    store::replace_source(
        pool,
        scope,
        scope_id,
        source_kind,
        source_id,
        source_path,
        &records,
    )
    .await?;
    Ok(n)
}

#[allow(clippy::too_many_arguments)]
async fn index_file_at(
    pool: &SqlitePool,
    cfg: &Config,
    scope: &str,
    scope_id: &str,
    source_kind: &str,
    source_id: Option<&str>,
    rel: &str,
    abs: &str,
) -> Result<usize> {
    let path = Path::new(abs);
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Ok(0);
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    if text.trim().is_empty() {
        return Ok(0);
    }
    index_text(
        pool,
        cfg,
        scope,
        scope_id,
        source_kind,
        source_id,
        rel,
        &text,
    )
    .await
}

fn hash_str(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:x}", h.finish())
}

fn is_text_path(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        return embed::is_text_ext(ext);
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| {
            let l = n.to_lowercase();
            l == "dockerfile" || l == "makefile" || l.starts_with('.')
        })
        .unwrap_or(false)
}

fn collect_text_files(root: &Path, max: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk(root, root, &mut out, max);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>, max: usize) {
    if out.len() >= max {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for ent in entries {
        if out.len() >= max {
            return;
        }
        let Ok(ent) = ent else { continue };
        let p = ent.path();
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.')
                || matches!(
                    name,
                    "node_modules"
                        | "target"
                        | "dist"
                        | "build"
                        | "out"
                        | "venv"
                        | "__pycache__"
                        | "vendor"
                        | "deps"
                        | ".git"
                )
            {
                continue;
            }
        }
        if p.is_dir() {
            walk(root, &p, out, max);
        } else if is_text_path(&p) {
            let rel = p
                .strip_prefix(root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| p.display().to_string());
            out.push((rel, p.display().to_string()));
        }
    }
}

/// Retrieve relevant snippets for the upcoming turn and format a system-prompt
/// section. Returns None when RAG is disabled, there is no query, no chunks are
/// indexed, or the embed call fails — the turn must never break on RAG errors.
pub async fn retrieve_for_turn(
    pool: &SqlitePool,
    cfg: &Config,
    chat_id: &str,
    project_id: Option<&str>,
    work: &[ChatMessage],
) -> Option<String> {
    if !embed_enabled(cfg) {
        return None;
    }
    let query = latest_user_text(work)?;
    if query.trim().is_empty() {
        return None;
    }
    let mut scopes: Vec<(&str, &str)> = vec![("chat", chat_id)];
    if let Some(pid) = project_id {
        scopes.push(("project", pid));
    }
    let rows = match store::list_for_scopes(pool, &scopes).await {
        Ok(r) => r,
        Err(e) => {
            warn!("rag: list chunks failed: {e}");
            return None;
        }
    };
    if rows.is_empty() {
        return None;
    }
    let qvec = match embed::embed(pool, cfg, &[query]).await {
        Ok(v) => v.into_iter().next()?,
        Err(e) => {
            warn!("rag: embed query failed: {e}");
            return None;
        }
    };
    let top: Vec<_> = store::top_k(&qvec, &rows, RAG_TOP_K)
        .into_iter()
        .filter(|(_, s)| *s >= RAG_MIN_SCORE)
        .collect();
    if top.is_empty() {
        return None;
    }
    let mut section = String::from(
        "Relevant snippets from indexed files (retrieved via RAG — use if relevant, ignore otherwise):",
    );
    for (idx, score) in top {
        let row = &rows[idx];
        let snippet: String = row.text.chars().take(SNIPPET_CHARS).collect();
        section.push_str(&format!(
            "\n\n--- {} (score {:.2}) ---\n{}",
            row.source_path, score, snippet
        ));
    }
    Some(section)
}

fn latest_user_text(work: &[ChatMessage]) -> Option<String> {
    for m in work.iter().rev() {
        if m.role != "user" {
            continue;
        }
        if let Some(parts) = m.parts.as_deref().filter(|p| !p.is_empty()) {
            let text: String = parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        if !m.content.trim().is_empty() {
            return Some(m.content.clone());
        }
        return None;
    }
    None
}
