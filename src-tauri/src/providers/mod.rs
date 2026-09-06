pub mod anthropic;
pub mod openai;

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::time::timeout;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;

/// A single message in a completion request (OpenAI-style roles).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// OpenAI tool-call id carried on `tool`-role messages (for tool results).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// OpenAI assistant `tool_calls` (reconstructed from persisted tool_use blocks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Multi-part content (text + base64 image data) for vision-capable models;
    /// providers serialize these into their native multipart format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<ContentPart>>,
}

/// Multipart message content: inline text segments and base64-encoded images
/// (raw base64, no `data:` prefix) for vision-capable models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { media_type: String, data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// OpenAI `tools` array (`[{type:"function", function:{...}}]`), if any.
    pub tools: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Thinking,
    Text,
    ToolUse,
}

/// Info attached to a `tool_use` block start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseInfo {
    pub name: String,
    pub tool_call_id: String,
}

/// Normalized content-block stream events (docs/13). MVP carries
/// thinking / text / tool_use blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompleteEvent {
    BlockStart {
        block_id: String,
        block_type: BlockType,
        #[serde(skip_serializing_if = "Option::is_none")]
        info: Option<ToolUseInfo>,
    },
    BlockDelta {
        block_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        partial_json: Option<String>,
    },
    BlockStop {
        block_id: String,
    },
    Done {
        finish_reason: String,
        usage: Option<Usage>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    /// Wall time from request send to Done (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_ms: Option<u64>,
    /// Wall time from request send to first token (ms); ollama's
    /// `load_duration` + `prompt_eval_duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_to_first_token_ms: Option<u64>,
    /// Wall time from first token to Done (ms); ollama's `eval_duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_duration_ms: Option<u64>,
}

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum ProviderError {
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("rate limited")]
    RateLimit,
    #[error("insufficient quota")]
    InsufficientQuota,
    #[error("context length exceeded")]
    ContextLength,
    #[error("content filtered")]
    ContentFilter,
    #[error("network error: {0}")]
    Network(String),
    #[error("server error: {0}")]
    Server(String),
    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid response: {0}")]
    Parse(String),
}

