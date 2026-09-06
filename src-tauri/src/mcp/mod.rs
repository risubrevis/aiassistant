pub mod client;
pub mod config;
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

use crate::db::mcp_servers;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct TestDefResult {
    pub ok: bool,
    pub tool_count: usize,
    pub tools: Vec<String>,
    pub error: Option<String>,
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
    pub async fn connect_all(&self) {
        let ids: Vec<String> = {
            let conns = self.conns.lock().await;
            conns
                .iter()
                .filter(|(_, c)| c.is_active && c.client.is_none())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in ids {
            if let Err(e) = self.connect(&id).await {
                warn!("mcp connect '{id}' failed: {e}");
                let mut conns = self.conns.lock().await;
                if let Some(c) = conns.get_mut(&id) {
                    c.status = Status::Error(e.to_string());
                }
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
        client
            .request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "aiassistant", "version": "1.0.0" }
                }),
            )
            .await?;
        let tools = client.tools_list().await.unwrap_or_default();
        Ok((client, tools))
    }

    pub async fn connect(&self, id: &str) -> Result<()> {
        let body = {
            let conns = self.conns.lock().await;
            conns
                .get(id)
                .map(|c| c.body.clone())
                .ok_or_else(|| anyhow::anyhow!("unknown server"))?
        };
        let (client, tools) = Self::build_and_probe(&body).await?;
        info!("mcp '{id}' connected: {} tool(s)", tools.len());
        let mut conns = self.conns.lock().await;
        if let Some(c) = conns.get_mut(id) {
            c.client = Some(client);
            c.tools = tools;
            c.status = Status::Connected;
        }
        Ok(())
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
            },
            Err(e) => TestDefResult {
                ok: false,
                tool_count: 0,
                tools: Vec::new(),
                error: Some(e.to_string()),
            },
        }
    }

    /// Drop existing clients and reconnect every configured server.
    /// In-flight calls are safe: `call_tool` clones the client Arc out of the lock.
    pub async fn recheck_all(&self) {
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
                if let Err(e) = self.connect(&id).await {
                    let mut conns = self.conns.lock().await;
                    if let Some(c) = conns.get_mut(&id) {
                        c.status = Status::Error(e.to_string());
                    }
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
        if let Some(c) = self.conns.lock().await.get_mut(id) {
            c.status = Status::Error(msg);
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
