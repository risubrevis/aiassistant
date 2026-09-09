use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::Value;
use tokio::time::timeout;

use super::{
    BlockType, ChatMessage, CompleteEvent, CompleteRequest, ContentPart, ModelInfo, Provider,
    ProviderError, ToolUseInfo, Usage,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic has no default for `max_tokens`; bump when the request omits it.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Native Anthropic Messages API adapter (`POST /v1/messages`, SSE streaming).
pub struct AnthropicProvider {
    base_url: String,
    client: reqwest::Client,
    timeout: Duration,
}

impl AnthropicProvider {
    pub fn new(
        base_url: String,
        api_key: Option<String>,
        extra_headers: HashMap<String, String>,
        timeout: Duration,
    ) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );
        if let Some(ref key) = api_key {
            if !key.is_empty() {
                if let Ok(v) = HeaderValue::from_str(key) {
                    headers.insert("x-api-key", v);
                }
            }
        }
        for (k, v) in &extra_headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, val);
            }
        }
        let client = crate::providers::http_client(timeout, headers);
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            timeout,
        }
    }

    fn messages_url(&self) -> String {
        if has_v1_path(&self.base_url) {
            format!("{}/messages", self.base_url)
        } else {
            format!("{}/v1/messages", self.base_url)
        }
    }

    async fn map_error(&self, resp: reqwest::Response) -> ProviderError {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        crate::providers::classify_anthropic_error(status, &body)
    }
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: &'static str,
    budget_tokens: u32,
}

#[derive(Serialize)]
struct MessagesBody {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    stream: bool,
}

#[derive(Debug, Serialize, PartialEq)]
struct ApiMessage {
    role: &'static str,
    content: Vec<ApiBlock>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ApiBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Serialize, PartialEq)]
struct ImageSource {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize, PartialEq)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: Value,
}

/// OpenAI-style roles -> Anthropic messages. `system` is carried on the
/// request, not in `messages`; `tool` results become user messages holding
/// `tool_result` blocks (consecutive ones are merged by the Messages API).
fn convert_messages(messages: &[ChatMessage]) -> Vec<ApiMessage> {
    let mut out = Vec::new();
    for m in messages {
        match m.role.as_str() {
            "system" => {}
            "tool" => out.push(ApiMessage {
                role: "user",
                content: vec![ApiBlock::ToolResult {
                    tool_use_id: m.tool_call_id.clone().unwrap_or_default(),
                    content: m.content.clone(),
                }],
            }),
            role => {
                let mut content = Vec::new();
                match m.parts.as_ref().filter(|p| !p.is_empty()) {
                    Some(parts) => {
                        for p in parts {
                            match p {
                                ContentPart::Text { text } => {
                                    content.push(ApiBlock::Text { text: text.clone() })
                                }
                                ContentPart::Image { media_type, data } => {
                                    content.push(ApiBlock::Image {
                                        source: ImageSource {
                                            kind: "base64",
                                            media_type: media_type.clone(),
                                            data: data.clone(),
                                        },
                                    });
                                }
                            }
                        }
                    }
                    None => {
                        if !m.content.is_empty() {
                            content.push(ApiBlock::Text {
                                text: m.content.clone(),
                            });
                        }
                    }
                }
                for tc in m.tool_calls.iter().flatten() {
                    content.push(ApiBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        // Arguments are reconstructed from persisted blocks as a
                        // JSON string; fall back to an empty object if malformed.
                        input: serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(Value::Object(serde_json::Map::new())),
                    });
                }
                if content.is_empty() {
                    continue;
                }
                out.push(ApiMessage {
                    role: if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    },
                    content,
                });
            }
        }
    }
    out
}

