use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::Mutex;

/// A discovered MCP tool (subset of the spec we need).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

/// JSON-RPC transport-agnostic client.
#[async_trait]
pub trait McpClient: Send + Sync {
    async fn request(&self, method: &str, params: Value) -> Result<Value>;
    async fn tools_list(&self) -> Result<Vec<McpTool>> {
        let res = self.request("tools/list", json!({})).await?;
        let tools = res
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("tools/list: missing 'tools'"))?
            .clone();
        serde_json::from_value(Value::Array(tools)).map_err(|e| anyhow!("parse tools: {e}"))
    }
    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult> {
        let res = self
            .request("tools/call", json!({ "name": name, "arguments": args }))
            .await?;
        Ok(serde_json::from_value(res).unwrap_or(McpToolResult {
            content: vec![],
            is_error: false,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpToolResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
    #[serde(other)]
    Other,
}

impl McpToolResult {
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for c in &self.content {
            if let McpContent::Text { text } = c {
                out.push_str(text);
                out.push('\n');
            }
        }
        if out.is_empty() {
            "(no text content)".into()
        } else {
            out
        }
    }
}

struct StdioInner {
    stdin: Option<tokio::process::ChildStdin>,
    reader: tokio::io::BufReader<tokio::process::ChildStdout>,
    id: u64,
    _child: Child,
}

/// stdio transport: newline-delimited JSON-RPC over the child process.
pub struct StdioClient {
    inner: Mutex<StdioInner>,
}

impl StdioClient {
    pub fn spawn(command: &str, args: &[String], env: &HashMap<String, String>) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("spawn '{command}' failed: {e}"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        Ok(Self {
            inner: Mutex::new(StdioInner {
                stdin: Some(stdin),
                reader: tokio::io::BufReader::new(stdout),
                id: 0,
                _child: child,
            }),
        })
    }
}

#[async_trait]
impl McpClient for StdioClient {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let mut inner = self.inner.lock().await;
        inner.id += 1;
        let id = inner.id;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let line = serde_json::to_string(&req)? + "\n";
        let stdin = inner
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("stdio client closed"))?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;

        loop {
            let mut buf = String::new();
            let n = inner.reader.read_line(&mut buf).await?;
            if n == 0 {
                return Err(anyhow!("stdio EOF"));
            }
            let val: Value = match serde_json::from_str(buf.trim()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if val.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(err) = val.get("error") {
                    return Err(anyhow!("rpc error: {err}"));
                }
                return Ok(val.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

/// Streamable HTTP transport (MCP spec 2025-03-26 / "HTTP transport"). POSTs
/// JSON-RPC with `Accept: application/json, text/event-stream`; the server may
/// answer with a single JSON object or an SSE stream. Supports the older SSE
/// transport too (single endpoint POST). Tracks `Mcp-Session-Id` when present.
pub struct HttpClient {
    url: String,
    client: reqwest::Client,
    extra_headers: HeaderMap,
    id: Mutex<u64>,
    session: Mutex<Option<String>>,
}

impl HttpClient {
    pub fn new(url: String, headers: &HashMap<String, String>) -> Result<Self> {
        let mut h = HeaderMap::new();
        for (k, v) in headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                h.insert(name, val);
            }
        }
        let client =
            crate::net::apply(reqwest::Client::builder().timeout(Duration::from_secs(120)))
                .build()?;
        Ok(Self {
            url,
            client,
            extra_headers: h,
            id: Mutex::new(0),
            session: Mutex::new(None),
        })
    }
}

#[async_trait]
impl McpClient for HttpClient {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = {
            let mut g = self.id.lock().await;
            *g += 1;
            *g
        };
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });

        let mut req = self
            .client
            .post(&self.url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .headers(self.extra_headers.clone())
            .json(&body);
        if let Some(sid) = self.session.lock().await.clone() {
            if let Ok(v) = HeaderValue::from_str(&sid) {
                req = req.header("mcp-session-id", v);
            }
        }

        let resp = req.send().await.map_err(|e| anyhow!("http {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {status}: {text}"));
        }

        // Capture session id if the server uses sessions.
        if let Some(sid) = resp.headers().get("mcp-session-id") {
            if let Ok(s) = sid.to_str() {
                *self.session.lock().await = Some(s.to_string());
            }
        }

        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ct.contains("text/event-stream") {
            // SSE response: find the JSON-RPC message matching our id.
            let mut stream = resp.bytes_stream().eventsource();
            while let Some(event) = stream.next().await {
                let event = event.map_err(|e| anyhow!("sse: {e}"))?;
                if event.data.trim().is_empty() {
                    continue;
                }
                let val: Value = match serde_json::from_str(event.data.trim()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if val.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    if let Some(err) = val.get("error") {
                        return Err(anyhow!("rpc error: {err}"));
                    }
                    return Ok(val.get("result").cloned().unwrap_or(Value::Null));
                }
            }
            Err(anyhow!("sse stream ended without a matching response"))
        } else {
            // Plain JSON response.
            let val: Value = resp.json().await.map_err(|e| anyhow!("http parse {e}"))?;
            if let Some(err) = val.get("error") {
                return Err(anyhow!("rpc error: {err}"));
            }
            Ok(val.get("result").cloned().unwrap_or(Value::Null))
        }
    }
}

pub type DynClient = Arc<dyn McpClient>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires network + a public Streamable-HTTP MCP server"]
    async fn context7_streamable_http() {
        let c = HttpClient::new("https://mcp.context7.com/mcp".into(), &HashMap::new()).unwrap();
        let _ = c
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "aiassistant", "version": "1.0.0" }
                }),
            )
            .await
            .expect("initialize");
        let tools = c.tools_list().await.expect("tools/list");
        println!("context7 tools: {}", tools.len());
        for t in tools.iter().take(5) {
            println!("  - {}", t.name);
        }
        assert!(!tools.is_empty(), "context7 returned no tools");
    }
}
