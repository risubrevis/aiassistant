use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use super::data_dir;

/// A file attached to a chat and (usually) linked to a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub chat_id: String,
    pub message_id: Option<String>,
    pub file_name: String,
    pub mime_type: String,
    pub file_size: i64,
    pub storage_path: String,
    pub is_image: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub created_at: i64,
}

fn attachment_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Attachment> {
    Ok(Attachment {
        id: row.try_get("id")?,
        chat_id: row.try_get("chat_id")?,
        message_id: row.try_get("message_id")?,
        file_name: row.try_get("file_name")?,
        mime_type: row.try_get("mime_type")?,
        file_size: row.try_get("file_size")?,
        storage_path: row.try_get("storage_path")?,
        is_image: row.try_get::<i64, _>("is_image")? != 0,
        width: row.try_get("width")?,
        height: row.try_get("height")?,
        created_at: row.try_get("created_at")?,
    })
}

pub fn attachments_dir() -> PathBuf {
    let dir = data_dir().join("attachments");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("failed to create attachments dir {}: {e}", dir.display());
    }
    dir
}

fn storage_file(chat_id: &str, id: &str, ext: &str) -> PathBuf {
    attachments_dir().join(chat_id).join(format!("{id}{ext}"))
}

pub fn store_file(chat_id: &str, id: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
    let path = storage_file(chat_id, id, ext);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "rs" => "text/x-rust",
        "ts" => "text/x-typescript",
        "js" => "text/javascript",
        "py" => "text/x-python",
        "go" => "text/x-go",
        "c" | "h" => "text/x-c",
        "cpp" => "text/x-c++",
        "java" => "text/x-java",
        "rb" => "text/x-ruby",
        "sh" => "text/x-shellscript",
        "yml" | "yaml" => "application/x-yaml",
        "toml" => "application/toml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        _ => "application/octet-stream",
    }
}

/// Persist a picked file into the attachment store and record its metadata.
/// Reads the source file synchronously; returns the stored (absolute) path.
pub async fn create(pool: &SqlitePool, chat_id: &str, file_path: &str) -> Result<Attachment> {
    let src = PathBuf::from(file_path);
    let bytes =
        std::fs::read(&src).with_context(|| format!("failed to read file {}", src.display()))?;
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = src
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let mime_type = mime_for_ext(ext.trim_start_matches('.')).to_string();
    let is_image = mime_type.starts_with("image/");
    let file_size = bytes.len() as i64;
    let id = uuid::Uuid::new_v4().to_string();
    let storage_path = store_file(chat_id, &id, &ext, &bytes)?;
    let created_at = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        "INSERT INTO attachments \
         (id, chat_id, message_id, file_name, mime_type, file_size, storage_path, is_image, \
          width, height, created_at) \
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8)",
    )
    .bind(&id)
    .bind(chat_id)
    .bind(&file_name)
    .bind(&mime_type)
    .bind(file_size)
    .bind(storage_path.to_string_lossy().as_ref())
    .bind(is_image)
    .bind(created_at)
    .execute(pool)
    .await?;

    Ok(Attachment {
        id,
        chat_id: chat_id.to_string(),
        message_id: None,
        file_name,
        mime_type,
        file_size,
        storage_path: storage_path.to_string_lossy().to_string(),
        is_image,
        width: None,
        height: None,
        created_at,
    })
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Attachment>> {
    let row = sqlx::query("SELECT * FROM attachments WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(attachment_from_row).transpose()
}

pub async fn list_for_chat(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Attachment>> {
    let rows = sqlx::query("SELECT * FROM attachments WHERE chat_id = ?1 ORDER BY created_at ASC")
        .bind(chat_id)
        .fetch_all(pool)
        .await?;
    rows.iter().map(attachment_from_row).collect()
}

pub async fn list_for_message(pool: &SqlitePool, message_id: &str) -> Result<Vec<Attachment>> {
    let rows =
        sqlx::query("SELECT * FROM attachments WHERE message_id = ?1 ORDER BY created_at ASC")
            .bind(message_id)
            .fetch_all(pool)
            .await?;
    rows.iter().map(attachment_from_row).collect()
}

pub async fn link_to_message(pool: &SqlitePool, ids: &[String], message_id: &str) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("UPDATE attachments SET message_id = ");
    qb.push_bind(message_id);
    qb.push(" WHERE id IN (");
    {
        let mut sep = qb.separated(", ");
        for id in ids {
            sep.push_bind(id);
        }
    }
    qb.push(")");
    qb.build().execute(pool).await?;
    Ok(())
}

fn delete_file_best_effort(path: &str) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("failed to remove attachment file {path}: {e}"),
    }
}

pub async fn remove(pool: &SqlitePool, id: &str) -> Result<()> {
    if let Some(att) = get(pool, id).await? {
        delete_file_best_effort(&att.storage_path);
    }
    sqlx::query("DELETE FROM attachments WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete attachments never linked to a message (e.g. user picked a file but
/// never sent it) older than `older_than_ms`.
pub async fn remove_orphans(pool: &SqlitePool, older_than_ms: i64) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp_millis() - older_than_ms;
    let rows = sqlx::query(
        "SELECT id, storage_path FROM attachments \
         WHERE message_id IS NULL AND created_at < ?1",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: String = row.try_get("id")?;
        let path: String = row.try_get("storage_path")?;
        delete_file_best_effort(&path);
        sqlx::query("DELETE FROM attachments WHERE id = ?1")
            .bind(&id)
            .execute(pool)
            .await?;
    }
    Ok(rows.len())
}

/// Remove a chat's attachment files from disk. Run before `delete_chat`, whose
/// cascade only removes DB rows.
pub async fn delete_chat_attachments(pool: &SqlitePool, chat_id: &str) -> Result<()> {
    for att in list_for_chat(pool, chat_id).await? {
        delete_file_best_effort(&att.storage_path);
    }
    sqlx::query("DELETE FROM attachments WHERE chat_id = ?1")
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}
