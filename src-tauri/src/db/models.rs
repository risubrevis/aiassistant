#![allow(clippy::too_many_arguments)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::config::{GlobalRule, Skill};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Chat {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f64>,
    pub pinned: i64,
    pub archived: i64,
    pub settings: Option<String>,
    pub meta: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Chat subset returned to the sidebar (lightweight).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatSummary {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub pinned: i64,
    pub archived: i64,
    pub meta: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub content_parts: Option<String>,
    pub model: Option<String>,
    pub usage: Option<String>,
    pub thinking_ms: Option<i64>,
    pub finish_reason: Option<String>,
    pub is_branch_root: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub default_provider_id: Option<String>,
    pub default_model_id: Option<String>,
    pub color: String,
    pub settings: Option<String>,
    pub pinned: i64,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectPath {
    pub id: String,
    pub project_id: String,
    pub path: String,
    pub kind: String,
    pub watch: i64,
    pub exclude_globs: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ChatPath {
    pub id: String,
    pub chat_id: String,
    pub path: String,
    pub kind: String,
    pub watch: i64,
    pub exclude_globs: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Rule {
    pub id: String,
    pub scope: String,
    pub scope_id: String,
    pub title: String,
    pub text: String,
    pub enabled: i64,
    pub sort_order: i64,
    pub added_by: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
struct SkillRow {
    id: String,
    title: String,
    body: String,
    sort_order: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ToolCallRow {
    pub id: String,
    pub chat_id: String,
    pub message_id: String,
    pub tool_call_id: Option<String>,
    pub tool_name: String,
    pub source: String,
    pub arguments: Option<String>,
    pub result: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct FtsHit {
    pub chat_id: String,
    pub message_id: String,
    pub rank: f64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentRun {
    pub id: String,
    pub chat_id: String,
    pub parent_tool_call_id: Option<String>,
    pub parent_id: Option<String>,
    pub agent_connection_id: String,
    pub subtask_index: i64,
    pub subtask_prompt: String,
    pub cwd: String,
    pub worktree_branch: Option<String>,
    pub agent_session_id: Option<String>,
    pub status: String,
    pub result_summary: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChatSession {
    pub id: String,
    pub chat_id: String,
    pub summary: String,
    pub boundary_message_id: String,
    pub token_count: i64,
    pub model: Option<String>,
    pub created_at: i64,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ----- chats -----

pub async fn create_chat(
    pool: &SqlitePool,
    id: &str,
    project_id: Option<&str>,
    title: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    now: i64,
) -> Result<Chat> {
    sqlx::query(
        "INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
    )
    .bind(id)
    .bind(project_id)
    .bind(title)
    .bind(provider_id)
    .bind(model_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(Chat {
        id: id.into(),
        project_id: project_id.map(|s| s.to_string()),
        title: title.into(),
        provider_id: provider_id.map(|s| s.into()),
        model_id: model_id.map(|s| s.into()),
        system_prompt: None,
        temperature: None,
        pinned: 0,
        archived: 0,
        settings: None,
        meta: None,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    })
}

pub async fn list_chats(pool: &SqlitePool) -> Result<Vec<ChatSummary>> {
    let chats = sqlx::query_as::<_, ChatSummary>(
        "SELECT id, project_id, title, provider_id, model_id, pinned, archived, meta, \
         sort_order, created_at, updated_at FROM chats WHERE archived = 0 \
         ORDER BY pinned DESC, sort_order ASC, updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(chats)
}

pub async fn list_project_chats(pool: &SqlitePool, project_id: &str) -> Result<Vec<ChatSummary>> {
    let chats = sqlx::query_as::<_, ChatSummary>(
        "SELECT id, project_id, title, provider_id, model_id, pinned, archived, meta, \
         sort_order, created_at, updated_at FROM chats WHERE project_id = ?1 AND archived = 0 \
         ORDER BY pinned DESC, sort_order ASC, updated_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(chats)
}

pub async fn get_chat(pool: &SqlitePool, id: &str) -> Result<Option<Chat>> {
    let chat = sqlx::query_as::<_, Chat>(
        "SELECT id, project_id, title, provider_id, model_id, system_prompt, temperature, \
         pinned, archived, settings, meta, sort_order, created_at, updated_at FROM chats WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(chat)
}

pub async fn rename_chat(pool: &SqlitePool, id: &str, title: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE chats SET title = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(title)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_chat_model(
    pool: &SqlitePool,
    id: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE chats SET provider_id = ?1, model_id = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(provider_id)
        .bind(model_id)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_chat_project(
    pool: &SqlitePool,
    id: &str,
    project_id: Option<&str>,
    now: i64,
) -> Result<()> {
    // Place the chat at the end of the target group's sort order so it
    // appears at a predictable position after a cross-group drag-and-drop.
    let max_sort: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sort_order), -1) FROM chats WHERE project_id IS ?1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    sqlx::query("UPDATE chats SET project_id = ?1, sort_order = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(project_id)
        .bind(max_sort + 1)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_chat_system_prompt(
    pool: &SqlitePool,
    id: &str,
    system_prompt: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE chats SET system_prompt = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(system_prompt)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_chat_meta(pool: &SqlitePool, id: &str, meta: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE chats SET meta = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(meta)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_chat(pool: &SqlitePool, id: &str, now: i64) -> Result<()> {
    sqlx::query("UPDATE chats SET updated_at = ?1 WHERE id = ?2")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_chat(pool: &SqlitePool, id: &str) -> Result<()> {
    // tool_calls and rules have no FK cascade; clean them explicitly.
    sqlx::query("DELETE FROM tool_calls WHERE chat_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rules WHERE scope = 'chat' AND scope_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    // messages, chat_paths, agent_runs, agent_sessions, chat_sessions and
    // attachments cascade via FK ON DELETE CASCADE; messages_fts triggers
    // remove FTS rows on message DELETE.
    sqlx::query("DELETE FROM chats WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_chat_pinned(pool: &SqlitePool, id: &str, pinned: bool, now: i64) -> Result<()> {
    sqlx::query("UPDATE chats SET pinned = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(pinned as i64)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set sort_order for each chat id to its index in the vector.
pub async fn reorder_chats(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE chats SET sort_order = ?1 WHERE id = ?2")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ----- messages -----

pub async fn insert_message(pool: &SqlitePool, m: &Message, now: i64) -> Result<()> {
    sqlx::query(
        "INSERT INTO messages \
         (id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
          finish_reason, is_branch_root, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(&m.id)
    .bind(&m.chat_id)
    .bind(&m.parent_id)
    .bind(&m.role)
    .bind(&m.content)
    .bind(&m.content_parts)
    .bind(&m.model)
    .bind(&m.usage)
    .bind(m.thinking_ms)
    .bind(&m.finish_reason)
    .bind(m.is_branch_root)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// All messages in a chat (every branch), ordered by creation.
pub async fn list_messages(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Message>> {
    let msgs = sqlx::query_as::<_, Message>(
        "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
         finish_reason, is_branch_root, created_at FROM messages WHERE chat_id = ?1 \
         ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(msgs)
}

/// Walk the active branch: from `leaf_id` up to the root via `parent_id`.
/// If `leaf_id` is None, returns the legacy linear list (by created_at).
pub async fn list_active_branch(
    pool: &SqlitePool,
    chat_id: &str,
    leaf_id: Option<&str>,
) -> Result<Vec<Message>> {
    let Some(leaf) = leaf_id else {
        return list_messages(pool, chat_id).await;
    };
    // Walk up the parent chain.
    let mut chain: Vec<Message> = Vec::new();
    let mut cursor = Some(leaf.to_string());
    while let Some(cur) = cursor {
        let row = sqlx::query_as::<_, Message>(
            "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, \
             thinking_ms, finish_reason, is_branch_root, created_at FROM messages WHERE id = ?1",
        )
        .bind(&cur)
        .fetch_optional(pool)
        .await?;
        let Some(m) = row else { break };
        cursor = m.parent_id.clone();
        chain.push(m);
    }
    chain.reverse();
    Ok(chain)
}

/// List child messages of `parent_id` (siblings for branching UI).
pub async fn list_children(pool: &SqlitePool, parent_id: &str) -> Result<Vec<Message>> {
    let msgs = sqlx::query_as::<_, Message>(
        "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
         finish_reason, is_branch_root, created_at FROM messages WHERE parent_id = ?1 \
         ORDER BY created_at ASC",
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;
    Ok(msgs)
}

/// Root messages of a chat (parent_id IS NULL), ordered by creation.
pub async fn list_roots(pool: &SqlitePool, chat_id: &str) -> Result<Vec<Message>> {
    let msgs = sqlx::query_as::<_, Message>(
        "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
         finish_reason, is_branch_root, created_at FROM messages WHERE chat_id = ?1 \
         AND parent_id IS NULL ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(msgs)
}

pub async fn get_message(pool: &SqlitePool, id: &str) -> Result<Option<Message>> {
    let m = sqlx::query_as::<_, Message>(
        "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
         finish_reason, is_branch_root, created_at FROM messages WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(m)
}

// ----- projects -----

pub async fn create_project(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    color: &str,
    now: i64,
) -> Result<Project> {
    sqlx::query(
        "INSERT INTO projects (id, name, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
    )
    .bind(id)
    .bind(name)
    .bind(color)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(Project {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        system_prompt: String::new(),
        default_provider_id: None,
        default_model_id: None,
        color: color.into(),
        settings: None,
        pinned: 0,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    })
}

pub async fn list_projects(pool: &SqlitePool) -> Result<Vec<Project>> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT id, name, description, system_prompt, default_provider_id, default_model_id, \
         color, settings, pinned, sort_order, created_at, updated_at FROM projects \
         ORDER BY pinned DESC, sort_order ASC, updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(projects)
}

pub async fn get_project(pool: &SqlitePool, id: &str) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>(
        "SELECT id, name, description, system_prompt, default_provider_id, default_model_id, \
         color, settings, pinned, sort_order, created_at, updated_at FROM projects WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

pub async fn update_project(pool: &SqlitePool, p: &Project, now: i64) -> Result<()> {
    sqlx::query(
        "UPDATE projects SET name = ?1, description = ?2, system_prompt = ?3, \
         default_provider_id = ?4, default_model_id = ?5, color = ?6, settings = ?7, \
         updated_at = ?8 WHERE id = ?9",
    )
    .bind(&p.name)
    .bind(&p.description)
    .bind(&p.system_prompt)
    .bind(&p.default_provider_id)
    .bind(&p.default_model_id)
    .bind(&p.color)
    .bind(&p.settings)
    .bind(now)
    .bind(&p.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_project_pinned(pool: &SqlitePool, id: &str, pinned: bool, now: i64) -> Result<()> {
    sqlx::query("UPDATE projects SET pinned = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(pinned as i64)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set sort_order for each project id to its index in the vector.
pub async fn reorder_projects(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE projects SET sort_order = ?1 WHERE id = ?2")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_project(pool: &SqlitePool, id: &str) -> Result<()> {
    // Collect chat ids of the project to clean no-FK tables first.
    let chat_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM chats WHERE project_id = ?1")
        .bind(id)
        .fetch_all(pool)
        .await?;
    for cid in &chat_ids {
        sqlx::query("DELETE FROM tool_calls WHERE chat_id = ?1")
            .bind(cid)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM rules WHERE scope = 'chat' AND scope_id = ?1")
            .bind(cid)
            .execute(pool)
            .await?;
    }
    // Chats cascade to messages, chat_paths, agent_runs, sessions, attachments.
    sqlx::query("DELETE FROM chats WHERE project_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    // rules and project_paths have no FK from projects; clean explicitly.
    sqlx::query("DELETE FROM rules WHERE scope = 'project' AND scope_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM project_paths WHERE project_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM projects WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ----- project_paths / chat_paths -----

pub async fn list_project_paths(pool: &SqlitePool, project_id: &str) -> Result<Vec<ProjectPath>> {
    let rows = sqlx::query_as::<_, ProjectPath>(
        "SELECT id, project_id, path, kind, watch, exclude_globs, created_at \
         FROM project_paths WHERE project_id = ?1 ORDER BY created_at ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_project_path(pool: &SqlitePool, id: &str) -> Result<Option<ProjectPath>> {
    let p = sqlx::query_as::<_, ProjectPath>(
        "SELECT id, project_id, path, kind, watch, exclude_globs, created_at \
         FROM project_paths WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

pub async fn add_project_path(
    pool: &SqlitePool,
    id: &str,
    project_id: &str,
    path: &str,
    kind: &str,
    watch: bool,
    exclude_globs: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO project_paths (id, project_id, path, kind, watch, exclude_globs, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(id)
    .bind(project_id)
    .bind(path)
    .bind(kind)
    .bind(if watch { 1 } else { 0 })
    .bind(exclude_globs)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_project_path(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM project_paths WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_chat_paths(pool: &SqlitePool, chat_id: &str) -> Result<Vec<ChatPath>> {
    let rows = sqlx::query_as::<_, ChatPath>(
        "SELECT id, chat_id, path, kind, watch, exclude_globs, created_at \
         FROM chat_paths WHERE chat_id = ?1 ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_chat_path(
    pool: &SqlitePool,
    id: &str,
    chat_id: &str,
    path: &str,
    kind: &str,
    watch: bool,
    exclude_globs: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO chat_paths (id, chat_id, path, kind, watch, exclude_globs, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(id)
    .bind(chat_id)
    .bind(path)
    .bind(kind)
    .bind(if watch { 1 } else { 0 })
    .bind(exclude_globs)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_chat_path(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chat_paths WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ----- rules -----

pub async fn list_rules(pool: &SqlitePool, scope: &str, scope_id: &str) -> Result<Vec<Rule>> {
    let rows = sqlx::query_as::<_, Rule>(
        "SELECT id, title, scope, scope_id, text, enabled, sort_order, added_by, created_at, updated_at \
         FROM rules WHERE scope = ?1 AND scope_id = ?2 ORDER BY sort_order ASC, created_at ASC",
    )
    .bind(scope)
    .bind(scope_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_rule(
    pool: &SqlitePool,
    id: &str,
    scope: &str,
    scope_id: &str,
    title: &str,
    text: &str,
    sort_order: i64,
    added_by: Option<&str>,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO rules (id, scope, scope_id, title, text, enabled, sort_order, added_by, \
         created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, ?8)",
    )
    .bind(id)
    .bind(scope)
    .bind(scope_id)
    .bind(title)
    .bind(text)
    .bind(sort_order)
    .bind(added_by)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_rule(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    text: &str,
    now: i64,
) -> Result<()> {
    sqlx::query("UPDATE rules SET title = ?1, text = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(title)
        .bind(text)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn toggle_rule(pool: &SqlitePool, id: &str, enabled: bool, now: i64) -> Result<()> {
    sqlx::query("UPDATE rules SET enabled = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if enabled { 1 } else { 0 })
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_rule(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM rules WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// List global rules (scope = "global", scope_id = ""), mapped to config shape.
#[allow(dead_code)]
pub async fn list_global_rules(pool: &SqlitePool) -> Result<Vec<GlobalRule>> {
    let rows = sqlx::query_as::<_, Rule>(
        "SELECT id, title, scope, scope_id, text, enabled, sort_order, added_by, created_at, updated_at \
         FROM rules WHERE scope = 'global' AND scope_id = '' ORDER BY sort_order ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| GlobalRule {
            title: r.title,
            text: r.text,
            enabled: r.enabled == 1,
        })
        .collect())
}

/// Replace all global rules (delete + insert) with the given list.
#[allow(dead_code)]
pub async fn replace_global_rules(pool: &SqlitePool, rules: &[GlobalRule]) -> Result<()> {
    let now = now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM rules WHERE scope = 'global' AND scope_id = ''")
        .execute(&mut *tx)
        .await?;
    for (i, r) in rules.iter().enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO rules (id, title, scope, scope_id, text, enabled, sort_order, added_by, created_at, updated_at) \
             VALUES (?1, ?2, 'global', '', ?3, ?4, ?5, 'user', ?6, ?6)",
        )
        .bind(&id)
        .bind(&r.title)
        .bind(&r.text)
        .bind(if r.enabled { 1 } else { 0 })
        .bind(i as i64)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ----- skills -----

/// List all skills (global user skills), mapped to config shape, ordered by sort_order then created_at.
#[allow(dead_code)]
pub async fn list_skills(pool: &SqlitePool) -> Result<Vec<Skill>> {
    let rows = sqlx::query_as::<_, SkillRow>(
        "SELECT id, title, body, sort_order, created_at, updated_at FROM skills ORDER BY sort_order ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| Skill {
            id: r.id,
            title: r.title,
            body: r.body,
        })
        .collect())
}

/// Upsert all given skills and delete skills whose id is not in the list.
#[allow(dead_code)]
pub async fn save_skills(pool: &SqlitePool, skills: &[Skill]) -> Result<()> {
    let now = now_ms();
    let mut tx = pool.begin().await?;
    for (i, s) in skills.iter().enumerate() {
        sqlx::query(
            "INSERT INTO skills (id, title, body, sort_order, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5) \
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body, \
             sort_order = excluded.sort_order, updated_at = excluded.updated_at",
        )
        .bind(&s.id)
        .bind(&s.title)
        .bind(&s.body)
        .bind(i as i64)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    if skills.is_empty() {
        sqlx::query("DELETE FROM skills").execute(&mut *tx).await?;
    } else {
        let ids: Vec<String> = skills.iter().map(|s| s.id.clone()).collect();
        let placeholders: Vec<String> = (0..ids.len()).map(|i| format!("?{}", i + 1)).collect();
        let q = format!(
            "DELETE FROM skills WHERE id NOT IN ({})",
            placeholders.join(", ")
        );
        let mut qb = sqlx::query(&q);
        for id in &ids {
            qb = qb.bind(id);
        }
        qb.execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

// ----- tool_calls -----

pub async fn insert_tool_call(pool: &SqlitePool, row: &ToolCallRow) -> Result<()> {
    sqlx::query(
        "INSERT INTO tool_calls \
         (id, chat_id, message_id, tool_call_id, tool_name, source, arguments, result, status, \
          error, started_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(&row.id)
    .bind(&row.chat_id)
    .bind(&row.message_id)
    .bind(&row.tool_call_id)
    .bind(&row.tool_name)
    .bind(&row.source)
    .bind(&row.arguments)
    .bind(&row.result)
    .bind(&row.status)
    .bind(&row.error)
    .bind(row.started_at)
    .bind(row.finished_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_tool_call(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
    finished_at: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE tool_calls SET status = ?1, result = ?2, error = ?3, finished_at = ?4 WHERE id = ?5",
    )
    .bind(status)
    .bind(result)
    .bind(error)
    .bind(finished_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

// ----- FTS -----

/// Full-text search across messages. When `project_id` is given, restrict to
/// chats of that project (docs/08 cross-chat retrieval).
pub async fn fts_search(
    pool: &SqlitePool,
    query: &str,
    project_id: Option<&str>,
) -> Result<Vec<FtsHit>> {
    // Escape double quotes in the query for FTS5 phrase.
    let q = query.replace('"', "\"\"");
    let pattern = format!("\"{}\"", q);
    let sql = if project_id.is_some() {
        "SELECT m.chat_id AS chat_id, m.id AS message_id, bm25(messages_fts) AS rank, \
         snippet(messages_fts, 0, '«', '»', '…', 12) AS snippet \
         FROM messages_fts JOIN messages m ON m.rowid = messages_fts.content_rowid \
         WHERE messages_fts MATCH ?1 AND m.chat_id IN (SELECT id FROM chats WHERE project_id = ?2) \
         ORDER BY rank LIMIT 25"
    } else {
        "SELECT m.chat_id AS chat_id, m.id AS message_id, bm25(messages_fts) AS rank, \
         snippet(messages_fts, 0, '«', '»', '…', 12) AS snippet \
         FROM messages_fts JOIN messages m ON m.rowid = messages_fts.content_rowid \
         WHERE messages_fts MATCH ?1 ORDER BY rank LIMIT 25"
    };
    let rows = if let Some(pid) = project_id {
        sqlx::query_as::<_, FtsHit>(sql)
            .bind(&pattern)
            .bind(pid)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_as::<_, FtsHit>(sql)
            .bind(&pattern)
            .fetch_all(pool)
            .await?
    };
    Ok(rows)
}

// ----- agent_runs (docs/16) -----

pub async fn insert_agent_run(pool: &SqlitePool, r: &AgentRun) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_runs (id, chat_id, parent_tool_call_id, parent_id, \
         agent_connection_id, subtask_index, subtask_prompt, cwd, worktree_branch, \
         agent_session_id, status, result_summary, started_at, ended_at, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )
    .bind(&r.id)
    .bind(&r.chat_id)
    .bind(&r.parent_tool_call_id)
    .bind(&r.parent_id)
    .bind(&r.agent_connection_id)
    .bind(r.subtask_index)
    .bind(&r.subtask_prompt)
    .bind(&r.cwd)
    .bind(&r.worktree_branch)
    .bind(&r.agent_session_id)
    .bind(&r.status)
    .bind(&r.result_summary)
    .bind(r.started_at)
    .bind(r.ended_at)
    .bind(r.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_agent_run_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    result_summary: Option<&str>,
    agent_session_id: Option<&str>,
    worktree_branch: Option<&str>,
    started_at: Option<i64>,
    ended_at: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_runs SET status = ?1, result_summary = COALESCE(?2, result_summary), \
         agent_session_id = COALESCE(?3, agent_session_id), \
         worktree_branch = COALESCE(?4, worktree_branch), \
         started_at = COALESCE(?5, started_at), ended_at = ?6 WHERE id = ?7",
    )
    .bind(status)
    .bind(result_summary)
    .bind(agent_session_id)
    .bind(worktree_branch)
    .bind(started_at)
    .bind(ended_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_agent_run(pool: &SqlitePool, id: &str) -> Result<Option<AgentRun>> {
    let row = sqlx::query_as::<_, AgentRun>(
        "SELECT id, chat_id, parent_tool_call_id, parent_id, agent_connection_id, subtask_index, \
         subtask_prompt, cwd, worktree_branch, agent_session_id, status, result_summary, \
         started_at, ended_at, created_at FROM agent_runs WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_agent_runs(pool: &SqlitePool, chat_id: Option<&str>) -> Result<Vec<AgentRun>> {
    let rows = if let Some(cid) = chat_id {
        sqlx::query_as::<_, AgentRun>(
            "SELECT id, chat_id, parent_tool_call_id, parent_id, agent_connection_id, \
             subtask_index, subtask_prompt, cwd, worktree_branch, agent_session_id, status, \
             result_summary, started_at, ended_at, created_at \
             FROM agent_runs WHERE chat_id = ?1 ORDER BY created_at DESC LIMIT 100",
        )
        .bind(cid)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, AgentRun>(
            "SELECT id, chat_id, parent_tool_call_id, parent_id, agent_connection_id, \
             subtask_index, subtask_prompt, cwd, worktree_branch, agent_session_id, status, \
             result_summary, started_at, ended_at, created_at \
             FROM agent_runs ORDER BY created_at DESC LIMIT 100",
        )
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

pub async fn list_agent_runs_by_parent(
    pool: &SqlitePool,
    parent_id: &str,
) -> Result<Vec<AgentRun>> {
    let rows = sqlx::query_as::<_, AgentRun>(
        "SELECT id, chat_id, parent_tool_call_id, parent_id, agent_connection_id, \
         subtask_index, subtask_prompt, cwd, worktree_branch, agent_session_id, status, \
         result_summary, started_at, ended_at, created_at \
         FROM agent_runs WHERE parent_id = ?1 ORDER BY subtask_index ASC",
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn latest_chat_session(pool: &SqlitePool, chat_id: &str) -> Result<Option<ChatSession>> {
    let row = sqlx::query_as::<_, ChatSession>(
        "SELECT id, chat_id, summary, boundary_message_id, token_count, model, created_at \
         FROM chat_sessions WHERE chat_id = ?1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_chat_sessions(pool: &SqlitePool, chat_id: &str) -> Result<Vec<ChatSession>> {
    let rows = sqlx::query_as::<_, ChatSession>(
        "SELECT id, chat_id, summary, boundary_message_id, token_count, model, created_at \
         FROM chat_sessions WHERE chat_id = ?1 ORDER BY created_at DESC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn insert_chat_session(
    pool: &SqlitePool,
    id: &str,
    chat_id: &str,
    summary: &str,
    boundary_message_id: &str,
    token_count: i64,
    model: Option<&str>,
    created_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO chat_sessions \
         (id, chat_id, summary, boundary_message_id, token_count, model, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(id)
    .bind(chat_id)
    .bind(summary)
    .bind(boundary_message_id)
    .bind(token_count)
    .bind(model)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Resolve the context window (in tokens) for a saved model. Primary source is
/// the `provider_models` row (by id); falls back to the `models_cache` table
/// via the row's provider+api name. 0 = unknown.
pub async fn resolve_context_window(pool: &SqlitePool, model_id: &str) -> u64 {
    let saved: Option<(i64, Option<String>, String)> = sqlx::query_as(
        "SELECT context_window, provider_id, name FROM provider_models WHERE id = ?1",
    )
    .bind(model_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some((context_window, provider_id, name)) = saved else {
        return 0;
    };
    if context_window > 0 {
        return context_window as u64;
    }
    let row: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT context_window FROM models_cache WHERE provider_id = ?1 AND name = ?2",
    )
    .bind(provider_id)
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.and_then(|(c,)| c)
        .filter(|c| *c > 0)
        .map(|c| c as u64)
        .unwrap_or(0)
}

// ----- chat info -----

/// Aggregated chat metadata for the chat-info panel.
#[derive(Debug, Clone, Serialize)]
pub struct ChatInfo {
    pub chat_id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub provider_name: Option<String>,
    pub model_display_name: Option<String>,
    pub model: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub user_messages: i64,
    pub assistant_messages: i64,
    pub tool_messages: i64,
    pub tool_calls: i64,
    pub attachments: i64,
    pub project_paths: i64,
    pub chat_paths: i64,
    pub rules: i64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub total_duration_ms: u64,
    pub total_ttft_ms: u64,
    pub total_generation_ms: u64,
    pub first_message_at: Option<i64>,
    pub last_message_at: Option<i64>,
}

pub async fn chat_info(pool: &SqlitePool, chat_id: &str) -> Result<ChatInfo> {
    let chat = sqlx::query_as::<_, Chat>(
        "SELECT id, project_id, title, provider_id, model_id, system_prompt, temperature, \
         pinned, archived, settings, meta, sort_order, created_at, updated_at \
         FROM chats WHERE id = ?1",
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    let Some(chat) = chat else {
        return Ok(ChatInfo {
            chat_id: chat_id.to_string(),
            title: String::new(),
            project_id: None,
            project_name: None,
            provider_id: None,
            model_id: None,
            provider_name: None,
            model_display_name: None,
            model: None,
            created_at: 0,
            updated_at: 0,
            message_count: 0,
            user_messages: 0,
            assistant_messages: 0,
            tool_messages: 0,
            tool_calls: 0,
            attachments: 0,
            project_paths: 0,
            chat_paths: 0,
            rules: 0,
            total_prompt_tokens: 0,
            total_completion_tokens: 0,
            total_tokens: 0,
            total_duration_ms: 0,
            total_ttft_ms: 0,
            total_generation_ms: 0,
            first_message_at: None,
            last_message_at: None,
        });
    };

    let msgs = sqlx::query_as::<_, Message>(
        "SELECT id, chat_id, parent_id, role, content, content_parts, model, usage, thinking_ms, \
         finish_reason, is_branch_root, created_at FROM messages WHERE chat_id = ?1 \
         ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(pool)
    .await?;
    // msgs are ordered by created_at ASC, so ends give first/last timestamps.
    let first_message_at = msgs.first().map(|m| m.created_at);
    let last_message_at = msgs.last().map(|m| m.created_at);

    let mut user_messages = 0i64;
    let mut assistant_messages = 0i64;
    let mut tool_messages = 0i64;
    let mut total_prompt = 0u64;
    let mut total_completion = 0u64;
    let mut total_tokens = 0u64;
    let mut total_duration = 0u64;
    let mut total_ttft = 0u64;
    let mut total_generation = 0u64;
    let mut last_model: Option<String> = None;
    for m in &msgs {
        match m.role.as_str() {
            "user" => user_messages += 1,
            "assistant" => assistant_messages += 1,
            "tool" => tool_messages += 1,
            _ => {}
        }
        if let Some(u) = m.usage.as_ref() {
            if let Ok(usage) = serde_json::from_str::<crate::providers::Usage>(u) {
                total_prompt += usage.prompt_tokens;
                total_completion += usage.completion_tokens;
                total_tokens += usage.total_tokens;
                if let Some(d) = usage.total_duration_ms {
                    total_duration += d;
                }
                if let Some(d) = usage.time_to_first_token_ms {
                    total_ttft += d;
                }
                if let Some(d) = usage.generation_duration_ms {
                    total_generation += d;
                }
            }
        }
        if m.role == "assistant" && m.model.is_some() {
            last_model = m.model.clone();
        }
    }

    let tool_calls: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_calls WHERE chat_id = ?1")
        .bind(chat_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let attachments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE chat_id = ?1")
            .bind(chat_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    let chat_paths: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_paths WHERE chat_id = ?1")
        .bind(chat_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let rules: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM rules WHERE scope = 'chat' AND scope_id = ?1")
            .bind(chat_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    let (project_name, project_paths) = match chat.project_id.as_ref() {
        Some(pid) => {
            let name: Option<String> =
                sqlx::query_scalar("SELECT name FROM projects WHERE id = ?1")
                    .bind(pid)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten();
            let project_paths: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM project_paths WHERE project_id = ?1")
                    .bind(pid)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
            (name, project_paths)
        }
        None => (None, 0),
    };

    let provider_name: Option<String> = match chat.provider_id.as_deref().filter(|s| !s.is_empty())
    {
        Some(pid) => sqlx::query_scalar("SELECT name FROM providers WHERE id = ?1")
            .bind(pid)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let model_display_name: Option<String> =
        match chat.model_id.as_deref().filter(|s| !s.is_empty()) {
            Some(mid) => {
                sqlx::query_scalar("SELECT display_name FROM provider_models WHERE id = ?1")
                    .bind(mid)
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        };

    Ok(ChatInfo {
        chat_id: chat.id,
        title: chat.title,
        project_id: chat.project_id,
        project_name,
        provider_id: chat.provider_id,
        model_id: chat.model_id,
        provider_name,
        model_display_name,
        model: last_model,
        created_at: chat.created_at,
        updated_at: chat.updated_at,
        message_count: user_messages + assistant_messages + tool_messages,
        user_messages,
        assistant_messages,
        tool_messages,
        tool_calls,
        attachments,
        project_paths,
        chat_paths,
        rules,
        total_prompt_tokens: total_prompt,
        total_completion_tokens: total_completion,
        total_tokens,
        total_duration_ms: total_duration,
        total_ttft_ms: total_ttft,
        total_generation_ms: total_generation,
        first_message_at,
        last_message_at,
    })
}

// ----- web search providers -----

/// Raw row of `web_search_tool_providers`.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct WebSearchProvider {
    pub id: String,
    pub title: String,
    pub url: String,
    pub enabled: i64,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Frontend-facing shape (enabled as bool).
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct WebSearchProviderView {
    pub id: String,
    pub title: String,
    pub url: String,
    pub enabled: bool,
    pub position: i64,
}

/// Input for replacing the whole provider list (frontend Save).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WebSearchProviderInput {
    pub title: String,
    pub url: String,
    pub enabled: bool,
}

/// List all providers ordered by position then created_at, mapped to the view shape.
#[allow(dead_code)]
pub async fn list_web_search_providers(pool: &SqlitePool) -> Result<Vec<WebSearchProviderView>> {
    let rows = sqlx::query_as::<_, WebSearchProvider>(
        "SELECT id, title, url, enabled, position, created_at, updated_at \
         FROM web_search_tool_providers ORDER BY position ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| WebSearchProviderView {
            id: r.id,
            title: r.title,
            url: r.url,
            enabled: r.enabled == 1,
            position: r.position,
        })
        .collect())
}

/// List only enabled providers ordered by position (used by the web_search tool).
#[allow(dead_code)]
pub async fn list_enabled_web_search_providers(
    pool: &SqlitePool,
) -> Result<Vec<WebSearchProvider>> {
    let rows = sqlx::query_as::<_, WebSearchProvider>(
        "SELECT id, title, url, enabled, position, created_at, updated_at \
         FROM web_search_tool_providers WHERE enabled = 1 ORDER BY position ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Replace all providers (delete + insert) with the given list. Position is the
/// array index; ids are regenerated.
#[allow(dead_code)]
pub async fn replace_web_search_providers(
    pool: &SqlitePool,
    providers: &[WebSearchProviderInput],
) -> Result<()> {
    let now = now_ms();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM web_search_tool_providers")
        .execute(&mut *tx)
        .await?;
    for (i, p) in providers.iter().enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO web_search_tool_providers \
             (id, title, url, enabled, position, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        )
        .bind(id)
        .bind(&p.title)
        .bind(&p.url)
        .bind(if p.enabled { 1 } else { 0 })
        .bind(i as i64)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ----- activity heatmap -----

/// Aggregate activity for one local day ("YYYY-MM-DD").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayActivity {
    pub date: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub messages: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayChatActivity {
    pub chat_id: String,
    pub title: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub requests: i64,
    pub total_tokens: i64,
    pub messages: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayProjectActivity {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub requests: i64,
    pub total_tokens: i64,
    pub messages: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayDetail {
    pub date: String,
    pub requests: i64,
    pub total_tokens: i64,
    pub messages: i64,
    pub chats: Vec<DayChatActivity>,
    pub projects: Vec<DayProjectActivity>,
}

#[derive(Debug, Clone, FromRow)]
struct DayRow {
    day: String,
    requests: i64,
    total_tokens: i64,
    messages: i64,
}

/// Per-local-day totals over all messages, for the activity heatmap.
pub async fn activity_daily(pool: &SqlitePool) -> Result<Vec<DayActivity>> {
    let rows = sqlx::query_as::<_, DayRow>(
        "SELECT \
         date(created_at/1000, 'unixepoch', 'localtime') AS day, \
         SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END) AS requests, \
         SUM(CASE WHEN usage IS NOT NULL THEN \
         CAST(json_extract(usage, '$.total_tokens') AS INTEGER) ELSE 0 END) AS total_tokens, \
         COUNT(*) AS messages \
         FROM messages GROUP BY day ORDER BY day ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| DayActivity {
            date: r.day,
            requests: r.requests,
            total_tokens: r.total_tokens,
            messages: r.messages,
        })
        .collect())
}

#[derive(Debug, Clone, FromRow)]
struct DayChatRow {
    chat_id: String,
    title: String,
    project_id: Option<String>,
    project_name: Option<String>,
    requests: i64,
    total_tokens: i64,
    messages: i64,
}

/// Chats, projects and totals for one local day ("YYYY-MM-DD").
pub async fn activity_day_detail(pool: &SqlitePool, date: &str) -> Result<DayDetail> {
    let rows = sqlx::query_as::<_, DayChatRow>(
        "SELECT \
         c.id AS chat_id, \
         c.title AS title, \
         c.project_id AS project_id, \
         p.name AS project_name, \
         SUM(CASE WHEN m.role = 'user' THEN 1 ELSE 0 END) AS requests, \
         SUM(CASE WHEN m.usage IS NOT NULL THEN \
         CAST(json_extract(m.usage, '$.total_tokens') AS INTEGER) ELSE 0 END) AS total_tokens, \
         COUNT(*) AS messages \
         FROM messages m \
         JOIN chats c ON c.id = m.chat_id \
         LEFT JOIN projects p ON p.id = c.project_id \
         WHERE date(m.created_at/1000, 'unixepoch', 'localtime') = ?1 \
         GROUP BY c.id \
         ORDER BY requests DESC, m.created_at ASC",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;
    let mut requests = 0i64;
    let mut total_tokens = 0i64;
    let mut messages = 0i64;
    let mut projects: Vec<DayProjectActivity> = Vec::new();
    let mut chats = Vec::with_capacity(rows.len());
    for r in rows {
        requests += r.requests;
        total_tokens += r.total_tokens;
        messages += r.messages;
        let idx = match r.project_id.as_deref() {
            Some(pid) => projects
                .iter()
                .position(|p| p.project_id.as_deref() == Some(pid)),
            None => projects.iter().position(|p| p.project_id.is_none()),
        };
        let idx = idx.unwrap_or_else(|| {
            projects.push(DayProjectActivity {
                project_id: r.project_id.clone(),
                project_name: r.project_name.clone(),
                requests: 0,
                total_tokens: 0,
                messages: 0,
            });
            projects.len() - 1
        });
        let p = &mut projects[idx];
        if p.project_name.is_none() {
            p.project_name = r.project_name.clone();
        }
        p.requests += r.requests;
        p.total_tokens += r.total_tokens;
        p.messages += r.messages;
        chats.push(DayChatActivity {
            chat_id: r.chat_id,
            title: r.title,
            project_id: r.project_id,
            project_name: r.project_name,
            requests: r.requests,
            total_tokens: r.total_tokens,
            messages: r.messages,
        });
    }
    projects.sort_by_key(|p| std::cmp::Reverse(p.requests));
    Ok(DayDetail {
        date: date.to_string(),
        requests,
        total_tokens,
        messages,
        chats,
        projects,
    })
}
