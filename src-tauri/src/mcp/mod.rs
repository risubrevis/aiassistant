pub mod client;
pub mod config;
pub mod oauth;
pub mod webui;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::db::{mcp_oauth, mcp_servers};
use crate::secrets;
use crate::tools::{Tool, ToolCategory, ToolResult, ToolSpec};

use client::{DynClient, HttpClient, McpTool, StdioClient};

pub use config::McpBody;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disabled,
    Connecting,
    Connected,
    Error(String),
    /// OAuth required: the user must sign in (button in the UI).
    NeedsAuth,
}

struct Conn {
    id: String,
    title: String,
    name: String,
    body: McpBody,
    is_active: bool,
    webui_url: String,
    webui_icon: String,
    position: i32,
    status: Status,
    /// Last OAuth check found the access token expired with no usable refresh token.
    oauth_expired: bool,
    /// True when the server is connected via an OAuth bearer token.
    oauth_authenticated: bool,
    transport: String,
    tools: Vec<McpTool>,
    client: Option<DynClient>,
}

impl Conn {
    fn from_row(row: &mcp_servers::McpServerRow) -> Self {
        let body = row.body();
        let transport = if body.command.is_some() {
            "stdio".to_string()
        } else {
            "http".to_string()
        };
        let status = if row.is_active_bool() {
            Status::Connecting
        } else {
            Status::Disabled
        };
        Self {
            id: row.id.clone(),
            title: row.title.clone(),
            name: row.name.clone(),
            body,
            is_active: row.is_active_bool(),
            webui_url: row.webui_url.clone(),
            webui_icon: row.webui_icon.clone(),
            position: row.position as i32,
            status,
            oauth_expired: false,
            oauth_authenticated: false,
            transport,
            tools: Vec::new(),
            client: None,
        }
    }
}

