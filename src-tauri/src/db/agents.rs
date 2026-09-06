use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::HashMap;

use super::models::now_ms;
use crate::config::{AgentContract, EventSchema};

const COLUMNS: &str = "id, name, description, default_model, capabilities, kind, command, args, \
     prompt_mode, cwd, env, output_format, event_schema, mode_flags, resume_flag, timeout_ms, \
     max_turns, is_active, position, created_at, updated_at";

/// Editable agent fields for create/update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentInput {
    pub name: String,
    pub description: String,
    pub default_model: String,
    pub capabilities: Vec<String>,
    pub kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub prompt_mode: String,
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub output_format: String,
    pub event_schema: Option<EventSchema>,
    pub mode_flags: HashMap<String, Vec<String>>,
    pub resume_flag: String,
    pub timeout_ms: u64,
    pub max_turns: u32,
    pub is_active: bool,
}

impl Default for AgentInput {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            default_model: String::new(),
            capabilities: Vec::new(),
            kind: "subprocess".into(),
            command: String::new(),
            args: Vec::new(),
            prompt_mode: "arg".into(),
            cwd: String::new(),
            env: HashMap::new(),
            output_format: "raw_text".into(),
            event_schema: None,
            mode_flags: HashMap::new(),
            resume_flag: String::new(),
            timeout_ms: 300_000,
            max_turns: 50,
            is_active: true,
        }
    }
}

impl AgentInput {
    /// Template for the Settings UI built from a preset contract.
    pub fn from_contract(c: &AgentContract) -> Self {
        Self {
            name: c.name.clone(),
            description: c.description.clone(),
            default_model: c.default_model.clone(),
            capabilities: c.capabilities.clone(),
            kind: c.kind.clone(),
            command: c.command.clone(),
            args: c.args.clone(),
            prompt_mode: c.prompt_mode.clone(),
            cwd: c.cwd.clone(),
            env: c.env.clone(),
            output_format: c.output_format.clone(),
            event_schema: c.event_schema.clone(),
            mode_flags: c.mode_flags.clone(),
            resume_flag: c.resume_flag.clone(),
            timeout_ms: c.timeout_ms,
            max_turns: c.max_turns,
            is_active: true,
        }
    }

    /// Reconstruct an in-memory contract (e.g. for a bridge test run without a DB row).
    pub fn to_contract(&self, id: &str) -> AgentContract {
        AgentContract {
            id: id.to_string(),
            name: self.name.clone(),
            description: self.description.clone(),
            capabilities: self.capabilities.clone(),
            kind: self.kind.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            prompt_mode: self.prompt_mode.clone(),
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            output_format: self.output_format.clone(),
            event_schema: self.event_schema.clone(),
            mode_flags: self.mode_flags.clone(),
            resume_flag: self.resume_flag.clone(),
            timeout_ms: self.timeout_ms,
            max_turns: self.max_turns,
            default_model: self.default_model.clone(),
        }
    }
}

/// DB row of `agents`; JSON columns are decoded to typed values.
#[derive(Debug, Clone, Serialize)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_model: String,
    pub capabilities: Vec<String>,
    pub kind: String,
    pub command: String,
    pub args: Vec<String>,
    pub prompt_mode: String,
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub output_format: String,
    pub event_schema: Option<EventSchema>,
    pub mode_flags: HashMap<String, Vec<String>>,
    pub resume_flag: String,
    pub timeout_ms: u64,
    pub max_turns: u32,
    pub is_active: bool,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl AgentRow {
    pub fn to_contract(&self) -> AgentContract {
        AgentContract {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            capabilities: self.capabilities.clone(),
            kind: self.kind.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            prompt_mode: self.prompt_mode.clone(),
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            output_format: self.output_format.clone(),
            event_schema: self.event_schema.clone(),
            mode_flags: self.mode_flags.clone(),
            resume_flag: self.resume_flag.clone(),
            timeout_ms: self.timeout_ms,
            max_turns: self.max_turns,
            default_model: self.default_model.clone(),
        }
    }
}

