//! Project Kanban tasks: per-project cards with a status column
//! (backlog | todo | in_progress | review | done), manual ordering within
//! each column, and a changelog of status transitions.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// One card on a project's Kanban board. `chat_id` links the task to the chat
/// created when it is "run" (NULL until then); `position` orders the task
/// within its (project_id, status) column.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProjectTask {
    pub id: String,
    pub project_id: String,
    pub chat_id: Option<String>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, project_id, chat_id, title, description, status, priority, \
     position, created_at, updated_at";

const STATUSES: [&str; 5] = ["backlog", "todo", "in_progress", "review", "done"];
const PRIORITIES: [&str; 4] = ["low", "medium", "high", "urgent"];

const STATUS_ORDER: &str = "CASE status WHEN 'backlog' THEN 0 WHEN 'todo' THEN 1 \
                            WHEN 'in_progress' THEN 2 WHEN 'review' THEN 3 ELSE 4 END";

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn validate_status(status: &str) -> Result<()> {
    if STATUSES.contains(&status) {
        Ok(())
    } else {
        bail!(
            "invalid project task status `{status}` (expected one of: {})",
            STATUSES.join(", ")
        )
    }
}

fn validate_priority(priority: &str) -> Result<()> {
    if PRIORITIES.contains(&priority) {
        Ok(())
    } else {
        bail!(
            "invalid project task priority `{priority}` (expected one of: {})",
            PRIORITIES.join(", ")
        )
    }
}