/// MCP host manager: holds connections to all configured servers (docs/06).
#[derive(Clone)]
pub struct McpManager {
    conns: Arc<Mutex<HashMap<String, Conn>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerInfo {
    pub id: String,
    pub name: String,
    pub title: String,
    pub is_active: bool,
    pub transport: String,
    pub status: Status,
    pub tool_count: usize,
    pub tools: Vec<String>,
    pub body: McpBody,
    pub webui_url: String,
    pub webui_icon: String,
    pub position: i32,
    /// True if the server requires OAuth and isn't authenticated.
    pub needs_auth: bool,
    /// True if the OAuth token is expired and can't be refreshed.
    pub auth_expired: bool,
    /// True when the server is currently connected via OAuth.
    pub oauth_authenticated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestDefResult {
    pub ok: bool,
    pub tool_count: usize,
    pub tools: Vec<String>,
    pub error: Option<String>,
    /// True when the probe received a 401, hinting that OAuth is required.
    pub needs_auth: bool,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            conns: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Load server rows from the DB and populate the connection map (without connecting).
    pub async fn reload(&self, pool: &SqlitePool) -> Result<()> {
        let rows = mcp_servers::list(pool).await?;
        let mut conns = self.conns.lock().await;
        conns.clear();
        for row in &rows {
            conns.insert(row.id.clone(), Conn::from_row(row));
        }
        Ok(())
    }

    /// Connect to all active servers (best-effort; logs errors).
    pub async fn connect_all(&self, pool: &SqlitePool) {
        let ids: Vec<String> = {
            let conns = self.conns.lock().await;
            conns
                .iter()
                .filter(|(_, c)| c.is_active && c.client.is_none())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in ids {
            if let Err(e) = self.connect(&id, pool).await {
                warn!("mcp connect '{id}' failed: {e}");
            }
        }
    }

    /// Build a client from `body` (stdio or http), run initialize + tools/list, return both.
    async fn build_and_probe(body: &McpBody) -> Result<(DynClient, Vec<McpTool>)> {
        let client: DynClient = if let Some(cmd) = &body.command {
            Arc::new(StdioClient::spawn(cmd, &body.args, &body.env)?)
        } else if let Some(url) = &body.url {
            Arc::new(HttpClient::new(url.clone(), &body.headers)?)
        } else {
            return Err(anyhow::anyhow!("server has neither command nor url"));
        };
        let tools = Self::probe(&client).await?;
        Ok((client, tools))
    }

    /// `initialize` + `tools/list` against a freshly built client.
    async fn probe(client: &DynClient) -> Result<Vec<McpTool>> {
        client
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "aiassistant", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        Ok(client.tools_list().await.unwrap_or_default())
    }

    /// Connect to one server. HTTP transports negotiate OAuth automatically:
    /// stored tokens are presented, expired ones refreshed, and a 401 without
    /// stored tokens triggers authorization-server discovery (metadata only).
    /// Status is fully managed here; callers must not override it on error.
    pub async fn connect(&self, id: &str, pool: &SqlitePool) -> Result<()> {
        let body = {
            let conns = self.conns.lock().await;
            conns
                .get(id)
                .map(|c| c.body.clone())
                .ok_or_else(|| anyhow::anyhow!("unknown server"))?
        };
        if body.command.is_some() {
            return self.connect_simple(id, &body).await;
        }
        let Some(url) = body.url.clone() else {
            let err = anyhow::anyhow!("server has neither command nor url");
            self.set_error(id, err.to_string()).await;
            return Err(err);
        };

        if let Some(oauth) = mcp_oauth::get(pool, id).await.ok().flatten() {
            if !oauth.client_id.is_empty() {
                return self.connect_oauth(id, &body, &oauth, pool).await;
            }
        }

        // No OAuth registration yet — connect plainly and detect a 401 challenge.
        match self.connect_simple(id, &body).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let err_str = e.to_string();
                if !is_auth_error(&err_str) {
                    return Err(e);
                }
                match oauth::discover(&url).await {
                    Ok(metadata) => {
                        let scopes = metadata.scopes_supported.join(" ");
                        let existing = mcp_oauth::get(pool, id).await.ok().flatten();
                        let input = mcp_oauth::McpOAuthInput::from_parts(
                            id,
                            &metadata,
                            &oauth::ClientRegistration {
                                client_id: String::new(),
                                client_secret: None,
                            },
                            &scopes,
                            existing
                                .as_ref()
                                .map(|o| o.redirect_uri.as_str())
                                .unwrap_or_default(),
                            0,
                            false,
                        );
                        let _ = mcp_oauth::upsert(pool, &input).await;
                        self.set_needs_auth(id, false).await;
                        Err(anyhow::anyhow!(
                            "OAuth authentication required for this server"
                        ))
                    }
                    // Discovery failed — keep the original transport error.
                    Err(_) => Err(e),
                }
            }
        }
    }

    /// Plain connect (stdio or unauthenticated HTTP); sets status on any outcome.
    async fn connect_simple(&self, id: &str, body: &McpBody) -> Result<()> {
        match Self::build_and_probe(body).await {
            Ok((client, tools)) => {
                info!("mcp '{id}' connected: {} tool(s)", tools.len());
                let mut conns = self.conns.lock().await;
                if let Some(c) = conns.get_mut(id) {
                    c.client = Some(client);
                    c.tools = tools;
                    c.status = Status::Connected;
                    c.oauth_authenticated = false;
                }
                Ok(())
            }
            Err(e) => {
                self.set_error(id, e.to_string()).await;
                Err(e)
            }
        }
    }

    /// HTTP connect with an OAuth-registered client: refresh an expired token
    /// via the refresh grant, else present the stored access token.
    async fn connect_oauth(
        &self,
        id: &str,
        body: &McpBody,
        oauth: &mcp_oauth::McpOAuthRow,
        pool: &SqlitePool,
    ) -> Result<()> {
        let client_secret = if oauth.client_secret.is_empty() {
            None
        } else {
            Some(oauth.client_secret.clone())
        };
        let is_expired =
            oauth.expires_at > 0 && chrono::Utc::now().timestamp_millis() >= oauth.expires_at;

        if !is_expired {
            if let Some(token) = secrets::get_mcp_oauth_token(id, "access") {
                return self.connect_with_token(id, body, &token).await;
            }
        } else if oauth.has_refresh_token_bool() {
            if let Some(rt) = secrets::get_mcp_oauth_token(id, "refresh") {
                match oauth::refresh_access_token(
                    &oauth.token_endpoint,
                    &oauth.client_id,
                    client_secret.as_deref(),
                    &rt,
                )
                .await
                {
                    Ok(tokens) => {
                        Self::store_tokens(id, &tokens, pool, oauth.has_refresh_token_bool()).await;
                        info!("mcp '{id}' oauth token refreshed");
                        return self
                            .connect_with_token(id, body, &tokens.access_token)
                            .await;
                    }
                    Err(e) => warn!("mcp oauth refresh for '{id}' failed: {e}"),
                }
            }
        }

        if is_expired {
            self.set_needs_auth(id, true).await;
            Err(anyhow::anyhow!(
                "OAuth token expired, re-authentication required"
            ))
        } else {
            self.set_needs_auth(id, false).await;
            Err(anyhow::anyhow!("OAuth authentication required"))
        }
    }

    /// HTTP connect with an `Authorization: Bearer` header (the OAuth token
    /// wins over any user-configured header of the same name).
    async fn connect_with_token(&self, id: &str, body: &McpBody, token: &str) -> Result<()> {
        let url = body
            .url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("server has no url"))?;
        let mut headers = body.headers.clone();
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        let client: DynClient = Arc::new(HttpClient::new(url, &headers)?);
        match Self::probe(&client).await {
            Ok(tools) => {
                info!("mcp '{id}' connected (oauth): {} tool(s)", tools.len());
                let mut conns = self.conns.lock().await;
                if let Some(c) = conns.get_mut(id) {
                    c.client = Some(client);
                    c.tools = tools;
                    c.status = Status::Connected;
                    c.oauth_authenticated = true;
                }
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if is_auth_error(&msg) {
                    self.set_needs_auth(id, false).await;
                } else {
                    self.set_error(id, msg).await;
                }
                Err(e)
            }
        }
    }