impl<'r> FromRow<'r, SqliteRow> for AgentRow {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::error::Error> {
        fn json_col<T: serde::de::DeserializeOwned + Default>(
            row: &SqliteRow,
            col: &str,
        ) -> Result<T, sqlx::error::Error> {
            let raw: String = row.try_get(col)?;
            // Corrupt/empty JSON degrades to the type default instead of failing the query.
            Ok(serde_json::from_str(&raw).unwrap_or_default())
        }
        let active: i64 = row.try_get("is_active")?;
        let timeout: i64 = row.try_get("timeout_ms")?;
        let turns: i64 = row.try_get("max_turns")?;
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            default_model: row.try_get("default_model")?,
            capabilities: json_col(row, "capabilities")?,
            kind: row.try_get("kind")?,
            command: row.try_get("command")?,
            args: json_col(row, "args")?,
            prompt_mode: row.try_get("prompt_mode")?,
            cwd: row.try_get("cwd")?,
            env: json_col(row, "env")?,
            output_format: row.try_get("output_format")?,
            event_schema: json_col(row, "event_schema")?,
            mode_flags: json_col(row, "mode_flags")?,
            resume_flag: row.try_get("resume_flag")?,
            timeout_ms: timeout.max(0) as u64,
            max_turns: turns.max(0) as u32,
            is_active: active != 0,
            position: row.try_get("position")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

fn json_list<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn json_map<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<AgentRow>> {
    let rows = sqlx::query_as::<_, AgentRow>(&format!(
        "SELECT {COLUMNS} FROM agents ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<AgentRow>> {
    let row = sqlx::query_as::<_, AgentRow>(&format!("SELECT {COLUMNS} FROM agents WHERE id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Only active agents, in user-defined order.
pub async fn list_active(pool: &SqlitePool) -> Result<Vec<AgentRow>> {
    let rows = sqlx::query_as::<_, AgentRow>(&format!(
        "SELECT {COLUMNS} FROM agents WHERE is_active = 1 ORDER BY position ASC, created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create(pool: &SqlitePool, input: AgentInput) -> Result<AgentRow> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM agents")
        .fetch_one(pool)
        .await
        .unwrap_or(1);
    sqlx::query(
        "INSERT INTO agents \
         (id, name, description, default_model, capabilities, kind, command, args, prompt_mode, \
          cwd, env, output_format, event_schema, mode_flags, resume_flag, timeout_ms, max_turns, \
          is_active, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                 ?18, ?19, ?20, ?20)",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.default_model)
    .bind(json_list(&input.capabilities))
    .bind(&input.kind)
    .bind(&input.command)
    .bind(json_list(&input.args))
    .bind(&input.prompt_mode)
    .bind(&input.cwd)
    .bind(json_map(&input.env))
    .bind(&input.output_format)
    .bind(serde_json::to_string(&input.event_schema).unwrap_or_else(|_| "null".into()))
    .bind(json_map(&input.mode_flags))
    .bind(&input.resume_flag)
    .bind(input.timeout_ms as i64)
    .bind(input.max_turns as i64)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(position)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("agent not found: {id}"))
}

pub async fn update(pool: &SqlitePool, id: &str, input: AgentInput) -> Result<AgentRow> {
    sqlx::query(
        "UPDATE agents SET name = ?1, description = ?2, default_model = ?3, capabilities = ?4, \
         kind = ?5, command = ?6, args = ?7, prompt_mode = ?8, cwd = ?9, env = ?10, \
         output_format = ?11, event_schema = ?12, mode_flags = ?13, resume_flag = ?14, \
         timeout_ms = ?15, max_turns = ?16, is_active = ?17, updated_at = ?18 WHERE id = ?19",
    )
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.default_model)
    .bind(json_list(&input.capabilities))
    .bind(&input.kind)
    .bind(&input.command)
    .bind(json_list(&input.args))
    .bind(&input.prompt_mode)
    .bind(&input.cwd)
    .bind(json_map(&input.env))
    .bind(&input.output_format)
    .bind(serde_json::to_string(&input.event_schema).unwrap_or_else(|_| "null".into()))
    .bind(json_map(&input.mode_flags))
    .bind(&input.resume_flag)
    .bind(input.timeout_ms as i64)
    .bind(input.max_turns as i64)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(now_ms())
    .bind(id)
    .execute(pool)
    .await?;
    get(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("agent not found: {id}"))
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM agents WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set position = index for each id in the given order.
pub async fn reorder(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE agents SET position = ?1, updated_at = ?3 WHERE id = ?2")
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
    sqlx::query("UPDATE agents SET is_active = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if is_active { 1 } else { 0 })
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
