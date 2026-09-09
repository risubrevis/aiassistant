use anyhow::Result;
use sqlx::{FromRow, SqlitePool};

use super::models::now_ms;

/// Raw DB row of `mcp_oauth`.
#[derive(Debug, Clone, FromRow)]
pub struct McpOAuthRow {
    #[allow(dead_code)] // primary key of the row, not read back in Rust
    pub server_id: String,
    pub auth_server_issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub token_endpoint: String,
    pub authorization_endpoint: String,
    pub registration_endpoint: String,
    pub revocation_endpoint: String,
    pub scopes: String,
    pub redirect_uri: String,
    pub expires_at: i64,
    pub has_refresh_token: i64,
    #[allow(dead_code)] // SQL-managed timestamps
    pub created_at: i64,
    #[allow(dead_code)] // SQL-managed timestamps
    pub updated_at: i64,
}

impl McpOAuthRow {
    pub fn has_refresh_token_bool(&self) -> bool {
        self.has_refresh_token != 0
    }
}

/// Upsert payload; timestamps are managed by the DB layer.
#[derive(Debug, Clone, Default)]
pub struct McpOAuthInput {
    pub server_id: String,
    pub auth_server_issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub token_endpoint: String,
    pub authorization_endpoint: String,
    pub registration_endpoint: String,
    pub revocation_endpoint: String,
    pub scopes: String,
    pub redirect_uri: String,
    pub expires_at: i64,
    pub has_refresh_token: bool,
}

impl McpOAuthInput {
    pub fn from_parts(
        server_id: &str,
        metadata: &crate::mcp::oauth::AuthServerMetadata,
        registration: &crate::mcp::oauth::ClientRegistration,
        scopes: &str,
        redirect_uri: &str,
        expires_at: i64,
        has_refresh_token: bool,
    ) -> Self {
        Self {
            server_id: server_id.to_string(),
            auth_server_issuer: metadata.issuer.clone(),
            client_id: registration.client_id.clone(),
            client_secret: registration.client_secret.clone().unwrap_or_default(),
            token_endpoint: metadata.token_endpoint.clone(),
            authorization_endpoint: metadata.authorization_endpoint.clone(),
            registration_endpoint: metadata.registration_endpoint.clone().unwrap_or_default(),
            revocation_endpoint: metadata.revocation_endpoint.clone().unwrap_or_default(),
            scopes: scopes.to_string(),
            redirect_uri: redirect_uri.to_string(),
            expires_at,
            has_refresh_token,
        }
    }
}

pub async fn upsert(pool: &SqlitePool, input: &McpOAuthInput) -> Result<()> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO mcp_oauth \
         (server_id, auth_server_issuer, client_id, client_secret, token_endpoint, \
          authorization_endpoint, registration_endpoint, revocation_endpoint, scopes, \
          redirect_uri, expires_at, has_refresh_token, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) \
         ON CONFLICT(server_id) DO UPDATE SET \
         auth_server_issuer = ?2, client_id = ?3, client_secret = ?4, token_endpoint = ?5, \
         authorization_endpoint = ?6, registration_endpoint = ?7, revocation_endpoint = ?8, \
         scopes = ?9, redirect_uri = ?10, expires_at = ?11, has_refresh_token = ?12, \
         updated_at = ?14",
    )
    .bind(&input.server_id)
    .bind(&input.auth_server_issuer)
    .bind(&input.client_id)
    .bind(&input.client_secret)
    .bind(&input.token_endpoint)
    .bind(&input.authorization_endpoint)
    .bind(&input.registration_endpoint)
    .bind(&input.revocation_endpoint)
    .bind(&input.scopes)
    .bind(&input.redirect_uri)
    .bind(input.expires_at)
    .bind(if input.has_refresh_token { 1 } else { 0 })
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, server_id: &str) -> Result<Option<McpOAuthRow>> {
    let row = sqlx::query_as::<_, McpOAuthRow>(
        "SELECT server_id, auth_server_issuer, client_id, client_secret, token_endpoint, \
         authorization_endpoint, registration_endpoint, revocation_endpoint, scopes, \
         redirect_uri, expires_at, has_refresh_token, created_at, updated_at \
         FROM mcp_oauth WHERE server_id = ?1",
    )
    .bind(server_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(dead_code)] // no in-app flow deletes OAuth metadata (FK cascade covers it)
pub async fn delete(pool: &SqlitePool, server_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM mcp_oauth WHERE server_id = ?1")
        .bind(server_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_tokens(
    pool: &SqlitePool,
    server_id: &str,
    expires_at: i64,
    has_refresh_token: bool,
) -> Result<()> {
    sqlx::query(
        "UPDATE mcp_oauth SET expires_at = ?1, has_refresh_token = ?2, updated_at = ?3 \
         WHERE server_id = ?4",
    )
    .bind(expires_at)
    .bind(if has_refresh_token { 1 } else { 0 })
    .bind(now_ms())
    .bind(server_id)
    .execute(pool)
    .await?;
    Ok(())
}