impl ProviderError {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Auth(_) => "auth",
            Self::RateLimit => "rate_limit",
            Self::InsufficientQuota => "insufficient_quota",
            Self::ContextLength => "context_length",
            Self::ContentFilter => "content_filter",
            Self::Network(_) => "network",
            Self::Server(_) => "server",
            Self::Status { .. } => "status",
            Self::Parse(_) => "parse",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Auth(m) => format!("Authentication failed: {m}"),
            Self::RateLimit => "Rate limit reached — wait and retry.".into(),
            Self::InsufficientQuota => {
                "Insufficient quota or out of credits. Add credits or wait for the quota to reset."
                    .into()
            }
            Self::ContextLength => {
                "Context length exceeded. Compact or shorten the conversation and retry.".into()
            }
            Self::ContentFilter => "Content blocked by the provider's safety policy.".into(),
            Self::Network(m) => format!("Network error: {m}"),
            Self::Server(m) => format!("Server error: {m}"),
            Self::Status { status, body } => format!("HTTP {status}: {body}"),
            Self::Parse(m) => format!("Invalid response: {m}"),
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit
                | Self::InsufficientQuota
                | Self::ContextLength
                | Self::Network(_)
                | Self::Server(_)
        )
    }
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Classify an error response from an OpenAI-compatible API (OpenAI,
/// OpenRouter, Mistral, DeepSeek, Groq, xAI, Gemini-OAI, Ollama /v1).
pub fn classify_openai_error(status: u16, body: &str) -> ProviderError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error_obj = parsed.as_ref().and_then(|v| v.get("error"));
    let message = error_obj
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|v| v.get("message"))
                .and_then(Value::as_str)
        })
        .unwrap_or(body)
        .to_string();
    let code = error_obj.and_then(|e| e.get("code")).and_then(|c| {
        c.as_str()
            .map(str::to_string)
            .or_else(|| Some(c.to_string()))
    });
    let etype = error_obj
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let meta_type = error_obj
        .and_then(|e| e.get("metadata"))
        .and_then(|m| m.get("error_type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let lc_message = message.to_lowercase();
    let signals = format!(
        "{} {etype} {meta_type} {lc_message}",
        code.as_deref().unwrap_or("").to_lowercase()
    );

    if status == 401 {
        return ProviderError::Auth(message);
    }
    if status == 402 {
        return ProviderError::InsufficientQuota;
    }
    if status >= 500 {
        return ProviderError::Server(message);
    }
    if has_any(
        &signals,
        &[
            "insufficient_quota",
            "credit_balance_exhausted",
            "spend_limit_exceeded",
            "usage_limit_exceeded",
            "payment_required",
        ],
    ) {
        return ProviderError::InsufficientQuota;
    }
    if has_any(&signals, &["context_length_exceeded"]) {
        return ProviderError::ContextLength;
    }
    if has_any(
        &signals,
        &[
            "content_filter",
            "content_policy_violation",
            "refusal",
            "moderation_flag",
        ],
    ) {
        return ProviderError::ContentFilter;
    }
    if status == 429 {
        return if has_any(
            &lc_message,
            &["quota", "credit", "balance", "billing", "insufficient"],
        ) {
            ProviderError::InsufficientQuota
        } else {
            ProviderError::RateLimit
        };
    }
    if has_any(
        &lc_message,
        &[
            "maximum context length",
            "context length",
            "too long",
            "exceeds context",
            "request too large",
        ],
    ) {
        return ProviderError::ContextLength;
    }
    if has_any(
        &lc_message,
        &[
            "safety",
            "moderation",
            "content policy",
            "flagged",
            "blocked",
            "guardrail",
            "refusal",
        ],
    ) {
        return ProviderError::ContentFilter;
    }
    if has_any(
        &lc_message,
        &["quota", "insufficient", "credit", "balance", "billing"],
    ) {
        return ProviderError::InsufficientQuota;
    }
    if has_any(
        &lc_message,
        &["rate limit", "too many requests", "overloaded", "capacity"],
    ) {
        return ProviderError::RateLimit;
    }
    if status == 413 {
        return ProviderError::ContextLength;
    }
    ProviderError::Status {
        status,
        body: message,
    }
}

/// Classify an error response from the native Anthropic Messages API
/// (`{"type":"error","error":{"type":"...","message":"..."}}`).
pub fn classify_anthropic_error(status: u16, body: &str) -> ProviderError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or(body)
        .to_string();
    let etype = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let lc_message = message.to_lowercase();

    if status == 401 {
        return ProviderError::Auth(message);
    }
    if status == 402 {
        return ProviderError::InsufficientQuota;
    }
    if status == 529 || status >= 500 {
        return ProviderError::Server(message);
    }
    if etype == "billing_error" || has_any(&lc_message, &["billing", "credit", "balance", "quota"])
    {
        return ProviderError::InsufficientQuota;
    }
    if etype == "rate_limit_error" || status == 429 {
        return ProviderError::RateLimit;
    }
    if has_any(
        &lc_message,
        &["prompt is too long", "context", "too long", "maximum"],
    ) {
        return ProviderError::ContextLength;
    }
    if has_any(
        &lc_message,
        &["safety", "refusal", "blocked", "content policy"],
    ) {
        return ProviderError::ContentFilter;
    }
    if status == 413 {
        return ProviderError::ContextLength;
    }
    ProviderError::Status {
        status,
        body: message,
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    /// Stream a completion. Emits block events + a final `Done` via `sink`.
    async fn stream_complete(
        &self,
        req: CompleteRequest,
        sink: tokio::sync::mpsc::Sender<CompleteEvent>,
    ) -> Result<Usage, ProviderError>;
}

/// Build a provider instance from a config entry, resolving the API key from
/// the OS keychain (docs/01). Returns `None` for unsupported kinds.
pub fn build(cfg: &crate::config::Provider) -> Option<Box<dyn Provider>> {
    let api_key = if cfg.api_key_ref.is_empty() {
        None
    } else {
        crate::secrets::get_api_key(&cfg.api_key_ref)
    };
    match cfg.kind.as_str() {
        "openai" | "ollama" | "custom" => Some(Box::new(OpenAiProvider::new(
            cfg.base_url.clone(),
            api_key,
            cfg.extra_headers.clone(),
            std::time::Duration::from_millis(cfg.timeout_ms),
        ))),
        "anthropic" => Some(Box::new(AnthropicProvider::new(
            cfg.base_url.clone(),
            api_key,
            cfg.extra_headers.clone(),
            std::time::Duration::from_millis(cfg.timeout_ms),
        ))),
        other => {
            tracing::warn!("unknown provider kind: {other}");
            None
        }
    }
}

/// Build an HTTP client tuned for streaming LLM completions.
///
/// A single total `timeout` is wrong for streaming: it aborts legitimate long
/// generations (thinking models may stream for minutes). Instead we use a
/// connect timeout (unreachable servers fail fast) and a per-read timeout that
/// detects stalls between chunks without bounding the total generation time.
/// The response-headers phase is bounded separately by the caller via
/// `tokio::time::timeout` around `send()`.
pub fn http_client(timeout: Duration, headers: HeaderMap) -> reqwest::Client {
    crate::net::apply(
        reqwest::Client::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .default_headers(headers),
    )
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
}

/// Replace reqwest's opaque `error sending request for url (...)` with the real
/// cause (timeout / connection refused / dns / ...) pulled from the source
/// chain, so the user can act on it (start the server, raise timeout_ms, …).
pub fn reqwest_error_detail(e: &reqwest::Error) -> String {
    use std::error::Error as StdError;
    let kind = if e.is_timeout() {
        "Timeout"
    } else if e.is_connect() {
        "Connection failed"
    } else if e.is_decode() {
        "Response decode failed"
    } else if e.is_body() {
        "Body error"
    } else if e.is_redirect() {
        "Redirect error"
    } else if e.is_request() {
        "Request failed"
    } else {
        "Network error"
    };
    let mut cause: Option<String> = None;
    let mut cur = e.source();
    while let Some(s) = cur {
        let s_str = s.to_string();
        if !s_str.is_empty() {
            cause = Some(s_str);
        }
        cur = s.source();
    }
    let url = e.url().map(|u| format!(" [{}]", u));
    match cause {
        Some(c) => format!("{kind}: {c}{}", url.unwrap_or_default()),
        None => format!("{kind}: {e}"),
    }
}

// --- model status probing (Settings → model indicator) ---

/// Live status of a model on its provider endpoint.
/// States: "loaded" (in memory, green), "unknown" (grey), "error" (red),
/// "cloud" (blue; remote endpoint reachable).
#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub state: String,
    pub detail: Value,
}