fn convert_tools(tools: Option<&Value>) -> Option<Vec<ApiTool>> {
    let arr = tools?.as_array()?;
    let converted: Vec<ApiTool> = arr
        .iter()
        .filter(|t| t.get("type").and_then(|v| v.as_str()).unwrap_or("function") == "function")
        .filter_map(|t| {
            let f = t.get("function")?;
            Some(ApiTool {
                name: f.get("name")?.as_str()?.to_string(),
                description: f
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: f
                    .get("parameters")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new())),
            })
        })
        .collect();
    (!converted.is_empty()).then_some(converted)
}

fn block_type_from(s: &str) -> Option<BlockType> {
    match s {
        "text" => Some(BlockType::Text),
        "thinking" => Some(BlockType::Thinking),
        "tool_use" => Some(BlockType::ToolUse),
        _ => None,
    }
}

fn finish_reason_from(stop_reason: &str) -> String {
    match stop_reason {
        "end_turn" | "stop_sequence" => "stop".into(),
        "tool_use" => "tool_calls".into(),
        "max_tokens" => "length".into(),
        other => other.to_string(),
    }
}

fn blk_id(index: u64) -> String {
    format!("blk-{index}")
}

/// Whether the URL path (not host) already contains a `v1` segment.
fn has_v1_path(base: &str) -> bool {
    let Some(scheme_end) = base.find("://") else {
        return false;
    };
    let after_scheme = &base[scheme_end + "://".len()..];
    let Some(path_start) = after_scheme.find('/') else {
        return false;
    };
    after_scheme[path_start..].split('/').any(|seg| seg == "v1")
}