fn task_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProjectTask> {
    Ok(ProjectTask {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        chat_id: row.try_get("chat_id")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        status: row.try_get("status")?,
        priority: row.try_get("priority")?,
        position: row.try_get("position")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn insert_changelog(
    tx: &mut sqlx::SqliteConnection,
    task_id: &str,
    from: Option<&str>,
    to: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO project_task_changelog (id, task_id, from_status, to_status, changed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(task_id)
    .bind(from)
    .bind(to)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Highest position in a column, or -1 when the column is empty.
async fn max_position(
    tx: &mut sqlx::SqliteConnection,
    project_id: &str,
    status: &str,
) -> Result<i64> {
    let max: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(position) FROM project_tasks WHERE project_id = ?1 AND status = ?2",
    )
    .bind(project_id)
    .bind(status)
    .fetch_one(&mut *tx)
    .await?;
    Ok(max.unwrap_or(-1))
}

/// All tasks of one (project_id, status) column, ordered by position.
async fn load_column(
    tx: &mut sqlx::SqliteConnection,
    project_id: &str,
    status: &str,
) -> Result<Vec<ProjectTask>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM project_tasks \
         WHERE project_id = ?1 AND status = ?2 \
         ORDER BY position ASC, created_at ASC"
    ))
    .bind(project_id)
    .bind(status)
    .fetch_all(&mut *tx)
    .await?;
    rows.iter().map(task_from_row).collect()
}

/// Rewrite the positions of a whole column to 0..n after a splice.
async fn write_positions(tx: &mut sqlx::SqliteConnection, column: &[ProjectTask]) -> Result<()> {
    for (position, task) in column.iter().enumerate() {
        sqlx::query("UPDATE project_tasks SET position = ?1 WHERE id = ?2")
            .bind(position as i64)
            .bind(&task.id)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

/// Move `task` to the end of the `new_status` column, shift the old column's
/// higher positions down by 1, and write the changelog row. Returns the new
/// position. Caller must ensure the status actually changes.
async fn append_to_column(
    tx: &mut sqlx::SqliteConnection,
    task: &ProjectTask,
    new_status: &str,
    now: i64,
) -> Result<i64> {
    let position = max_position(tx, &task.project_id, new_status).await? + 1;
    sqlx::query(
        "UPDATE project_tasks SET position = position - 1 \
         WHERE project_id = ?1 AND status = ?2 AND position > ?3",
    )
    .bind(&task.project_id)
    .bind(&task.status)
    .bind(task.position)
    .execute(&mut *tx)
    .await?;
    insert_changelog(tx, &task.id, Some(&task.status), new_status, now).await?;
    Ok(position)
}

/// All tasks of a project, ordered by status in lifecycle order, then position.
pub async fn list_for_project(pool: &SqlitePool, project_id: &str) -> Result<Vec<ProjectTask>> {
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM project_tasks WHERE project_id = ?1 \
         ORDER BY {STATUS_ORDER}, position ASC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(task_from_row).collect()
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<ProjectTask>> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM project_tasks WHERE id = ?1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(task_from_row).transpose()
}

pub async fn create(
    pool: &SqlitePool,
    project_id: &str,
    title: &str,
    description: &str,
    status: &str,
    priority: &str,
) -> Result<ProjectTask> {
    validate_status(status)?;
    validate_priority(priority)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let position = max_position(&mut tx, project_id, status).await? + 1;
    sqlx::query(
        "INSERT INTO project_tasks \
         (id, project_id, chat_id, title, description, status, priority, position, \
          created_at, updated_at) \
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(title)
    .bind(description)
    .bind(status)
    .bind(priority)
    .bind(position)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {id}"))
}

/// Update editable fields. A status change appends the task to the end of the
/// new column, shifts the old column's higher positions down by 1, and writes
/// a changelog row; an unchanged status keeps the position.
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    description: &str,
    status: &str,
    priority: &str,
) -> Result<ProjectTask> {
    validate_status(status)?;
    validate_priority(priority)?;
    let current = get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {id}"))?;
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let mut position = current.position;
    if current.status != status {
        position = append_to_column(&mut tx, &current, status, now).await?;
    }
    sqlx::query(
        "UPDATE project_tasks SET title = ?1, description = ?2, status = ?3, priority = ?4, \
         position = ?5, updated_at = ?6 WHERE id = ?7",
    )
    .bind(title)
    .bind(description)
    .bind(status)
    .bind(priority)
    .bind(position)
    .bind(now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {id}"))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM project_tasks WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Drag-and-drop move: reorder within a column or move across columns to
/// `to_position`. Both affected columns are spliced and their positions
/// rewritten 0..n, so an out-of-range `to_position` clamps to the column end.
pub async fn move_task(
    pool: &SqlitePool,
    id: &str,
    to_status: &str,
    to_position: i64,
) -> Result<()> {
    validate_status(to_status)?;
    let current = get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {id}"))?;
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let new_position = if current.status == to_status {
        let mut column = load_column(&mut tx, &current.project_id, to_status).await?;
        column.retain(|t| t.id != id);
        let target = to_position.clamp(0, column.len() as i64) as usize;
        column.insert(target, current.clone());
        write_positions(&mut tx, &column).await?;
        target as i64
    } else {
        let mut old_column = load_column(&mut tx, &current.project_id, &current.status).await?;
        old_column.retain(|t| t.id != id);
        write_positions(&mut tx, &old_column).await?;
        let mut new_column = load_column(&mut tx, &current.project_id, to_status).await?;
        let target = to_position.clamp(0, new_column.len() as i64) as usize;
        new_column.insert(target, current.clone());
        write_positions(&mut tx, &new_column).await?;
        target as i64
    };
    sqlx::query(
        "UPDATE project_tasks SET status = ?1, position = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind(to_status)
    .bind(new_position)
    .bind(now)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if current.status != to_status {
        insert_changelog(&mut tx, id, Some(&current.status), to_status, now).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Attach the chat created when a task is "run"; the task moves to
/// in_progress (appended to that column with a changelog row if it changed).
pub async fn link_chat(pool: &SqlitePool, task_id: &str, chat_id: &str) -> Result<ProjectTask> {
    let current = get(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {task_id}"))?;
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let mut position = current.position;
    if current.status != "in_progress" {
        position = append_to_column(&mut tx, &current, "in_progress", now).await?;
    }
    sqlx::query(
        "UPDATE project_tasks SET chat_id = ?1, status = ?2, position = ?3, updated_at = ?4 \
         WHERE id = ?5",
    )
    .bind(chat_id)
    .bind("in_progress")
    .bind(position)
    .bind(now)
    .bind(task_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {task_id}"))
}

/// On turn completion: if the chat is linked to a task currently in
/// `in_progress`, move it to `review` (append to the review column + changelog).
/// Returns the updated task + its project_id (for event emission), or None if
/// no linked task is in progress (e.g. already in review/done, or no link).
pub async fn finish_by_chat(
    pool: &SqlitePool,
    chat_id: &str,
) -> Result<Option<(String, ProjectTask)>> {
    let row = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM project_tasks WHERE chat_id = ?1 AND status = 'in_progress' LIMIT 1"
    ))
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    let Some(current) = row else { return Ok(None) };
    let current = task_from_row(&current)?;
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let position = append_to_column(&mut tx, &current, "review", now).await?;
    sqlx::query(
        "UPDATE project_tasks SET status = ?1, position = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind("review")
    .bind(position)
    .bind(now)
    .bind(&current.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let updated = get(pool, &current.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project task not found: {}", current.id))?;
    Ok(Some((updated.project_id.clone(), updated)))
}
