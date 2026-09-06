use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use super::models::now_ms;

const COLS: &str = "id, title, name, description, method, url, headers, body_template, \
     auth_type, auth_username, auth_header_name, auth_param_name, has_secret, \
     timeout_ms, is_active, position, created_at, updated_at";

/// Raw DB row of `web_hooks`.
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)] // created_at/updated_at are written/read via SQL only
pub struct WebHookRow {
    pub id: String,
    pub title: String,
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body_template: String,
    pub auth_type: String,
    pub auth_username: String,
    pub auth_header_name: String,
    pub auth_param_name: String,
    pub has_secret: i64,
    pub timeout_ms: i64,
    pub is_active: i64,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Serialized shape for the frontend. `url`/`headers`/`body_template` keep the
/// `{{secret}}` placeholder — the secret itself never leaves the keyring.
#[derive(Debug, Clone, Serialize)]
pub struct WebHookView {
    pub id: String,
    pub title: String,
    pub name: String,
    pub description: String,
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body_template: String,
    pub auth_type: String,
    pub auth_username: String,
    pub auth_header_name: String,
    pub auth_param_name: String,
    pub has_secret: bool,
    pub timeout_ms: i64,
    pub is_active: bool,
    pub position: i64,
}

/// Editable web hook fields for create/update. The secret is managed through a
/// separate command, not through this struct.
#[derive(Debug, Clone, Deserialize)]
pub struct WebHookInput {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: String,
    #[serde(default)]
    pub body_template: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub auth_username: String,
    #[serde(default)]
    pub auth_header_name: String,
    #[serde(default)]
    pub auth_param_name: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: i64,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> i64 {
    30_000
}

/// Result of the Settings → Web Hooks "Test" button.
#[derive(Debug, Clone, Serialize)]
pub struct WebHookTestResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub detail: String,
    pub elapsed_ms: u64,
    pub body: String,
}

fn to_view(r: WebHookRow) -> WebHookView {
    WebHookView {
        id: r.id,
        title: r.title,
        name: r.name,
        description: r.description,
        method: r.method,
        url: r.url,
        headers: r.headers,
        body_template: r.body_template,
        auth_type: r.auth_type,
        auth_username: r.auth_username,
        auth_header_name: r.auth_header_name,
        auth_param_name: r.auth_param_name,
        has_secret: r.has_secret != 0,
        timeout_ms: r.timeout_ms,
        is_active: r.is_active != 0,
        position: r.position,
    }
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<WebHookRow>> {
    let rows = sqlx::query_as::<_, WebHookRow>(&format!(
        "SELECT {COLS} FROM web_hooks ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_views(pool: &SqlitePool) -> Result<Vec<WebHookView>> {
    let rows = list(pool).await?;
    Ok(rows.into_iter().map(to_view).collect())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<WebHookRow>> {
    let row =
        sqlx::query_as::<_, WebHookRow>(&format!("SELECT {COLS} FROM web_hooks WHERE id = ?1"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn get_by_name(pool: &SqlitePool, name: &str) -> Result<Option<WebHookRow>> {
    let row =
        sqlx::query_as::<_, WebHookRow>(&format!("SELECT {COLS} FROM web_hooks WHERE name = ?1"))
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

pub async fn has_active(pool: &SqlitePool) -> Result<bool> {
    let row = sqlx::query_scalar::<_, i64>("SELECT 1 FROM web_hooks WHERE is_active = 1 LIMIT 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn list_active(pool: &SqlitePool) -> Result<Vec<WebHookRow>> {
    let rows = sqlx::query_as::<_, WebHookRow>(&format!(
        "SELECT {COLS} FROM web_hooks WHERE is_active = 1 ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create(pool: &SqlitePool, input: WebHookInput) -> Result<WebHookRow> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM web_hooks")
        .fetch_one(pool)
        .await
        .unwrap_or(1);
    sqlx::query(
        "INSERT INTO web_hooks \
         (id, title, name, description, method, url, headers, body_template, auth_type, \
         auth_username, auth_header_name, auth_param_name, has_secret, timeout_ms, \
         is_active, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?14, ?15, ?16, ?16)",
    )
    .bind(&id)
    .bind(&input.title)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.method)
    .bind(&input.url)
    .bind(&input.headers)
    .bind(&input.body_template)
    .bind(&input.auth_type)
    .bind(&input.auth_username)
    .bind(&input.auth_header_name)
    .bind(&input.auth_param_name)
    .bind(input.timeout_ms)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(position)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("web hook not found: {id}"))
}

pub async fn update(pool: &SqlitePool, id: &str, input: WebHookInput) -> Result<WebHookRow> {
    sqlx::query(
        "UPDATE web_hooks SET title = ?1, name = ?2, description = ?3, method = ?4, url = ?5, \
         headers = ?6, body_template = ?7, auth_type = ?8, auth_username = ?9, \
         auth_header_name = ?10, auth_param_name = ?11, timeout_ms = ?12, is_active = ?13, \
         updated_at = ?14 WHERE id = ?15",
    )
    .bind(&input.title)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.method)
    .bind(&input.url)
    .bind(&input.headers)
    .bind(&input.body_template)
    .bind(&input.auth_type)
    .bind(&input.auth_username)
    .bind(&input.auth_header_name)
    .bind(&input.auth_param_name)
    .bind(input.timeout_ms)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("web hook not found: {id}"))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM web_hooks WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set position = index for each id in the given order.
pub async fn reorder(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE web_hooks SET position = ?1, updated_at = ?3 WHERE id = ?2")
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
    sqlx::query("UPDATE web_hooks SET is_active = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if is_active { 1 } else { 0 })
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_has_secret(pool: &SqlitePool, id: &str, has_secret: bool) -> Result<()> {
    sqlx::query("UPDATE web_hooks SET has_secret = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if has_secret { 1 } else { 0 })
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