/// Static list (Anthropic has no public list-models endpoint).
fn known_models() -> Vec<ModelInfo> {
    [
        ("claude-opus-4", "Claude Opus 4"),
        ("claude-sonnet-4-5", "Claude Sonnet 4.5"),
        ("claude-sonnet-4", "Claude Sonnet 4"),
        ("claude-haiku-4", "Claude Haiku 4"),
        ("claude-3-7-sonnet", "Claude 3.7 Sonnet"),
        ("claude-3-5-haiku", "Claude 3.5 Haiku"),
    ]
    .into_iter()
    .map(|(id, name)| ModelInfo {
        id: id.to_string(),
        name: name.to_string(),
        context_window: Some(200_000),
    })
    .collect()
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(known_models())
    }

    async fn stream_complete(
        &self,
        req: CompleteRequest,
        sink: tokio::sync::mpsc::Sender<CompleteEvent>,
    ) -> Result<Usage, ProviderError> {
        // Extended thinking requires temperature to be unset and max_tokens
        // comfortably above the thinking budget.
        let (thinking, temperature, max_tokens) = if req.thinking == Some(true) {
            let budget_tokens = match req.thinking_effort.as_deref() {
                Some("low") => 2048,
                Some("high") => 8192,
                _ => 4096, // medium or default
            };
            (
                Some(ThinkingConfig {
                    kind: "enabled",
                    budget_tokens,
                }),
                None,
                req.max_tokens
                    .unwrap_or(DEFAULT_MAX_TOKENS)
                    .max(budget_tokens + 1024),
            )
        } else {
            (
                None,
                req.temperature,
                req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            )
        };
        let body = MessagesBody {
            model: req.model.clone(),
            max_tokens,
            system: req
                .system
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            messages: convert_messages(&req.messages),
            tools: convert_tools(req.tools.as_ref()),
            temperature,
            thinking,
            stream: true,
        };

        let started = std::time::Instant::now();
        let resp = timeout(
            self.timeout,
            self.client.post(self.messages_url()).json(&body).send(),
        )
        .await
        .map_err(|_| {
            ProviderError::Network(format!(
                "Timeout: no response within {} ms (the model may still be loading or the server is busy). Consider raising timeout_ms.",
                self.timeout.as_millis()
            ))
        })?
        .map_err(|e| ProviderError::Network(crate::providers::reqwest_error_detail(&e)))?;
        if !resp.status().is_success() {
            return Err(self.map_error(resp).await);
        }

        // Dispatch on the payload's `type` field (canonical per the Messages
        // API docs); the SSE `event:` name carries the same value.
        let mut stream = resp.bytes_stream().eventsource();
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut stop_reason: Option<String> = None;
        let mut first_token_at: Option<std::time::Instant> = None;

        while let Some(event) = stream.next().await {
            let event = event.map_err(|e| ProviderError::Network(e.to_string()))?;
            let data: Value = match serde_json::from_str(&event.data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match data.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "message_start" => {
                    if let Some(t) = data
                        .pointer("/message/usage/input_tokens")
                        .and_then(Value::as_u64)
                    {
                        input_tokens = t;
                    }
                    if let Some(t) = data
                        .pointer("/message/usage/output_tokens")
                        .and_then(Value::as_u64)
                    {
                        output_tokens = t;
                    }
                }
                "content_block_start" => {
                    let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                    let Some(block) = data.get("content_block") else {
                        continue;
                    };
                    let Some(block_type) =
                        block_type_from(block.get("type").and_then(Value::as_str).unwrap_or(""))
                    else {
                        continue;
                    };
                    let info = if block_type == BlockType::ToolUse {
                        block
                            .get("name")
                            .and_then(Value::as_str)
                            .zip(block.get("id").and_then(Value::as_str))
                            .map(|(name, id)| ToolUseInfo {
                                name: name.to_string(),
                                tool_call_id: id.to_string(),
                            })
                    } else {
                        None
                    };
                    let _ = sink
                        .send(CompleteEvent::BlockStart {
                            block_id: blk_id(index),
                            block_type,
                            info,
                        })
                        .await;
                }
                "content_block_delta" => {
                    let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                    let Some(delta) = data.get("delta") else {
                        continue;
                    };
                    let (text, partial_json) =
                        match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                            "text_delta" => (
                                delta
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                None,
                            ),
                            "thinking_delta" => (
                                delta
                                    .get("thinking")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                None,
                            ),
                            "input_json_delta" => (
                                None,
                                delta
                                    .get("partial_json")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                            ),
                            _ => continue,
                        };
                    if first_token_at.is_none() && text.as_deref().is_some_and(|t| !t.is_empty()) {
                        first_token_at = Some(std::time::Instant::now());
                    }
                    let _ = sink
                        .send(CompleteEvent::BlockDelta {
                            block_id: blk_id(index),
                            text,
                            partial_json,
                        })
                        .await;
                }
                "content_block_stop" => {
                    let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
                    let _ = sink
                        .send(CompleteEvent::BlockStop {
                            block_id: blk_id(index),
                        })
                        .await;
                }
                "message_delta" => {
                    if let Some(sr) = data
                        .pointer("/delta/stop_reason")
                        .and_then(Value::as_str)
                        .filter(|sr| !sr.is_empty())
                    {
                        stop_reason = Some(sr.to_string());
                    }
                    if let Some(t) = data.pointer("/usage/input_tokens").and_then(Value::as_u64) {
                        input_tokens = t;
                    }
                    if let Some(t) = data.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                        output_tokens = t;
                    }
                }
                "message_stop" => break,
                "error" => {
                    let msg = data
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| event.data.clone());
                    return Err(ProviderError::Parse(format!(
                        "anthropic stream error: {msg}"
                    )));
                }
                _ => {}
            }
        }

        let total_duration_ms = started.elapsed().as_millis() as u64;
        let (ttft_ms, gen_ms) = first_token_at.map_or((None, None), |t| {
            (
                Some(t.duration_since(started).as_millis() as u64),
                Some(t.elapsed().as_millis() as u64),
            )
        });
        let usage = Usage {
            prompt_tokens: input_tokens,
            completion_tokens: output_tokens,
            total_tokens: input_tokens + output_tokens,
            total_duration_ms: Some(total_duration_ms),
            time_to_first_token_ms: ttft_ms,
            generation_duration_ms: gen_ms,
        };
        let _ = sink
            .send(CompleteEvent::Done {
                finish_reason: stop_reason
                    .as_deref()
                    .map_or_else(|| "stop".into(), finish_reason_from),
                usage: Some(usage.clone()),
            })
            .await;
        Ok(usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ToolCall, ToolCallFunction};
    use std::time::Duration;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
            parts: None,
        }
    }

    #[test]
    fn converts_user_assistant_and_tool_flow() {
        let messages = vec![
            msg("user", "List files"),
            ChatMessage {
                role: "assistant".into(),
                content: "".into(),
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "toolu_1".into(),
                    kind: "function".into(),
                    function: ToolCallFunction {
                        name: "read_file".into(),
                        arguments: r#"{"path":"Cargo.toml"}"#.into(),
                    },
                }]),
                parts: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: "file bytes".into(),
                tool_call_id: Some("toolu_1".into()),
                tool_calls: None,
                parts: None,
            },
        ];

        let out = convert_messages(&messages);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "user");
        assert_eq!(
            out[0].content[0],
            ApiBlock::Text {
                text: "List files".into()
            }
        );
        assert_eq!(out[1].role, "assistant");
        assert_eq!(
            out[1].content[0],
            ApiBlock::ToolUse {
                id: "toolu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({"path": "Cargo.toml"}),
            }
        );
        assert_eq!(out[2].role, "user");
        assert_eq!(
            out[2].content[0],
            ApiBlock::ToolResult {
                tool_use_id: "toolu_1".into(),
                content: "file bytes".into(),
            }
        );
        let ser = serde_json::to_value(&out[2].content[0]).unwrap();
        assert_eq!(ser["type"], "tool_result");
        assert_eq!(ser["tool_use_id"], "toolu_1");
    }

    #[test]
    fn converts_skips_system_and_empty_messages() {
        let messages = vec![
            msg("system", "should be dropped"),
            msg("user", ""),
            msg("user", "hello"),
        ];
        let out = convert_messages(&messages);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "user");
        assert_eq!(
            out[0].content,
            vec![ApiBlock::Text {
                text: "hello".into()
            }]
        );
    }

    #[test]
    fn converts_openai_tools() {
        let tools = serde_json::json!([
            {
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Reads a file",
                    "parameters": {
                        "type": "object",
                        "properties": {"path": {"type": "string"}}
                    }
                }
            },
            {"type": "unsupported", "function": {"name": "x", "parameters": {}}}
        ]);
        let api_tools = convert_tools(Some(&tools)).expect("tools");
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0].name, "read_file");
        assert_eq!(api_tools[0].description, "Reads a file");
        assert_eq!(
            api_tools[0].input_schema["properties"]["path"]["type"],
            "string"
        );
        let ser = serde_json::to_value(&api_tools[0]).unwrap();
        assert_eq!(ser["input_schema"]["type"], "object");
        assert_eq!(convert_tools(None), None);
    }

    #[test]
    fn maps_stop_reasons_and_urls() {
        assert_eq!(finish_reason_from("end_turn"), "stop");
        assert_eq!(finish_reason_from("stop_sequence"), "stop");
        assert_eq!(finish_reason_from("tool_use"), "tool_calls");
        assert_eq!(finish_reason_from("max_tokens"), "length");
        assert_eq!(finish_reason_from("pause_turn"), "pause_turn");

        assert!(has_v1_path("https://api.anthropic.com/v1"));
        assert!(has_v1_path("http://localhost:8080/gw/v1"));
        assert!(!has_v1_path("https://api.anthropic.com"));
        assert!(!has_v1_path("https://v1.example.com"));

        let p = AnthropicProvider::new(
            "https://api.anthropic.com/v1/".into(),
            None,
            HashMap::new(),
            Duration::from_secs(30),
        );
        assert_eq!(p.messages_url(), "https://api.anthropic.com/v1/messages");
        let p = AnthropicProvider::new(
            "https://api.anthropic.com".into(),
            None,
            HashMap::new(),
            Duration::from_secs(30),
        );
        assert_eq!(p.messages_url(), "https://api.anthropic.com/v1/messages");
    }
}
