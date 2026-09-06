use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use super::{
    BlockType, ChatMessage, CompleteEvent, CompleteRequest, ContentPart, ModelInfo, Provider,
    ProviderError, ToolUseInfo, Usage,
};

/// OpenAI-compatible Chat Completions adapter. Covers OpenAI, Ollama (`/v1`),
/// llama.cpp, LM Studio, vLLM, LocalAI, OpenRouter, Groq, etc. (docs/05).
pub struct OpenAiProvider {
    base_url: String,
    client: reqwest::Client,
    timeout: Duration,
}

impl OpenAiProvider {
    pub fn new(
        base_url: String,
        api_key: Option<String>,
        extra_headers: HashMap<String, String>,
        timeout: Duration,
    ) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        if let Some(ref key) = api_key {
            if !key.is_empty() {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {key}")) {
                    headers.insert(AUTHORIZATION, v);
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

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

#[derive(Serialize)]
struct ChatCompletionsBody<'a> {
    model: &'a str,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Option<Vec<ModelEntry>>,
}
#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

impl OpenAiProvider {
    async fn map_error(&self, resp: reqwest::Response) -> ProviderError {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        crate::providers::classify_openai_error(status, &body)
    }
}

/// Build an OpenAI Chat Completions message. Multi-part content (text +
/// images) becomes an array of typed parts; plain messages stay string-typed.
fn message_to_json(m: &ChatMessage) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("role".into(), serde_json::Value::String(m.role.clone()));
    match m.parts.as_deref().filter(|p| !p.is_empty()) {
        Some(parts) => {
            let arr = parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => {
                        serde_json::json!({ "type": "text", "text": text })
                    }
                    ContentPart::Image { media_type, data } => serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{media_type};base64,{data}") },
                    }),
                })
                .collect();
            obj.insert("content".into(), serde_json::Value::Array(arr));
        }
        None => {
            obj.insert(
                "content".into(),
                serde_json::Value::String(m.content.clone()),
            );
        }
    }
    if let Some(id) = &m.tool_call_id {
        obj.insert("tool_call_id".into(), serde_json::Value::String(id.clone()));
    }
    if let Some(tcs) = &m.tool_calls {
        if let Ok(v) = serde_json::to_value(tcs) {
            obj.insert("tool_calls".into(), v);
        }
    }
    serde_json::Value::Object(obj)
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = timeout(self.timeout, self.client.get(self.url("/models")).send())
            .await
            .map_err(|_| {
                ProviderError::Network(format!(
                    "Timeout: no response within {} ms",
                    self.timeout.as_millis()
                ))
            })?
            .map_err(|e| ProviderError::Network(crate::providers::reqwest_error_detail(&e)))?;
        if !resp.status().is_success() {
            return Err(self.map_error(resp).await);
        }
        let parsed: ModelsResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let models = parsed
            .data
            .unwrap_or_default()
            .into_iter()
            .map(|m| ModelInfo {
                name: m.id.clone(),
                id: m.id,
                context_window: None,
            })
            .collect();
        Ok(models)
    }

    async fn stream_complete(
        &self,
        req: CompleteRequest,
        sink: tokio::sync::mpsc::Sender<CompleteEvent>,
    ) -> Result<Usage, ProviderError> {
        let mut messages: Vec<serde_json::Value> = Vec::with_capacity(req.messages.len() + 1);
        if let Some(sys) = req.system.as_ref().filter(|s| !s.is_empty()) {
            messages.push(message_to_json(&ChatMessage {
                role: "system".into(),
                content: sys.clone(),
                tool_call_id: None,
                tool_calls: None,
                parts: None,
            }));
        }
        messages.extend(req.messages.iter().map(message_to_json));

        let body = ChatCompletionsBody {
            model: &req.model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            tools: req.tools.as_ref(),
            tool_choice: req.tools.as_ref().map(|_| "auto"),
        };

        let started = std::time::Instant::now();
        let resp = timeout(
            self.timeout,
            self.client.post(self.url("/chat/completions")).json(&body).send(),
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

        let mut stream = resp.bytes_stream().eventsource();

        let mut current_block: Option<(String, BlockType)> = None;
        let mut thinking_started: Option<std::time::Instant> = None;
        let mut thinking_ms: u128 = 0;
        let mut first_token_at: Option<std::time::Instant> = None;
        let mut finish_reason = "stop".to_string();
        let mut usage: Option<Usage> = None;

        // Per-index tool-call accumulation.
        #[derive(Default)]
        struct ToolAccum {
            block_id: String,
            name: String,
            id: String,
            started: bool,
        }
        let mut tool_blocks: HashMap<u32, ToolAccum> = HashMap::new();

        while let Some(event) = stream.next().await {
            // Transport/decode errors must not discard already streamed content;
            // finalize the partial turn gracefully.
            let event = match event {
                Ok(ev) => ev,
                Err(e) => {
                    tracing::warn!("stream event error: {e}; finalizing partial turn");
                    break;
                }
            };
            if event.data == "[DONE]" {
                break;
            }
            if event.data.is_empty() {
                continue;
            }
            // Malformed chunks from non-conforming local models are skipped.
            let chunk: serde_json::Value = match serde_json::from_str(&event.data) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        "skipping malformed stream chunk: {e}; data={}",
                        event.data.chars().take(200).collect::<String>()
                    );
                    continue;
                }
            };

            if let Some(u) = chunk.get("usage") {
                usage = Some(Usage {
                    prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                    completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
                    total_tokens: u["total_tokens"].as_u64().unwrap_or(0),
                    ..Default::default()
                });
            }

            let choice = match chunk.get("choices").and_then(|c| c.get(0)) {
                Some(c) => c,
                None => continue,
            };

            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                if !fr.is_empty() {
                    finish_reason = fr.to_string();
                }
            }

            let delta = match choice.get("delta") {
                Some(d) => d,
                None => continue,
            };

            // Reasoning/thinking (DeepSeek/Qwen/GLM `reasoning_content`, OpenAI `reasoning`).
            if let Some(reasoning) = delta
                .get("reasoning_content")
                .and_then(|v| v.as_str())
                .or_else(|| delta.get("reasoning").and_then(|v| v.as_str()))
            {
                if !reasoning.is_empty() {
                    if first_token_at.is_none() {
                        first_token_at = Some(std::time::Instant::now());
                    }
                    if !matches!(&current_block, Some((_, t)) if *t == BlockType::Thinking) {
                        close_block(
                            &sink,
                            &current_block,
                            &mut thinking_ms,
                            &mut thinking_started,
                        )
                        .await;
                        let id = "block-thinking".to_string();
                        let _ = sink
                            .send(CompleteEvent::BlockStart {
                                block_id: id.clone(),
                                block_type: BlockType::Thinking,
                                info: None,
                            })
                            .await;
                        thinking_started = Some(std::time::Instant::now());
                        current_block = Some((id, BlockType::Thinking));
                    }
                    let _ = sink
                        .send(CompleteEvent::BlockDelta {
                            block_id: current_block.as_ref().unwrap().0.clone(),
                            text: Some(reasoning.to_string()),
                            partial_json: None,
                        })
                        .await;
                }
            }

            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    if first_token_at.is_none() {
                        first_token_at = Some(std::time::Instant::now());
                    }
                    if !matches!(&current_block, Some((_, t)) if *t == BlockType::Text) {
                        close_block(
                            &sink,
                            &current_block,
                            &mut thinking_ms,
                            &mut thinking_started,
                        )
                        .await;
                        let id = "block-text".to_string();
                        let _ = sink
                            .send(CompleteEvent::BlockStart {
                                block_id: id.clone(),
                                block_type: BlockType::Text,
                                info: None,
                            })
                            .await;
                        current_block = Some((id, BlockType::Text));
                    }
                    let _ = sink
                        .send(CompleteEvent::BlockDelta {
                            block_id: current_block.as_ref().unwrap().0.clone(),
                            text: Some(content.to_string()),
                            partial_json: None,
                        })
                        .await;
                }
            }

            // Tool calls (OpenAI streaming: each chunk carries tool_calls[index]
            // with optional id/function.name and function.arguments fragments).
            if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let entry = tool_blocks.entry(idx).or_insert_with(|| ToolAccum {
                        block_id: format!("tool-{idx}"),
                        ..Default::default()
                    });
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        entry.id = id.to_string();
                        if entry.block_id.starts_with("tool-") {
                            entry.block_id = id.to_string();
                        }
                    }
                    if let Some(name) = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                    {
                        entry.name = name.to_string();
                    }
                    if !entry.started && (!entry.name.is_empty() || !entry.id.is_empty()) {
                        close_block(
                            &sink,
                            &current_block,
                            &mut thinking_ms,
                            &mut thinking_started,
                        )
                        .await;
                        let _ = sink
                            .send(CompleteEvent::BlockStart {
                                block_id: entry.block_id.clone(),
                                block_type: BlockType::ToolUse,
                                info: Some(ToolUseInfo {
                                    name: entry.name.clone(),
                                    tool_call_id: entry.id.clone(),
                                }),
                            })
                            .await;
                        current_block = Some((entry.block_id.clone(), BlockType::ToolUse));
                        entry.started = true;
                    }
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                    {
                        if !args.is_empty() {
                            let _ = sink
                                .send(CompleteEvent::BlockDelta {
                                    block_id: entry.block_id.clone(),
                                    text: None,
                                    partial_json: Some(args.to_string()),
                                })
                                .await;
                        }
                    }
                }
            }
        }

        close_block(
            &sink,
            &current_block,
            &mut thinking_ms,
            &mut thinking_started,
        )
        .await;

        let _ = thinking_ms;
        let total_duration_ms = started.elapsed().as_millis() as u64;
        let (ttft_ms, gen_ms) = first_token_at.map_or((None, None), |t| {
            (
                Some(t.duration_since(started).as_millis() as u64),
                Some(t.elapsed().as_millis() as u64),
            )
        });
        let mut usage = usage.unwrap_or_default();
        usage.total_duration_ms = Some(total_duration_ms);
        usage.time_to_first_token_ms = ttft_ms;
        usage.generation_duration_ms = gen_ms;

        let _ = sink
            .send(CompleteEvent::Done {
                finish_reason: finish_reason.clone(),
                usage: Some(usage.clone()),
            })
            .await;

        Ok(usage)
    }
}