    /// Persist a fresh token set: tokens in the OS keychain, expiry in the DB.
    async fn store_tokens(
        id: &str,
        tokens: &oauth::TokenSet,
        pool: &SqlitePool,
        had_refresh: bool,
    ) {
        let _ = secrets::set_mcp_oauth_token(id, "access", &tokens.access_token);
        if let Some(rt) = &tokens.refresh_token {
            let _ = secrets::set_mcp_oauth_token(id, "refresh", rt);
        }
        let has_refresh = tokens.refresh_token.is_some() || had_refresh;
        if let Err(e) = mcp_oauth::update_tokens(pool, id, tokens.expires_at, has_refresh).await {
            warn!("mcp oauth token persist for '{id}' failed: {e}");
        }
    }

    pub async fn list(&self) -> Vec<McpServerInfo> {
        let conns = self.conns.lock().await;
        let mut rows: Vec<&Conn> = conns.values().collect();
        rows.sort_by_key(|c| c.position);
        rows.iter()
            .map(|c| McpServerInfo {
                id: c.id.clone(),
                name: c.name.clone(),
                title: c.title.clone(),
                is_active: c.is_active,
                transport: c.transport.clone(),
                status: c.status.clone(),
                tool_count: c.tools.len(),
                tools: c.tools.iter().map(|t| t.name.clone()).collect(),
                body: c.body.clone(),
                webui_url: c.webui_url.clone(),
                webui_icon: c.webui_icon.clone(),
                position: c.position,
                needs_auth: matches!(c.status, Status::NeedsAuth),
                auth_expired: c.oauth_expired,
                oauth_authenticated: c.oauth_authenticated,
            })
            .collect()
    }

    /// Probe a transport spec without persisting or touching existing connections.
    pub async fn test_def(&self, body: &McpBody) -> TestDefResult {
        match Self::build_and_probe(body).await {
            Ok((_client, tools)) => TestDefResult {
                ok: true,
                tool_count: tools.len(),
                tools: tools.iter().map(|t| t.name.clone()).collect(),
                error: None,
                needs_auth: false,
            },
            Err(e) => {
                let err_str = e.to_string();
                TestDefResult {
                    ok: false,
                    tool_count: 0,
                    tools: Vec::new(),
                    error: Some(err_str.clone()),
                    needs_auth: is_auth_error(&err_str),
                }
            }
        }
    }

    /// Drop existing clients and reconnect every configured server.
    /// In-flight calls are safe: `call_tool` clones the client Arc out of the lock.
    pub async fn recheck_all(&self, pool: &SqlitePool) {
        let ids: Vec<(String, bool)> = {
            let conns = self.conns.lock().await;
            conns
                .iter()
                .map(|(k, c)| (k.clone(), c.is_active))
                .collect()
        };
        for (id, is_active) in ids {
            {
                let mut conns = self.conns.lock().await;
                if let Some(c) = conns.get_mut(&id) {
                    c.client = None;
                    c.tools.clear();
                    c.status = if is_active {
                        Status::Connecting
                    } else {
                        Status::Disabled
                    };
                }
            }
            if is_active {
                if let Err(e) = self.connect(&id, pool).await {
                    warn!("mcp recheck '{id}' failed: {e}");
                }
            }
        }
    }

    /// Replace the in-memory state of one server from a DB row (no reconnect).
    pub async fn sync_server(&self, row: &mcp_servers::McpServerRow) {
        let mut conns = self.conns.lock().await;
        conns.insert(row.id.clone(), Conn::from_row(row));
    }

    pub async fn delete_server(&self, id: &str) {
        self.conns.lock().await.remove(id);
    }

    /// Update only the in-memory webui fields of a server (no reconnect).
    pub async fn set_webui(&self, id: &str, webui_url: &str, webui_icon: &str) {
        if let Some(c) = self.conns.lock().await.get_mut(id) {
            c.webui_url = webui_url.to_string();
            c.webui_icon = webui_icon.to_string();
        }
    }

    /// Mark a server as failed after a connect attempt (keeps the conn).
    pub async fn set_error(&self, id: &str, msg: String) {
        let mut conns = self.conns.lock().await;
        if let Some(c) = conns.get_mut(id) {
            c.status = Status::Error(msg);
            c.oauth_expired = false;
        }
    }

