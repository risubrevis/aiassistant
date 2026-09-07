use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use super::models::now_ms;

/// A configured LLM provider (DB row, public shape).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key_ref: String,
    pub extra_headers: HashMap<String, String>,
    pub timeout_ms: u64,
    pub is_active: bool,
    pub position: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Editable provider fields for create/update.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    #[serde(default)]
    pub name: String,
    /// openai | anthropic | ollama | custom
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    /// if empty, use the new provider id
    #[serde(default)]
    pub api_key_ref: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_timeout_ms() -> u64 {
    30000
}
fn default_true() -> bool {
    true
}

/// A model saved under a provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderModel {
    pub id: String,
    pub provider_id: String,
    /// API model name
    pub name: String,
    pub display_name: String,
    pub enabled: bool,
    pub alias: String,
    pub capabilities: Vec<String>,
    pub context_window: u64,
}

/// Editable model fields for replace-all saves.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderModelInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub context_window: u64,
}

/// One selectable model for dropdowns (provider + model joined).
#[derive(Debug, Clone, Serialize)]
pub struct ModelOption {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub display_name: String,
    pub is_active: bool,
    pub enabled: bool,
}

impl ProviderRow {
    pub fn to_config_provider(&self) -> crate::config::Provider {
        crate::config::Provider {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            base_url: self.base_url.clone(),
            api_key_ref: self.api_key_ref.clone(),
            extra_headers: self.extra_headers.clone(),
            timeout_ms: self.timeout_ms,
        }
    }
}

/// Raw DB row; JSON columns and booleans stay as text/int until mapped.
#[derive(Debug, Clone, FromRow)]
struct ProviderDb {
    id: String,
    name: String,
    kind: String,
    base_url: String,
    api_key_ref: String,
    extra_headers: String,
    timeout_ms: i64,
    is_active: i64,
    position: i64,
    created_at: i64,
    updated_at: i64,
}