async fn close_block(
    sink: &tokio::sync::mpsc::Sender<CompleteEvent>,
    current: &Option<(String, BlockType)>,
    thinking_ms: &mut u128,
    thinking_started: &mut Option<std::time::Instant>,
) {
    if let Some((id, ty)) = current {
        if *ty == BlockType::Thinking {
            if let Some(start) = thinking_started.take() {
                *thinking_ms += start.elapsed().as_millis();
            }
        }
        let _ = sink
            .send(CompleteEvent::BlockStop {
                block_id: id.clone(),
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "requires a local Ollama at localhost:11434"]
    async fn ollama_stream_smoke() {
        let provider = OpenAiProvider::new(
            "http://localhost:11434/v1".into(),
            None,
            HashMap::new(),
            Duration::from_secs(60),
        );
        let models = provider.list_models().await.expect("list_models");
        assert!(!models.is_empty(), "ollama returned no models");

        let (tx, mut rx) = tokio::sync::mpsc::channel(128);
        let req = CompleteRequest {
            model: "llama3.2:3b".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "Reply with the single word: pong".into(),
                tool_call_id: None,
                tool_calls: None,
                parts: None,
            }],
            system: None,
            temperature: None,
            max_tokens: Some(32),
            tools: None,
        };
        let h = tokio::spawn(async move { provider.stream_complete(req, tx).await });

        let mut got_text = false;
        while let Some(ev) = rx.recv().await {
            println!("event: {ev:?}");
            if let CompleteEvent::BlockDelta { text: Some(t), .. } = &ev {
                if !t.is_empty() {
                    got_text = true;
                }
            }
        }
        h.await.unwrap().expect("stream_complete");
        assert!(got_text, "no text delta received");
    }
}