    /// Mark a server as awaiting OAuth sign-in (`expired` = token lapsed and
    /// can't be refreshed).
    pub async fn set_needs_auth(&self, id: &str, expired: bool) {
        let mut conns = self.conns.lock().await;
        if let Some(c) = conns.get_mut(id) {
            c.status = Status::NeedsAuth;
            c.oauth_expired = expired;
        }
    }

    /// Apply an active-flag change to the in-memory conn (command handles connect).
    pub async fn set_active(&self, id: &str, is_active: bool) {
        let mut conns = self.conns.lock().await;
        if let Some(c) = conns.get_mut(id) {
            c.is_active = is_active;
            if is_active {
                c.status = Status::Connecting;
            } else {
                c.client = None;
                c.tools.clear();
                c.status = Status::Disabled;
            }
        }
    }

    /// Apply a new ordering to the in-memory conns.
    pub async fn set_positions(&self, ordered_ids: &[String]) {
        let mut conns = self.conns.lock().await;
        for (i, id) in ordered_ids.iter().enumerate() {
            if let Some(c) = conns.get_mut(id) {
                c.position = i as i32;
            }
        }
    }

    /// Call an MCP tool, returning a builtin-style ToolResult.
    pub async fn call_tool(&self, id: &str, tool: &str, args: Value) -> ToolResult {
        let client = {
            let conns = self.conns.lock().await;
            conns.get(id).and_then(|c| c.client.clone())
        };
        let Some(client) = client else {
            return ToolResult::err(format!("mcp server not connected: {id}"));
        };
        match client.call_tool(tool, args).await {
            Ok(r) => ToolResult {
                content: r.to_text(),
                is_error: r.is_error,
            },
            Err(e) => ToolResult::err(format!("mcp call failed: {e}")),
        }
    }

    /// Add active+connected MCP tools to the registry (mode-filtered).
    pub async fn add_to_registry(&self, registry: &mut crate::tools::Registry, mode: &str) {
        // Minimal mode = bare model: no tools are offered at all (docs/12).
        if mode == "minimal" {
            return;
        }
        let conns = self.conns.lock().await;
        for c in conns.values() {
            if !c.is_active || c.client.is_none() {
                continue;
            }
            for t in &c.tools {
                let category = category_for(&t.name);
                if mode == "plan" && !matches!(category, ToolCategory::Readonly) {
                    continue;
                }
                registry.register(Box::new(McpToolAdapter {
                    manager: self.clone(),
                    id: c.id.clone(),
                    name: c.name.clone(),
                    tool: t.clone(),
                    category,
                }));
            }
        }
    }
}

/// HTTP 401 responses from `HttpClient` render as "HTTP 401 …"; treat them as
/// an auth challenge.
fn is_auth_error(msg: &str) -> bool {
    msg.contains("HTTP 401") || msg.contains("Unauthorized")
}

fn category_for(name: &str) -> ToolCategory {
    let n = name.to_lowercase();
    if n.contains("delete") || n.contains("remove") || n.contains("drop") || n.contains("destroy") {
        ToolCategory::Destructive
    } else if n.contains("exec")
        || n.contains("run")
        || n.contains("command")
        || n.contains("shell")
        || n.contains("bash")
    {
        ToolCategory::Exec
    } else if n.contains("write")
        || n.contains("edit")
        || n.contains("create")
        || n.contains("move")
        || n.contains("patch")
        || n.contains("mkdir")
        || n.contains("set_")
    {
        ToolCategory::Write
    } else {
        ToolCategory::Readonly
    }
}

/// Adapter exposing an MCP tool through the builtin `Tool` trait.
struct McpToolAdapter {
    manager: McpManager,
    id: String,
    name: String,
    tool: McpTool,
    category: ToolCategory,
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn category(&self) -> ToolCategory {
        self.category
    }
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: format!("mcp__{}__{}", self.name, self.tool.name),
            description: self.tool.description.clone(),
            parameters: self.tool.input_schema.clone(),
        }
    }
    async fn execute(&self, args: Value) -> ToolResult {
        self.manager
            .call_tool(&self.id, &self.tool.name, args)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::category_for;
    use crate::tools::ToolCategory;

    #[test]
    fn category_heuristic() {
        assert_eq!(category_for("read_file"), ToolCategory::Readonly);
        assert_eq!(category_for("write_file"), ToolCategory::Write);
        assert_eq!(category_for("edit_file"), ToolCategory::Write);
        assert_eq!(category_for("delete_file"), ToolCategory::Destructive);
        assert_eq!(category_for("run_command"), ToolCategory::Exec);
        assert_eq!(category_for("list_directory"), ToolCategory::Readonly);
    }
}
