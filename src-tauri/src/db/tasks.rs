use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// One persisted task item on a chat's task list. `source` is "internal"
/// (LLM todo_write plan) or "agent" (external worker-agent run); `run_id`
/// links agent rows to their agent_runs record (NULL for internal).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub chat_id: String,
    pub position: i64,
    pub content: String,
    pub active_form: Option<String>,
    pub status: String,
    pub message_id: String,
    pub updated_at: i64,
    pub source: String,
    pub run_id: Option<String>,
}

/// A single item from a full-list replace (todo_write tool payload).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TaskInput {
    pub content: String,
    #[serde(rename = "activeForm", default)]
    pub active_form: Option<String>,
    pub status: String, // pending | in_progress | completed | cancelled
}

const VALID_STATUSES: [&str; 4] = ["pending", "in_progress", "completed", "cancelled"];

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn validate_status(status: &str) -> Result<()> {
    if VALID_STATUSES.contains(&status) {
        Ok(())
    } else {
        bail!(
            "invalid task status `{status}` (expected one of: {})",
            VALID_STATUSES.join(", ")
        )
    }
}

fn task_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Task> {
    Ok(Task {
        id: row.try_get("id")?,
        chat_id: row.try_get("chat_id")?,
        position: row.try_get("position")?,
        content: row.try_get("content")?,
        active_form: row.try_get("active_form")?,
        status: row.try_get("status")?,
        message_id: row.try_get("message_id")?,
        updated_at: row.try_get("updated_at")?,
        source: row.try_get("source")?,
        run_id: row.try_get("run_id")?,
    })
}

/// Insert rows for one scope (internal plan, or one agent run) inside `tx`.
#[allow(clippy::too_many_arguments)]
async fn insert_tasks(
    tx: &mut sqlx::SqliteConnection,
    chat_id: &str,
    source: &str,
    run_id: Option<&str>,
    message_id: &str,
    items: Vec<TaskInput>,
    now: i64,
) -> Result<Vec<Task>> {
    let mut out = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.to_string(),
            position: position as i64,
            content: item.content.clone(),
            active_form: item.active_form.clone(),
            status: item.status.clone(),
            message_id: message_id.to_string(),
            updated_at: now,
            source: source.to_string(),
            run_id: run_id.map(str::to_string),
        };
        sqlx::query(
            "INSERT INTO chat_tasks \
             (id, chat_id, position, content, active_form, status, message_id, updated_at, \
              source, run_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(&task.id)
        .bind(&task.chat_id)
        .bind(task.position)
        .bind(&task.content)
        .bind(&task.active_form)
        .bind(&task.status)
        .bind(&task.message_id)
        .bind(task.updated_at)
        .bind(&task.source)
        .bind(&task.run_id)
        .execute(&mut *tx)
        .await?;
        out.push(task);
    }
    Ok(out)
}

/// Replace the chat's internal task list in one transaction and return the
/// inserted rows ordered by position. Agent-run rows are left untouched.
pub async fn replace_tasks(
    pool: &SqlitePool,
    chat_id: &str,
    message_id: &str,
    items: Vec<TaskInput>,
) -> Result<Vec<Task>> {
    for item in &items {
        validate_status(&item.status)?;
    }
    let now = now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM chat_tasks WHERE chat_id = ?1 AND source = 'internal'")
        .bind(chat_id)
        .execute(&mut *tx)
        .await?;
    let out = insert_tasks(&mut tx, chat_id, "internal", None, message_id, items, now).await?;
    tx.commit().await?;
    Ok(out)
}

/// Replace the task list of one agent run in one transaction and return the
/// inserted rows ordered by position.
pub async fn replace_agent_tasks(
    pool: &SqlitePool,
    chat_id: &str,
    run_id: &str,
    message_id: &str,
    items: Vec<TaskInput>,
) -> Result<Vec<Task>> {
    for item in &items {
        validate_status(&item.status)?;
    }
    let now = now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM chat_tasks WHERE chat_id = ?1 AND run_id = ?2 AND source = 'agent'")
        .bind(chat_id)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    let out = insert_tasks(
        &mut tx,
        chat_id,
        "agent",
        Some(run_id),
        message_id,
        items,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}

/// All tasks of a chat: internal rows first (by position), then agent rows
/// grouped by run in agent_runs.started_at order.
pub async fn list_tasks(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Task>> {
    let rows = sqlx::query(
        "SELECT t.id, t.chat_id, t.position, t.content, t.active_form, t.status, t.message_id, \
         t.updated_at, t.source, t.run_id \
         FROM chat_tasks t \
         LEFT JOIN agent_runs r ON t.run_id = r.id \
         WHERE t.chat_id = ?1 \
         ORDER BY CASE t.source WHEN 'internal' THEN 0 ELSE 1 END, \
                  COALESCE(r.started_at, 0) ASC, t.position ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(task_from_row).collect()
}

/// Clear every task of the chat (internal + agent) — the modal's Clear button.
pub async fn clear_tasks(pool: &SqlitePool, chat_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chat_tasks WHERE chat_id = ?1")
        .bind(chat_id)
        .execute(pool)
        .await?;
    Ok(())
}
