use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::path::PathBuf;

pub mod agents;
pub mod attachments;
pub mod mcp_servers;
pub mod models;
pub mod project_tasks;
pub mod prompts;
pub mod providers;
pub mod tasks;

#[cfg(test)]
mod tests;

pub type DbPool = SqlitePool;

/// App data directory: `data_dir/aiassistant` per-OS (XDG/AppData/Library).
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aiassistant")
}

pub fn db_path() -> PathBuf {
    data_dir().join("app.db")
}

/// Open (creating if missing) and migrate the SQLite database.
pub async fn init() -> Result<DbPool> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(opts)
        .await
        .with_context(|| format!("failed to open db at {}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run migrations")?;
    tracing::info!("database ready at {}", path.display());
    Ok(pool)
}
