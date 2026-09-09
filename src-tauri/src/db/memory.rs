//! Chat memory: small persistent notes the LLM can save. A standalone chat
//! owns its memory; a chat inside a project shares memory with the whole
//! project (rows carry the project's id).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, FromRow, Row};

use crate::db::models::now_ms;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Memory {
    pub id: String,
    pub chat_id: String,
    pub project_id: Option<String>,
    pub content: String,
    pub category: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, chat_id, project_id, content, category, \
     created_at, updated_at";

pub async fn list_for_chat(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Memory>> {
    let row = sqlx::query("SELECT project_id FROM chats WHERE id = ?1")
        .bind(chat_id)
        .fetch_optional(pool)
        .await?;
    let project_id: Option<String> = match row {
        Some(row) => row.try_get("project_id")?,
        None => None,
    };
    match project_id {
        // Project chat: memory is shared across the project's chats.
        Some(pid) => list_for_project(pool, &pid).await,
        None => {
            let rows = sqlx::query_as::<_, Memory>(&format!(
                "SELECT {COLUMNS} FROM memory \
                 WHERE chat_id = ?1 AND project_id IS NULL \
                 ORDER BY created_at ASC"
            ))
            .bind(chat_id)
            .fetch_all(pool)
            .await?;
            Ok(rows)
        }
    }
}

pub async fn list_owned_by_chat(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Memory>> {
    let rows = sqlx::query_as::<_, Memory>(&format!(
        "SELECT {COLUMNS} FROM memory WHERE chat_id = ?1 ORDER BY created_at ASC"
    ))
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_for_project(pool: &SqlitePool, project_id: &str) -> Result<Vec<Memory>> {
    let rows = sqlx::query_as::<_, Memory>(&format!(
        "SELECT {COLUMNS} FROM memory WHERE project_id = ?1 ORDER BY created_at ASC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn save(
    pool: &SqlitePool,
    chat_id: &str,
    project_id: Option<&str>,
    content: &str,
    category: Option<&str>,
) -> Result<Memory> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO memory \
         (id, chat_id, project_id, content, category, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
    )
    .bind(&id)
    .bind(chat_id)
    .bind(project_id)
    .bind(content)
    .bind(category)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(Memory {
        id,
        chat_id: chat_id.to_string(),
        project_id: project_id.map(str::to_string),
        content: content.to_string(),
        category: category.map(str::to_string),
        created_at: now,
        updated_at: now,
    })
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    content: &str,
    category: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE memory SET content = ?1, category = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(content)
        .bind(category)
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM memory WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_chat(pool: &SqlitePool, chat_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM memory WHERE chat_id = ?1")
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_project(pool: &SqlitePool, project_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM memory WHERE project_id = ?1")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn reassign_chat_project(
    pool: &SqlitePool,
    chat_id: &str,
    project_id: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE memory SET project_id = ?1 WHERE chat_id = ?2")
        .bind(project_id)
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}
