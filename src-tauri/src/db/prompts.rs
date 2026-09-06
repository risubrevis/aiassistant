//! Reusable prompts: user-defined prompt templates runnable from the Home
//! view or the Prompts view. Running a prompt creates a NEW chat (standalone
//! when project_id is NULL, or inside the linked project) and sends the
//! prompt text with its attached files and skills.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// A reusable prompt. `project_id` picks the project the chat created on
/// "run" lives in (NULL = standalone chat).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub body: String,
    pub project_id: Option<String>,
    pub attach_files: Vec<String>,
    pub skill_ids: Vec<String>,
    pub is_favorite: bool,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, title, body, project_id, attach_files, skill_ids, \
     is_favorite, position, created_at, updated_at";

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// JSON columns (attach_files, skill_ids) and the i64 is_favorite need
/// manual parsing, hence no sqlx::FromRow.
fn prompt_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Prompt> {
    Ok(Prompt {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        project_id: row.try_get("project_id")?,
        attach_files: serde_json::from_str(&row.try_get::<String, _>("attach_files")?)?,
        skill_ids: serde_json::from_str(&row.try_get::<String, _>("skill_ids")?)?,
        is_favorite: row.try_get::<i64, _>("is_favorite")? != 0,
        position: row.try_get("position")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Prompt>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM prompts ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    rows.iter().map(prompt_from_row).collect()
}

pub async fn list_favorites(pool: &SqlitePool) -> Result<Vec<Prompt>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM prompts WHERE is_favorite = 1 \
         ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    rows.iter().map(prompt_from_row).collect()
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Prompt>> {
    let row = sqlx::query(&format!("SELECT {COLUMNS} FROM prompts WHERE id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(prompt_from_row).transpose()
}

pub async fn create(
    pool: &SqlitePool,
    title: &str,
    body: &str,
    project_id: Option<&str>,
    attach_files: &[String],
    skill_ids: &[String],
    is_favorite: bool,
) -> Result<Prompt> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let max: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM prompts")
        .fetch_one(&mut *tx)
        .await?;
    let position = max.unwrap_or(-1) + 1;
    sqlx::query(
        "INSERT INTO prompts \
         (id, title, body, project_id, attach_files, skill_ids, is_favorite, position, \
          created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
    )
    .bind(&id)
    .bind(title)
    .bind(body)
    .bind(project_id)
    .bind(serde_json::to_string(attach_files)?)
    .bind(serde_json::to_string(skill_ids)?)
    .bind(is_favorite as i64)
    .bind(position)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("prompt not found: {id}"))
}

/// Update editable fields. The position is left untouched.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    body: &str,
    project_id: Option<&str>,
    attach_files: &[String],
    skill_ids: &[String],
    is_favorite: bool,
) -> Result<Prompt> {
    sqlx::query(
        "UPDATE prompts SET title = ?1, body = ?2, project_id = ?3, attach_files = ?4, \
         skill_ids = ?5, is_favorite = ?6, updated_at = ?7 WHERE id = ?8",
    )
    .bind(title)
    .bind(body)
    .bind(project_id)
    .bind(serde_json::to_string(attach_files)?)
    .bind(serde_json::to_string(skill_ids)?)
    .bind(is_favorite as i64)
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("prompt not found: {id}"))
}

pub async fn set_favorite(pool: &SqlitePool, id: &str, is_favorite: bool) -> Result<()> {
    sqlx::query("UPDATE prompts SET is_favorite = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(is_favorite as i64)
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Global drag-and-drop reorder: splice the moved prompt at `to_position`
/// (clamped to the list end) and rewrite positions 0..n.
pub async fn move_prompt(pool: &SqlitePool, id: &str, to_position: i64) -> Result<()> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM prompts ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    let mut prompts = rows
        .iter()
        .map(prompt_from_row)
        .collect::<Result<Vec<_>>>()?;
    let current = prompts
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("prompt not found: {id}"))?;
    prompts.retain(|p| p.id != id);
    let target = to_position.clamp(0, prompts.len() as i64) as usize;
    prompts.insert(target, current);
    let now = now_ms();
    let mut tx = pool.begin().await?;
    for (position, prompt) in prompts.iter().enumerate() {
        sqlx::query("UPDATE prompts SET position = ?1 WHERE id = ?2")
            .bind(position as i64)
            .bind(&prompt.id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE prompts SET updated_at = ?1 WHERE id = ?2")
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM prompts WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