impl ModelStatus {
    pub fn error(detail: Value) -> Self {
        Self {
            state: "error".into(),
            detail,
        }
    }
}

/// Local vs remote endpoint: loopback/private/link-local hosts and the
/// `localhost` name run on the user's machine — a "loaded in memory" state is
/// meaningful there. Anything else is a cloud endpoint.
fn host_is_local(host: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if h == "localhost" {
        return true;
    }
    match h.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        Ok(IpAddr::V6(v6)) => {
            v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

fn split_tag(s: &str) -> (&str, &str) {
    s.split_once(':').unwrap_or((s, ""))
}

/// Loose model-name match: same base name, missing tags act as wildcard
/// ("llama3" matches "llama3:latest" but not "llama3:8b").
fn tag_matches(entry: &str, model: &str) -> bool {
    let (b1, t1) = split_tag(entry);
    let (b2, t2) = split_tag(model);
    b1 == b2 && (t1.is_empty() || t2.is_empty() || t1 == t2)
}

/// Origin of the native Ollama control API (GET /api/ps, POST /api/show), if
/// `base_url` points at one: local host + classic port 11434 or an /api... path.
pub fn ollama_origin(base_url: &str) -> Option<String> {
    let url = reqwest::Url::parse(base_url.trim_end_matches('/')).ok()?;
    let host = url.host_str()?;
    if !host_is_local(host) {
        return None;
    }
    let port = url.port_or_known_default();
    let api_path = url.path().starts_with("/api");
    if port == Some(11434) || api_path {
        Some(url.origin().ascii_serialization())
    } else {
        None
    }
}

fn insert_if_present(from: &Value, key: &str, to: &mut serde_json::Map<String, Value>) {
    if let Some(v) = from.get(key).filter(|v| !v.is_null()) {
        to.insert(key.into(), v.clone());
    }
}

/// Model info detail block (params, quantization, context window, capabilities).
fn model_info_detail(info: &Value) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    if let Some(d) = info.get("details") {
        for k in ["parameter_size", "quantization_level", "family", "format"] {
            insert_if_present(d, k, &mut out);
        }
    }
    if let Some(caps) = info.get("capabilities").filter(|c| c.is_array()) {
        out.insert("capabilities".into(), caps.clone());
    }
    // Context window lives in model_info under "<architecture>.context_length".
    if let Some(mi) = info.get("model_info").and_then(|m| m.as_object()) {
        if let Some((k, v)) = mi.iter().find(|(k, _)| k.ends_with(".context_length")) {
            if let Some(n) = v.as_u64() {
                out.insert("context_window".into(), Value::from(n));
                out.insert("context_window_source".into(), Value::from(k.clone()));
            }
        }
    }
    insert_if_present(info, "modified_at", &mut out);
    out
}

/// Status via the native Ollama API (/api/ps + /api/show).
/// Returns None when the server doesn't speak that API (or is unreachable).
async fn ollama_status(origin: &str, model: &str) -> Option<ModelStatus> {
    let client = crate::net::apply(reqwest::Client::builder().timeout(Duration::from_secs(4)))
        .build()
        .ok()?;
    let resp = client.get(format!("{origin}/api/ps")).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let running: Value = resp.json().await.ok()?;
    let loaded = running
        .get("models")
        .and_then(|m| m.as_array())
        .and_then(|models| {
            models.iter().find(|e| {
                ["name", "model"].iter().any(|k| {
                    e.get(k)
                        .and_then(Value::as_str)
                        .map(|s| tag_matches(s, model))
                        .unwrap_or(false)
                })
            })
        });
    let entry = match loaded {
        Some(e) => e.clone(),
        None => {
            // Listed on disk but not loaded in memory → grey.
            let show = client
                .post(format!("{origin}/api/show"))
                .json(&serde_json::json!({ "model": model }))
                .send()
                .await
                .ok()?;
            if !show.status().is_success() {
                return Some(ModelStatus::error(serde_json::json!({
                    "error": format!("model '{model}' not found on {origin}"),
                    "http_status": show.status().as_u16(),
                })));
            }
            let info: Value = show.json().await.ok()?;
            let mut detail = model_info_detail(&info);
            detail.insert("loaded".into(), Value::from(false));
            detail.insert("server".into(), Value::from(origin));
            return Some(ModelStatus {
                state: "unknown".into(),
                detail: Value::Object(detail),
            });
        }
    };
    // Loaded in VRAM/RAM → green, with the richest detail available.
    let mut detail = serde_json::Map::new();
    detail.insert("loaded".into(), Value::from(true));
    detail.insert("server".into(), Value::from(origin));
    for k in [
        "digest",
        "size",
        "size_vram",
        "context_length",
        "expires_at",
    ] {
        insert_if_present(&entry, k, &mut detail);
    }
    if let Some(d) = entry.get("details").cloned() {
        for k in ["parameter_size", "quantization_level", "family", "format"] {
            insert_if_present(&d, k, &mut detail);
        }
    }
    Some(ModelStatus {
        state: "loaded".into(),
        detail: Value::Object(detail),
    })
}

/// Probe a reachable server through the provider abstraction. Used for cloud
/// endpoints (state "cloud") and, as a fallback, for local non-Ollama servers.
async fn reachable_status(cfg: &crate::config::Provider, model: &str, local: bool) -> ModelStatus {
    let detail_server = serde_json::json!({ "server": cfg.base_url });
    let Some(provider) = build(cfg) else {
        return ModelStatus::error(serde_json::json!({
            "error": format!("unsupported provider kind: {}", cfg.kind)
        }));
    };
    match timeout(Duration::from_secs(5), provider.list_models()).await {
        Err(_) => ModelStatus::error(serde_json::json!({
            "error": "request timed out",
            "server": cfg.base_url
        })),
        Ok(Err(e)) => ModelStatus::error(serde_json::json!({
            "error": e.detail(),
            "server": cfg.base_url
        })),
        Ok(Ok(models)) => {
            let model_listed = models
                .iter()
                .any(|m| m.id == model || (!m.name.is_empty() && m.name == model));
            let mut detail = serde_json::Map::new();
            if let Value::Object(o) = detail_server {
                detail.extend(o);
            }
            detail.insert("models".into(), Value::from(models.len()));
            detail.insert("model_listed".into(), Value::from(model_listed));
            // Local OpenAI-compatible servers don't expose in-memory state via
            // their APIs, and "reachable" is the strongest available signal —
            // report it like a loaded model (green). Cloud endpoints keep a
            // distinct "cloud" state (blue).
            ModelStatus {
                state: if local { "loaded" } else { "cloud" }.into(),
                detail: Value::Object(detail),
            }
        }
    }
}

/// Resolve the live status for the chat's model on its provider endpoint.
pub async fn model_status(cfg: &crate::config::Provider, model: &str) -> ModelStatus {
    if model.trim().is_empty() {
        return ModelStatus {
            state: "unknown".into(),
            detail: serde_json::json!({ "reason": "no model selected" }),
        };
    }
    if let Ok(url) = reqwest::Url::parse(cfg.base_url.trim_end_matches('/')) {
        if url.host_str().map(host_is_local) == Some(false) {
            return reachable_status(cfg, model, false).await;
        }
    }
    if let Some(origin) = ollama_origin(&cfg.base_url) {
        if let Some(status) = ollama_status(&origin, model).await {
            return status;
        }
    }
    reachable_status(cfg, model, true).await
}