impl From<ProviderDb> for ProviderRow {
    fn from(r: ProviderDb) -> Self {
        ProviderRow {
            id: r.id,
            name: r.name,
            kind: r.kind,
            base_url: r.base_url,
            api_key_ref: r.api_key_ref,
            extra_headers: parse_json(&r.extra_headers),
            timeout_ms: r.timeout_ms.max(0) as u64,
            is_active: r.is_active != 0,
            position: r.position as i32,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Raw DB row of `provider_models`.
#[derive(Debug, Clone, FromRow)]
struct ProviderModelDb {
    id: String,
    provider_id: String,
    name: String,
    display_name: String,
    enabled: i64,
    alias: String,
    capabilities: String,
    context_window: i64,
}

impl From<ProviderModelDb> for ProviderModel {
    fn from(r: ProviderModelDb) -> Self {
        ProviderModel {
            id: r.id,
            provider_id: r.provider_id,
            name: r.name,
            display_name: r.display_name,
            enabled: r.enabled != 0,
            alias: r.alias,
            capabilities: parse_json(&r.capabilities),
            context_window: r.context_window.max(0) as u64,
        }
    }
}

/// Parse a JSON column, tolerating empty/invalid values as empty default.
fn parse_json<T: serde::de::DeserializeOwned + Default>(s: &str) -> T {
    serde_json::from_str(s).unwrap_or_default()
}

pub async fn list_providers(pool: &SqlitePool) -> Result<Vec<ProviderRow>> {
    let rows = sqlx::query_as::<_, ProviderDb>(
        "SELECT id, name, kind, base_url, api_key_ref, extra_headers, timeout_ms, is_active, \
         position, created_at, updated_at FROM providers ORDER BY position ASC, created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_provider(pool: &SqlitePool, id: &str) -> Result<Option<ProviderRow>> {
    let row = sqlx::query_as::<_, ProviderDb>(
        "SELECT id, name, kind, base_url, api_key_ref, extra_headers, timeout_ms, is_active, \
         position, created_at, updated_at FROM providers WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn create_provider(pool: &SqlitePool, input: ProviderInput) -> Result<ProviderRow> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let api_key_ref = if input.api_key_ref.is_empty() {
        id.clone()
    } else {
        input.api_key_ref.clone()
    };
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM providers")
        .fetch_one(pool)
        .await
        .unwrap_or(1);
    sqlx::query(
        "INSERT INTO providers \
         (id, name, kind, base_url, api_key_ref, extra_headers, timeout_ms, is_active, position, \
          created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(&input.kind)
    .bind(&input.base_url)
    .bind(&api_key_ref)
    .bind(serde_json::to_string(&input.extra_headers).unwrap_or_else(|_| "{}".into()))
    .bind(input.timeout_ms as i64)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(position)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(ProviderRow {
        id,
        name: input.name,
        kind: input.kind,
        base_url: input.base_url,
        api_key_ref,
        extra_headers: input.extra_headers,
        timeout_ms: input.timeout_ms,
        is_active: input.is_active,
        position: position as i32,
        created_at: now,
        updated_at: now,
    })
}

pub async fn update_provider(
    pool: &SqlitePool,
    id: &str,
    input: ProviderInput,
) -> Result<ProviderRow> {
    let now = now_ms();
    sqlx::query(
        "UPDATE providers SET name = ?1, kind = ?2, base_url = ?3, api_key_ref = ?4, \
         extra_headers = ?5, timeout_ms = ?6, is_active = ?7, updated_at = ?8 WHERE id = ?9",
    )
    .bind(&input.name)
    .bind(&input.kind)
    .bind(&input.base_url)
    .bind(&input.api_key_ref)
    .bind(serde_json::to_string(&input.extra_headers).unwrap_or_else(|_| "{}".into()))
    .bind(input.timeout_ms as i64)
    .bind(if input.is_active { 1 } else { 0 })
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    get_provider(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found: {id}"))
}

pub async fn delete_provider(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("UPDATE chats SET provider_id = NULL, model_id = NULL WHERE provider_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM models_cache WHERE provider_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM providers WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set position = index for each id in the given order.
pub async fn reorder_providers(pool: &SqlitePool, ordered_ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (i, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE providers SET position = ?1, updated_at = ?3 WHERE id = ?2")
            .bind(i as i64)
            .bind(id)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn set_provider_active(pool: &SqlitePool, id: &str, is_active: bool) -> Result<()> {
    sqlx::query("UPDATE providers SET is_active = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(if is_active { 1 } else { 0 })
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_provider_models(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<Vec<ProviderModel>> {
    let rows = sqlx::query_as::<_, ProviderModelDb>(
        "SELECT id, provider_id, name, display_name, enabled, alias, capabilities, context_window \
         FROM provider_models WHERE provider_id = ?1 ORDER BY name ASC",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Merge saved models of a provider by API name: rows with a known `name` are
/// updated in place (id preserved), unknown names are inserted, and rows whose
/// `name` is absent from the input are removed. Returns the ids of removed
/// models so the caller can clean dangling references.
pub async fn merge_provider_models(
    pool: &SqlitePool,
    provider_id: &str,
    models: &[ProviderModelInput],
) -> Result<Vec<String>> {
    let now = now_ms();
    let mut tx = pool.begin().await?;
    let existing: Vec<(String, String)> =
        sqlx::query_as("SELECT name, id FROM provider_models WHERE provider_id = ?1")
            .bind(provider_id)
            .fetch_all(&mut *tx)
            .await?;
    let mut existing_by_name: HashMap<String, String> = existing.into_iter().collect();
    for m in models {
        if let Some(id) = existing_by_name.remove(&m.name) {
            sqlx::query(
                "UPDATE provider_models SET display_name = ?1, enabled = ?2, alias = ?3, \
                 capabilities = ?4, context_window = ?5, updated_at = ?6 WHERE id = ?7",
            )
            .bind(&m.display_name)
            .bind(if m.enabled { 1 } else { 0 })
            .bind(&m.alias)
            .bind(serde_json::to_string(&m.capabilities).unwrap_or_else(|_| "[]".into()))
            .bind(m.context_window as i64)
            .bind(now)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO provider_models \
                 (id, provider_id, name, display_name, enabled, alias, capabilities, context_window, \
                  created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(provider_id)
            .bind(&m.name)
            .bind(&m.display_name)
            .bind(if m.enabled { 1 } else { 0 })
            .bind(&m.alias)
            .bind(serde_json::to_string(&m.capabilities).unwrap_or_else(|_| "[]".into()))
            .bind(m.context_window as i64)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }
    // Whatever is left in the map was not in the input: removed from the provider.
    let removed: Vec<(String, String)> = existing_by_name.into_iter().collect();
    for (name, id) in &removed {
        sqlx::query("DELETE FROM models_cache WHERE provider_id = ?1 AND name = ?2")
            .bind(provider_id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE chats SET model_id = NULL WHERE model_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM provider_models WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(removed.into_iter().map(|(_, id)| id).collect())
}

pub async fn get_model(pool: &SqlitePool, model_id: &str) -> Result<Option<ProviderModel>> {
    let row = sqlx::query_as::<_, ProviderModelDb>(
        "SELECT id, provider_id, name, display_name, enabled, alias, capabilities, context_window \
         FROM provider_models WHERE id = ?1",
    )
    .bind(model_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Look up a saved model scoped to its provider (kept for command-layer use).
#[allow(dead_code)]
pub async fn get_model_by_provider(
    pool: &SqlitePool,
    provider_id: &str,
    model_id: &str,
) -> Result<Option<ProviderModel>> {
    let row = sqlx::query_as::<_, ProviderModelDb>(
        "SELECT id, provider_id, name, display_name, enabled, alias, capabilities, context_window \
         FROM provider_models WHERE provider_id = ?1 AND id = ?2",
    )
    .bind(provider_id)
    .bind(model_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

#[derive(Debug, Clone, FromRow)]
struct JoinedModel {
    provider_id: String,
    provider_name: String,
    model_id: String,
    model_name: String,
    display_name: String,
    is_active: i64,
    enabled: i64,
}

impl From<JoinedModel> for ModelOption {
    fn from(r: JoinedModel) -> Self {
        ModelOption {
            provider_id: r.provider_id,
            provider_name: r.provider_name,
            model_id: r.model_id,
            model_name: r.model_name,
            display_name: r.display_name,
            is_active: r.is_active != 0,
            enabled: r.enabled != 0,
        }
    }
}

const SELECT_MODEL_OPTIONS: &str = "SELECT p.id AS provider_id, p.name AS provider_name, \
     m.id AS model_id, m.name AS model_name, m.display_name AS display_name, \
     p.is_active AS is_active, m.enabled AS enabled \
     FROM providers p JOIN provider_models m ON m.provider_id = p.id";

/// Enabled models of active providers (dropdown content).
pub async fn list_active_model_options(pool: &SqlitePool) -> Result<Vec<ModelOption>> {
    let rows = sqlx::query_as::<_, JoinedModel>(&format!(
        "{SELECT_MODEL_OPTIONS} WHERE p.is_active = 1 AND m.enabled = 1 \
         ORDER BY p.position ASC, m.name ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// All providers' enabled models, including inactive providers (greyed display).
pub async fn list_all_model_options(pool: &SqlitePool) -> Result<Vec<ModelOption>> {
    let rows = sqlx::query_as::<_, JoinedModel>(&format!(
        "{SELECT_MODEL_OPTIONS} WHERE m.enabled = 1 ORDER BY p.position ASC, m.name ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}
