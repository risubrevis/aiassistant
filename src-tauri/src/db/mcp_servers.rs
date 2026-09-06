use anyhow::Result;
use serde::Deserialize;
use sqlx::{FromRow, SqlitePool};

use super::models::now_ms;

pub use crate::mcp::McpBody;

/// Editable MCP server fields for create/update.
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerInput {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub body: McpBody,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

/// Raw DB row of `mcp_servers`.
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)] // created_at/updated_at are written/read via SQL only
pub struct McpServerRow {
    pub id: String,
    pub title: String,
    pub name: String,
    pub json_body: String,
    pub is_active: i64,
    pub webui_url: String,
    pub webui_icon: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl McpServerRow {
    pub fn body(&self) -> McpBody {
        serde_json::from_str(&self.json_body).unwrap_or_default()
    }

    pub fn is_active_bool(&self) -> bool {
        self.is_active != 0
    }
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<McpServerRow>> {
    let rows = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, title, name, json_body, is_active, webui_url, webui_icon, position, \
         created_at, updated_at FROM mcp_servers ORDER BY position ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<McpServerRow>> {
    let row = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, title, name, json_body, is_active, webui_url, webui_icon, position, \
         created_at, updated_at FROM mcp_servers WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_by_name(pool: &SqlitePool, name: &str) -> Result<Option<McpServerRow>> {
    let row = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, title, name, json_body, is_active, webui_url, webui_icon, position, \
         created_at, updated_at FROM mcp_servers WHERE name = ?1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create(pool: &SqlitePool, input: McpServerInput) -> Result<McpServerRow> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let position: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM mcp_servers")
            .fetch_one(pool)
            .await
            .unwrap_or(1);
    sqlx::query(
        "INSERT INTO mcp_servers \
         (id, title, name, json_body, is_active, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
    )
    .bind(&id)
    .bind(&input.title)
    .bind(&input.name)
    .bind(serde_json::to_string(&input.body).unwrap_or_else(|_| "{}".into()))
    .bind(if input.is_active { 1 } else { 0 })
    .bind(position)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("mcp server not found: {id}"))
}

pub async fn update(pool: &SqlitePool, id: &str, input: McpServerInput) -> Result<McpServerRow> {
    sqlx::query(
        "UPDATE mcp_servers SET title = ?1, name = ?2, json_body = ?3, is_active = ?4, \
         updated_at = ?5 WHERE id = ?6",
    )
    .bind(&input.title)
    .bind(&input.name)
    .bind(serde_json::to_string(&input.body).unwrap_or_else(|_| "{}".into()))
    .bind(if input.is_active { 1 } else { 0 })
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("mcp server not found: {id}"))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set position = index for each id in the given order.
pub async fn reorder(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE mcp_servers SET position = ?1, updated_at = ?3 WHERE id = ?2")
            .bind(i as i64)
            .bind(id)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn set_active(pool: &SqlitePool, id: &str, is_active: bool) -> Result<()> {
    sqlx::query("UPDATE mcp_servers SET is_active = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if is_active { 1 } else { 0 })
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_webui(
    pool: &SqlitePool,
    id: &str,
    webui_url: &str,
    webui_icon: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE mcp_servers SET webui_url = ?1, webui_icon = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind(webui_url)
    .bind(webui_icon)
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_webui(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE mcp_servers SET webui_url = '', webui_icon = '', updated_at = ?2 WHERE id = ?1",
    )
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_with_webui(pool: &SqlitePool) -> Result<Vec<McpServerRow>> {
    let rows = sqlx::query_as::<_, McpServerRow>(
        "SELECT id, title, name, json_body, is_active, webui_url, webui_icon, position, \
         created_at, updated_at FROM mcp_servers WHERE webui_url != '' ORDER BY position ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
