#![allow(clippy::too_many_arguments)]
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use base64::Engine;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::agents::{self, AgentRunner};
use crate::approval::{self, ApprovalRegistry};
use crate::ask::{self, AskRegistry};
use crate::config::Config;
use crate::db::attachments;
use crate::db::models::{self, Chat, Message, ToolCallRow};
use crate::db::tasks::{self, TaskInput};
use crate::pending::PendingManager;
use crate::permission::{self, Decision};
use crate::projects;
use crate::providers::{
    self, BlockType, ChatMessage, CompleteEvent, CompleteRequest, ContentPart, Provider, ToolCall,
    ToolCallFunction, ToolUseInfo,
};
use crate::pty::PtyManager;
use crate::rules;
use crate::tools::Registry as ToolRegistry;

/// Map of chat_id -> running turn task handle, for cancellation (docs/18).
pub type ActiveTurns = Arc<Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>>;

/// Turns since the last todo_write per chat; drives the stale-open-task nudge.
/// In-memory only, resets on app restart.
#[derive(Clone, Default)]
pub struct TaskNudge {
    counters: Arc<Mutex<HashMap<String, u32>>>,
}

impl TaskNudge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a turn without todo_write; returns the running counter.
    pub fn bump(&self, chat_id: &str) -> u32 {
        let mut map = self.counters.lock().unwrap();
        let counter = map.entry(chat_id.to_string()).or_insert(0);
        *counter += 1;
        *counter
    }

    pub fn reset(&self, chat_id: &str) {
        self.counters.lock().unwrap().remove(chat_id);
    }
}

/// Turns without a todo_write before the stale-open-task reminder fires.
const TASK_NUDGE_TURNS: u32 = 3;

/// A task the model is still expected to act on.
fn is_open_task(t: &tasks::Task) -> bool {
    t.status == "pending" || t.status == "in_progress"
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatStatus {
    Idle,
    Running,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
struct StatusPayload {
    chat_id: String,
    status: ChatStatus,
    detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectTaskChangedPayload {
    project_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct BlockStartPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    block_type: BlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    info: Option<ToolUseInfo>,
}

#[derive(Debug, Clone, Serialize)]
struct BlockDeltaPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partial_json: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BlockStopPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct ToolResultPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    result: String,
    is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PendingUpdatePayload {
    chat_id: String,
    changes: Vec<crate::pending::ChangeInfo>,
}

#[derive(Debug, Clone, Serialize)]
struct PtyStartPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    session_id: String,
    command: String,
}

#[derive(Debug, Clone, Serialize)]
struct PtyOutputPayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    session_id: String,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct PtyDonePayload {
    chat_id: String,
    message_id: String,
    block_id: String,
    session_id: String,
    code: i32,
}

#[derive(Debug, Clone, Serialize)]
struct MessageDonePayload {
    chat_id: String,
    message_id: String,
    usage: Option<crate::providers::Usage>,
    finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct TurnErrorPayload {
    chat_id: String,
    message_id: String,
    kind: String,
    message: String,
    retryable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TasksUpdatePayload {
    pub chat_id: String,
    pub tasks: Vec<tasks::Task>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn emit_status(app: &AppHandle, chat_id: &str, status: ChatStatus, detail: Option<String>) {
    let _ = app.emit(
        "chat:status",
        StatusPayload {
            chat_id: chat_id.into(),
            status,
            detail,
        },
    );
}

/// Resolve the provider config + model for a chat (with fallbacks). Project
/// defaults override global defaults; explicit chat selection wins (docs/08).
/// `provider_id`/`model_id` are UUIDs into the DB (`providers`/`provider_models`).
/// Returns `(provider_cfg, model_api_name, model_uuid)`; `None` when either the
/// provider or the model can't be resolved.
pub async fn resolve_provider(
    pool: &SqlitePool,
    chat: &Chat,
    project: Option<&crate::db::models::Project>,
    config: &Config,
) -> Option<(crate::config::Provider, String, String)> {
    let provider_uuid = chat
        .provider_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(project
            .and_then(|p| p.default_provider_id.as_deref())
            .filter(|s| !s.is_empty()))
        .or(config
            .defaults
            .main_model
            .as_ref()
            .map(|m| m.provider.as_str())
            .filter(|s| !s.is_empty()))?;
    let model_uuid = chat
        .model_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(project
            .and_then(|p| p.default_model_id.as_deref())
            .filter(|s| !s.is_empty()))
        .or(config
            .defaults
            .main_model
            .as_ref()
            .map(|m| m.model.as_str())
            .filter(|s| !s.is_empty()))?;
    let prov_row = crate::db::providers::get_provider(pool, provider_uuid)
        .await
        .ok()
        .flatten()?;
    let model_row = crate::db::providers::get_model(pool, model_uuid)
        .await
        .ok()
        .flatten()?;
    Some((prov_row.to_config_provider(), model_row.name, model_row.id))
}

/// Parse persisted content_parts into block JSON values.
fn parse_blocks(content_parts: &Option<String>) -> Vec<serde_json::Value> {
    match content_parts {
        Some(s) if !s.is_empty() => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                if let Some(arr) = v.get("blocks").and_then(|b| b.as_array()) {
                    return arr.clone();
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Multipart content built from the chat's persisted attachments, grouped by
/// the user message each attachment is linked to. Images become base64 image
/// parts; text-like files are inlined into a text part; other binaries get a
/// short annotation so the model at least knows the file exists.
async fn build_attachment_parts(
    pool: &SqlitePool,
    chat_id: &str,
) -> anyhow::Result<HashMap<String, Vec<ContentPart>>> {
    const MAX_TEXT_ATTACHMENT_BYTES: i64 = 262_144;
    let list = attachments::list_for_chat(pool, chat_id).await?;
    let mut map: HashMap<String, Vec<ContentPart>> = HashMap::new();
    for att in list {
        let Some(message_id) = att.message_id else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&att.storage_path) else {
            warn!("attachment file missing: {}", att.storage_path);
            continue;
        };
        let slot = map.entry(message_id).or_default();
        if att.is_image {
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            slot.push(ContentPart::Image {
                media_type: att.mime_type.clone(),
                data,
            });
        } else if is_text_mime(&att.mime_type) && att.file_size <= MAX_TEXT_ATTACHMENT_BYTES {
            let text = String::from_utf8_lossy(&bytes);
            slot.push(ContentPart::Text {
                text: format!(
                    "--- Attached file: {} ---\n{}\n--- end {} ---",
                    att.file_name, text, att.file_name
                ),
            });
        } else {
            slot.push(ContentPart::Text {
                text: format!(
                    "[Attached file: {} ({}), {} bytes]",
                    att.file_name, att.mime_type, att.file_size
                ),
            });
        }
    }
    Ok(map)
}

fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/x-yaml"
                | "application/toml"
        )
}

/// Reconstruct OpenAI-style messages from the active branch history.
fn build_history(
    messages: &[Message],
    parts: &HashMap<String, Vec<ContentPart>>,
) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    for m in messages {
        match m.role.as_str() {
            "user" => {
                let user_parts = parts.get(&m.id).cloned().filter(|p| !p.is_empty());
                out.push(ChatMessage {
                    role: "user".into(),
                    content: m.content.clone(),
                    tool_call_id: None,
                    tool_calls: None,
                    parts: user_parts,
                });
            }
            "assistant" => {
                let blocks = parse_blocks(&m.content_parts);
                let tool_calls: Vec<ToolCall> = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
                    .filter_map(|b| {
                        Some(ToolCall {
                            id: b.get("tool_call_id")?.as_str()?.to_string(),
                            kind: "function".into(),
                            function: ToolCallFunction {
                                name: b.get("name")?.as_str()?.to_string(),
                                arguments: b
                                    .get("input")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                        })
                    })
                    .collect();
                let text: String = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
                out.push(ChatMessage {
                    role: "assistant".into(),
                    content: if text.is_empty() {
                        m.content.clone()
                    } else {
                        text
                    },
                    tool_call_id: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    parts: None,
                });
            }
            "tool" => {
                let blocks = parse_blocks(&m.content_parts);
                let tool_call_id = blocks
                    .iter()
                    .find_map(|b| b.get("tool_call_id").and_then(|v| v.as_str()))
                    .unwrap_or_default()
                    .to_string();
                out.push(ChatMessage {
                    role: "tool".into(),
                    content: m.content.clone(),
                    tool_call_id: if tool_call_id.is_empty() {
                        None
                    } else {
                        Some(tool_call_id)
                    },
                    tool_calls: None,
                    parts: None,
                });
            }
            _ => {}
        }
    }
    out
}

/// System prompt for the summarization call (provider-agnostic, structured).
const SUMMARY_PROMPT: &str = r#"You are a conversation summarizer. Summarize the conversation so another assistant can continue the work without losing context.

Rules:
- Preserve exact file paths, identifiers, commands, error messages, URLs, and user decisions.
- Do not include code blocks; reference files by path only.
- Be concise: use terse bullets, not prose.
- Keep every section below; if a section is empty, write "(none)".
- Do not mention that this is a summary or that context was compacted.

Output exactly this Markdown structure:

## Objective
- (what the user is trying to accomplish)

## Key decisions and context
- (constraints, preferences, decisions and rationale, important facts)

## Work completed
- (finished work, verified facts, changes made)

## Current state
- (current work, partial changes, open questions)

## Next step
- (immediate concrete action)

## Relevant files
- (file or directory path: why it matters)"#;

const TITLE_PROMPT: &str = "You generate a short, descriptive title for a chat conversation. \
Rules: 2-6 words, plain text, no quotes, no trailing punctuation, no prefix like \"Title:\". \
Respond with ONLY the title.";

/// Rough token estimate (chars/4). Used only for sizing the tail and recording
/// an approximate token_count — the trigger itself uses real API usage.
fn estimate_tokens(text: &str) -> i64 {
    (text.len() as i64) / 4
}

/// Extract `prompt_tokens` from the last assistant message that has a usage JSON.
fn last_prompt_tokens(history: &[Message]) -> Option<u64> {
    for m in history.iter().rev() {
        if m.role != "assistant" {
            continue;
        }
        let Some(u) = m.usage.as_ref() else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(u) else {
            continue;
        };
        if let Some(pt) = v.get("prompt_tokens").and_then(|x| x.as_u64()) {
            return Some(pt);
        }
    }
    None
}

/// Approximate char size of a message (content + content_parts).
fn msg_chars(m: &Message) -> usize {
    let mut n = m.content.len();
    if let Some(p) = m.content_parts.as_ref() {
        n += p.len();
    }
    n
}

/// Pick the index of the first message of the verbatim tail. The boundary is
/// always a `user` message so tool-call/tool-result pairs stay intact. Returns
/// None when the conversation is too short to compact.
fn find_compaction_boundary(messages: &[Message], context_window: u64) -> Option<usize> {
    let len = messages.len();
    if len < 6 || context_window == 0 {
        return None;
    }
    // Tail budget: 25% of the context window, clamped to [2_000, 16_000] tokens.
    let tail_tokens = (context_window / 4).clamp(2_000, 16_000);
    let target_chars = tail_tokens * 4;
    // Candidate boundaries: a user message with >=2 msgs before it and >=4 after it.
    let candidates: Vec<usize> = (2..=(len - 4))
        .filter(|&i| messages[i].role == "user")
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let tail_chars = |i: usize| messages[i..len].iter().map(msg_chars).sum::<usize>();
    // Prefer the largest index (smallest tail) that still meets the budget;
    // fall back to the largest index (minimal valid tail).
    candidates
        .iter()
        .copied()
        .filter(|&i| tail_chars(i) >= target_chars as usize)
        .max()
        .or_else(|| candidates.iter().copied().max())
}

/// Flatten a message range into labeled text for the summarizer. Tool outputs
/// are truncated to 2000 chars. `prior_summary` is prepended so iterative
/// compaction absorbs the previous summary.
fn serialize_for_summary(messages: &[Message], prior_summary: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(p) = prior_summary.filter(|s| !s.is_empty()) {
        out.push_str("<previous_summary>\n");
        out.push_str(p);
        out.push_str("\n</previous_summary>\n\n");
    }
    out.push_str("<conversation>\n");
    for m in messages {
        match m.role.as_str() {
            "user" => {
                out.push_str("[User]: ");
                out.push_str(&m.content);
                out.push('\n');
            }
            "assistant" => {
                let blocks = parse_blocks(&m.content_parts);
                let text: String = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
                out.push_str("[Assistant]: ");
                out.push_str(if text.is_empty() { &m.content } else { &text });
                out.push('\n');
                for b in &blocks {
                    if b.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let input = b
                            .get("input")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(500)
                            .collect::<String>();
                        out.push_str(&format!("[Assistant tool call: {name}({input})]\n"));
                    }
                }
            }
            "tool" => {
                let blocks = parse_blocks(&m.content_parts);
                let name = blocks
                    .iter()
                    .find_map(|b| b.get("name").and_then(|v| v.as_str()))
                    .unwrap_or("tool");
                let truncated: String = m.content.chars().take(2000).collect();
                let suffix = if m.content.len() > 2000 {
                    " [truncated]"
                } else {
                    ""
                };
                out.push_str(&format!("[Tool result ({name})]: {truncated}{suffix}\n"));
            }
            _ => {}
        }
    }
    out.push_str("</conversation>");
    out
}

/// Run a compaction: summarize the head, keep a verbatim tail, persist a
/// `chat_sessions` row. Returns None if the conversation is too short.
pub async fn run_compaction(
    pool: &SqlitePool,
    pcfg: &crate::config::Provider,
    model: &str,
    history: &[Message],
    prior_summary: Option<&str>,
    context_window: u64,
    chat_id: &str,
) -> anyhow::Result<Option<crate::db::models::ChatSession>> {
    let Some(boundary) = find_compaction_boundary(history, context_window) else {
        return Ok(None);
    };
    let head = &history[..boundary];
    let body = serialize_for_summary(head, prior_summary);
    let req = CompleteRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: body,
            tool_call_id: None,
            tool_calls: None,
            parts: None,
        }],
        system: Some(SUMMARY_PROMPT.to_string()),
        temperature: None,
        max_tokens: None,
        tools: None,
    };

    // Stream the summary and collect text deltas.
    let (tx, mut rx) = mpsc::channel::<CompleteEvent>(64);
    let provider: Box<dyn crate::providers::Provider + Send> = provider_dyn(pcfg);
    let stream_task = tauri::async_runtime::spawn(async move {
        if let Err(e) = provider.stream_complete(req, tx).await {
            error!("compaction stream error: {e}");
        }
    });
    let mut summary_text = String::new();
    while let Some(ev) = rx.recv().await {
        if let CompleteEvent::BlockDelta { text: Some(t), .. } = ev {
            summary_text.push_str(&t);
        }
    }
    let _ = stream_task.await;
    let summary_text = summary_text.trim();
    if summary_text.is_empty() {
        warn!("compaction produced empty summary for chat {chat_id}");
        return Ok(None);
    }

    let id = Uuid::new_v4().to_string();
    let token_count = estimate_tokens(&serialize_for_summary(head, None));
    models::insert_chat_session(
        pool,
        &id,
        chat_id,
        summary_text,
        &history[boundary].id,
        token_count,
        Some(model),
        now_ms(),
    )
    .await?;
    Ok(Some(crate::db::models::ChatSession {
        id,
        chat_id: chat_id.to_string(),
        summary: summary_text.to_string(),
        boundary_message_id: history[boundary].id.clone(),
        token_count,
        model: Some(model.to_string()),
        created_at: now_ms(),
    }))
}

/// Best-effort LLM-generated chat title. Uses the configured secondary (fast)
/// model when available, otherwise the chat's main model. Returns the title,
/// or None on failure (caller falls back to a first-line heuristic).
pub async fn generate_chat_title(
    pool: &SqlitePool,
    config: &Config,
    chat: &Chat,
    project: Option<&crate::db::models::Project>,
) -> Option<String> {
    let (pcfg, model) = if let Some(sm) = config
        .defaults
        .secondary_model
        .as_ref()
        .filter(|m| !m.provider.is_empty() && !m.model.is_empty())
    {
        let p = crate::db::providers::get_provider(pool, &sm.provider)
            .await
            .ok()
            .flatten()?
            .to_config_provider();
        let m = crate::db::providers::get_model(pool, &sm.model)
            .await
            .ok()
            .flatten()?
            .name;
        (p, m)
    } else {
        let (p, model, _uuid) = resolve_provider(pool, chat, project, config).await?;
        (p, model)
    };

    let history =
        models::list_active_branch(pool, &chat.id, rules::active_leaf(&chat.meta).as_deref())
            .await
            .ok()?;
    let user_text = history
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let assistant_text = history
        .iter()
        .find(|m| m.role == "assistant")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    if user_text.is_empty() || assistant_text.is_empty() {
        return None;
    }
    let user_text: String = user_text.chars().take(500).collect();
    let assistant_text: String = assistant_text.chars().take(1000).collect();

    let req = CompleteRequest {
        model,
        messages: vec![ChatMessage {
            role: "user".into(),
            content: format!("User: {user_text}\n\nAssistant: {assistant_text}"),
            tool_call_id: None,
            tool_calls: None,
            parts: None,
        }],
        system: Some(TITLE_PROMPT.to_string()),
        temperature: Some(0.3),
        max_tokens: Some(48),
        tools: None,
    };

    // Stream the title and collect text deltas.
    let (tx, mut rx) = mpsc::channel::<CompleteEvent>(64);
    let provider: Box<dyn Provider + Send> = provider_dyn(&pcfg);
    let stream_task = tauri::async_runtime::spawn(async move {
        if let Err(e) = provider.stream_complete(req, tx).await {
            error!("title gen stream error: {e}");
        }
    });
    let mut title = String::new();
    while let Some(ev) = rx.recv().await {
        if let CompleteEvent::BlockDelta { text: Some(t), .. } = ev {
            title.push_str(&t);
        }
    }
    let _ = stream_task.await;

    let cleaned: String = title
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .chars()
        .take(60)
        .collect();
    if cleaned.is_empty() {
        warn!(
            "title generation produced an empty title for chat {}",
            chat.id
        );
        return None;
    }
    Some(cleaned)
}

/// Build LLM history, replacing the pre-boundary range with the session summary
/// (as a user-role message) when a compaction session applies.
fn build_history_with_compaction(
    history: &[Message],
    session: Option<&crate::db::models::ChatSession>,
    parts: &HashMap<String, Vec<ContentPart>>,
) -> Vec<ChatMessage> {
    if let Some(s) = session {
        if let Some(idx) = history.iter().position(|m| m.id == s.boundary_message_id) {
            let mut out = Vec::with_capacity(idx + 1);
            out.push(ChatMessage {
                role: "user".into(),
                content: format!("Summary of the earlier conversation:\n\n{}", s.summary),
                tool_call_id: None,
                tool_calls: None,
                parts: None,
            });
            out.extend(build_history(&history[idx..], parts));
            return out;
        }
    }
    build_history(history, parts)
}

/// Resolve (provider_id, model_id) for a new chat: prefer the given UUIDs, fall
/// back to `defaults.main_model`; only IDs that still exist in the DB count.
async fn default_refs(
    pool: &SqlitePool,
    config: &Config,
    preferred_provider: Option<&str>,
    preferred_model: Option<&str>,
) -> (Option<String>, Option<String>) {
    let main = config.defaults.main_model.as_ref();
    let provider_uuid = preferred_provider
        .filter(|s| !s.is_empty())
        .or(main.map(|m| m.provider.as_str()).filter(|s| !s.is_empty()));
    let model_uuid = preferred_model
        .filter(|s| !s.is_empty())
        .or(main.map(|m| m.model.as_str()).filter(|s| !s.is_empty()));
    let provider_id = match provider_uuid {
        Some(u) => crate::db::providers::get_provider(pool, u)
            .await
            .ok()
            .flatten()
            .map(|_| u.to_string()),
        None => None,
    };
    let model_id = match model_uuid {
        Some(u) => crate::db::providers::get_model(pool, u)
            .await
            .ok()
            .flatten()
            .map(|_| u.to_string()),
        None => None,
    };
    (provider_id, model_id)
}

/// Create a new standalone chat using the configured default model.
pub async fn create_chat(pool: &SqlitePool, config: &Config) -> anyhow::Result<Chat> {
    let id = Uuid::new_v4().to_string();
    let (provider_id, model_id) = default_refs(pool, config, None, None).await;
    let title = "New chat".to_string();
    let chat = models::create_chat(
        pool,
        &id,
        None,
        &title,
        provider_id.as_deref(),
        model_id.as_deref(),
        now_ms(),
    )
    .await?;
    Ok(chat)
}

/// Create a new chat inside a project, inheriting the project default model.
pub async fn create_project_chat(
    pool: &SqlitePool,
    config: &Config,
    project_id: &str,
) -> anyhow::Result<Chat> {
    let id = Uuid::new_v4().to_string();
    let project = models::get_project(pool, project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project not found"))?;
    let (provider_id, model_id) = default_refs(
        pool,
        config,
        project.default_provider_id.as_deref(),
        project.default_model_id.as_deref(),
    )
    .await;
    let title = "New chat".to_string();
    let chat = models::create_chat(
        pool,
        &id,
        Some(project_id),
        &title,
        provider_id.as_deref(),
        model_id.as_deref(),
        now_ms(),
    )
    .await?;
    Ok(chat)
}

/// Context bundled into a turn for context-aware tools (ask_user, todo, FTS, agents).
struct TurnCtx {
    pool: SqlitePool,
    chat: Chat,
    project: Option<crate::db::models::Project>,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    app: AppHandle,
    project_skills: Vec<projects::ProjectSkill>,
}

/// Send a user message and stream the assistant reply (with tool-calling loop).
#[allow(clippy::too_many_arguments)]
pub fn send(
    app: AppHandle,
    pool: SqlitePool,
    config: Arc<RwLock<Config>>,
    active: ActiveTurns,
    approvals: ApprovalRegistry,
    mcp: crate::mcp::McpManager,
    pending: PendingManager,
    pty: PtyManager,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    changes: projects::ChangeTracker,
    chat_id: String,
    text: String,
    attachment_ids: Vec<String>,
    skill_ids: Vec<String>,
) {
    let app2 = app.clone();
    let key = chat_id.clone();
    let active_c = active.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = start_turn(
            app2.clone(),
            pool,
            config,
            active_c.clone(),
            approvals,
            mcp,
            pending,
            pty,
            ask,
            task_nudge,
            runner,
            changes,
            chat_id.clone(),
            text,
            attachment_ids,
            skill_ids,
            TurnKind::Send,
        )
        .await
        {
            error!("chat turn failed: {e:#}");
            emit_status(&app2, &chat_id, ChatStatus::Error, Some(e.to_string()));
        }
    });
    if let Ok(mut map) = active.lock() {
        map.insert(key, handle);
    }
}

/// Regenerate the assistant reply from a given message's parent (branching).
pub fn regenerate(
    app: AppHandle,
    pool: SqlitePool,
    config: Arc<RwLock<Config>>,
    active: ActiveTurns,
    approvals: ApprovalRegistry,
    mcp: crate::mcp::McpManager,
    pending: PendingManager,
    pty: PtyManager,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    changes: projects::ChangeTracker,
    chat_id: String,
    from_message_id: String,
) {
    let app2 = app.clone();
    let key = chat_id.clone();
    let active_c = active.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = start_turn(
            app2.clone(),
            pool,
            config,
            active_c.clone(),
            approvals,
            mcp,
            pending,
            pty,
            ask,
            task_nudge,
            runner,
            changes,
            chat_id.clone(),
            from_message_id,
            Vec::new(),
            Vec::new(),
            TurnKind::Regenerate,
        )
        .await
        {
            error!("regenerate failed: {e:#}");
            emit_status(&app2, &chat_id, ChatStatus::Error, Some(e.to_string()));
        }
    });
    if let Ok(mut map) = active.lock() {
        map.insert(key, handle);
    }
}

/// Edit a user message and re-run from the new branch root (branching).
#[allow(clippy::too_many_arguments)]
pub fn edit_message(
    app: AppHandle,
    pool: SqlitePool,
    config: Arc<RwLock<Config>>,
    active: ActiveTurns,
    approvals: ApprovalRegistry,
    mcp: crate::mcp::McpManager,
    pending: PendingManager,
    pty: PtyManager,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    changes: projects::ChangeTracker,
    chat_id: String,
    message_id: String,
    new_text: String,
) {
    let app2 = app.clone();
    let key = chat_id.clone();
    let active_c = active.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = start_turn(
            app2.clone(),
            pool,
            config,
            active_c.clone(),
            approvals,
            mcp,
            pending,
            pty,
            ask,
            task_nudge,
            runner,
            changes,
            chat_id.clone(),
            message_id,
            Vec::new(),
            Vec::new(),
            TurnKind::Edit(new_text),
        )
        .await
        {
            error!("edit+rerun failed: {e:#}");
            emit_status(&app2, &chat_id, ChatStatus::Error, Some(e.to_string()));
        }
    });
    if let Ok(mut map) = active.lock() {
        map.insert(key, handle);
    }
}

enum TurnKind {
    /// `text` is the user input; parent = current active leaf.
    Send,
    /// `from_message_id` is the assistant message to replace; re-run from its parent.
    Regenerate,
    /// `message_id` is the user message to edit; `new_text` replaces it as a new branch root.
    Edit(String),
}

/// Prepend the bodies of the selected skills to the user message text.
/// Skills are resolved from the DB by id, in the order given by the caller.
async fn compose_with_skills(pool: &SqlitePool, skill_ids: &[String], user_text: &str) -> String {
    if skill_ids.is_empty() {
        return user_text.to_string();
    }
    let skills = match models::list_skills(pool).await {
        Ok(s) => s,
        Err(_) => return user_text.to_string(),
    };
    let mut blocks = Vec::new();
    for id in skill_ids {
        if let Some(s) = skills.iter().find(|s| s.id == *id) {
            if s.body.trim().is_empty() {
                continue;
            }
            blocks.push(format!(
                "<skill title=\"{}\">\n{}\n</skill>",
                s.title,
                s.body.trim()
            ));
        }
    }
    if blocks.is_empty() {
        return user_text.to_string();
    }
    format!(
        "<applied-skills>\n{}\n</applied-skills>\n\n{}",
        blocks.join("\n"),
        user_text
    )
}

/// Set up the turn: insert/locate the user message, set the active leaf, then
/// run the assistant generation loop from that leaf.
#[allow(clippy::too_many_arguments)]
async fn start_turn(
    app: AppHandle,
    pool: SqlitePool,
    config: Arc<RwLock<Config>>,
    active: ActiveTurns,
    approvals: ApprovalRegistry,
    mcp: crate::mcp::McpManager,
    pending: PendingManager,
    pty: PtyManager,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    changes: projects::ChangeTracker,
    chat_id: String,
    payload: String,
    attachment_ids: Vec<String>,
    skill_ids: Vec<String>,
    kind: TurnKind,
) -> anyhow::Result<()> {
    let chat = models::get_chat(&pool, &chat_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("chat not found"))?;
    let _project = projects::project_for_chat(&pool, &chat).await;

    // Determine the leaf to continue from and (for Send/Edit) the user message.
    let leaf_id: String = match kind {
        TurnKind::Send => {
            let expanded =
                projects::expand_refs(&pool, chat.project_id.as_deref(), &chat_id, &payload).await;
            let parent = rules::active_leaf(&chat.meta);
            let user_id = Uuid::new_v4().to_string();
            let user_msg = Message {
                id: user_id.clone(),
                chat_id: chat_id.clone(),
                parent_id: parent,
                role: "user".into(),
                content: compose_with_skills(&pool, &skill_ids, &expanded).await,
                content_parts: None,
                model: None,
                usage: None,
                thinking_ms: None,
                finish_reason: None,
                is_branch_root: 0,
                created_at: now_ms(),
            };
            models::insert_message(&pool, &user_msg, now_ms()).await?;
            models::touch_chat(&pool, &chat_id, now_ms()).await?;
            if !attachment_ids.is_empty() {
                if let Err(e) = attachments::link_to_message(&pool, &attachment_ids, &user_id).await
                {
                    warn!("failed to link attachments: {e:#}");
                }
            }
            user_id
        }
        TurnKind::Regenerate => {
            // The payload is the assistant message id to replace; re-run from its parent.
            let target = models::get_message(&pool, &payload)
                .await?
                .ok_or_else(|| anyhow::anyhow!("message not found"))?;
            target
                .parent_id
                .ok_or_else(|| anyhow::anyhow!("cannot regenerate a root message"))?
        }
        TurnKind::Edit(new_text) => {
            let target = models::get_message(&pool, &payload)
                .await?
                .ok_or_else(|| anyhow::anyhow!("message not found"))?;
            if target.role != "user" {
                anyhow::bail!("can only edit user messages");
            }
            let expanded =
                projects::expand_refs(&pool, chat.project_id.as_deref(), &chat_id, &new_text).await;
            let new_id = Uuid::new_v4().to_string();
            let user_msg = Message {
                id: new_id.clone(),
                chat_id: chat_id.clone(),
                parent_id: target.parent_id.clone(),
                role: "user".into(),
                content: expanded,
                content_parts: None,
                model: None,
                usage: None,
                thinking_ms: None,
                finish_reason: None,
                is_branch_root: 1,
                created_at: now_ms(),
            };
            models::insert_message(&pool, &user_msg, now_ms()).await?;
            let _ = models::touch_chat(&pool, &chat_id, now_ms()).await;
            new_id
        }
    };

    // Persist active leaf.
    let new_meta = rules::with_active_leaf(&chat.meta, &leaf_id);
    let _ = models::set_chat_meta(&pool, &chat_id, &new_meta, now_ms()).await;

    run_turn(
        app, pool, config, active, approvals, mcp, pending, pty, ask, task_nudge, runner, changes,
        chat_id, leaf_id,
    )
    .await
}

#[derive(Default)]
struct ToolCallAccum {
    block_id: String,
    name: String,
    id: String,
    args: String,
}

#[allow(clippy::too_many_arguments)]
async fn run_turn(
    app: AppHandle,
    pool: SqlitePool,
    config: Arc<RwLock<Config>>,
    _active: ActiveTurns,
    approvals: ApprovalRegistry,
    mcp: crate::mcp::McpManager,
    pending: PendingManager,
    pty: PtyManager,
    ask: AskRegistry,
    task_nudge: TaskNudge,
    runner: AgentRunner,
    changes: projects::ChangeTracker,
    chat_id: String,
    leaf_id: String,
) -> anyhow::Result<()> {
    let chat = models::get_chat(&pool, &chat_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("chat not found"))?;
    let project = projects::project_for_chat(&pool, &chat).await;
    let cfg = config.read().unwrap().clone();
    let (pcfg, model, model_uuid) =
        match resolve_provider(&pool, &chat, project.as_ref(), &cfg).await {
            Some(v) => v,
            None => {
                return Err(anyhow::anyhow!(
                    "no provider/model configured for this chat — select a model"
                ))
            }
        };
    if model.is_empty() {
        return Err(anyhow::anyhow!(
            "no model selected for chat; pick a model first"
        ));
    }
    if providers::build(&pcfg).is_none() {
        return Err(anyhow::anyhow!(
            "provider kind '{}' not supported",
            pcfg.kind
        ));
    }

    // Path security roots (project + chat paths).
    let roots = projects::allowed_roots(&pool, chat.project_id.as_deref(), &chat_id).await;
    crate::tools::set_path_roots(roots.clone());
    crate::tools::set_trash_mode(cfg.defaults.delete_to_trash);

    // History = active branch up to the leaf.
    let history = models::list_active_branch(&pool, &chat_id, Some(&leaf_id)).await?;
    let context_window = models::resolve_context_window(&pool, &model_uuid).await;
    let pct = cfg.defaults.auto_collapse_context_pct;
    let existing = models::latest_chat_session(&pool, &chat_id).await?;
    let over_threshold = pct > 0
        && context_window > 0
        && last_prompt_tokens(&history)
            .map(|pt| pt * 100 >= context_window * pct as u64)
            .unwrap_or(false);
    let session = if over_threshold {
        let prior = existing.as_ref().map(|s| s.summary.as_str());
        match run_compaction(
            &pool,
            &pcfg,
            &model,
            &history,
            prior,
            context_window,
            &chat_id,
        )
        .await
        {
            Ok(Some(s)) => {
                let _ = app.emit("chat:compacted", s.clone());
                Some(s)
            }
            Ok(None) => existing,
            Err(e) => {
                warn!("compaction failed for chat {chat_id}: {e}");
                existing
            }
        }
    } else {
        existing
    };
    // Multipart content from persisted attachments for this chat.
    let attachment_parts = build_attachment_parts(&pool, &chat_id)
        .await
        .unwrap_or_else(|e| {
            warn!("failed to load attachments for chat {chat_id}: {e}");
            HashMap::new()
        });
    let mut work = build_history_with_compaction(&history, session.as_ref(), &attachment_parts);

    // Effective system prompt (global + project + chat + rules + cross-chat).
    let system =
        rules::effective_system_prompt(&pool, &cfg, &chat, project.as_ref(), &cfg.defaults.mode)
            .await;
    // Minimal mode sends no system prompt — bare model only (docs/12).
    let mut system = if cfg.defaults.mode == "minimal" {
        None
    } else {
        system
    };
    // Auto-pull files changed since the last turn into context (docs/08).
    if cfg.defaults.auto_pull_changes {
        if let Some(p) = project.as_ref() {
            let drained = changes.drain(&p.id);
            if !drained.is_empty() {
                if let Some(section) = projects::changed_files_section(&pool, p, drained).await {
                    system = Some(match system {
                        Some(s) => format!("{s}\n\n{section}"),
                        None => section,
                    });
                }
            }
        }
    }

    // RAG retrieval: embed the latest user message and pull relevant snippets
    // from indexed chat attachments + project files. No-op without an
    // embedding model; never breaks the turn on retrieval errors.
    if cfg.defaults.mode != "minimal" {
        if let Some(section) =
            crate::rag::retrieve_for_turn(&pool, &cfg, &chat.id, chat.project_id.as_deref(), &work)
                .await
        {
            system = Some(match system {
                Some(s) => format!("{s}\n\n{section}"),
                None => section,
            });
        }
    }

    // Project skills auto-discovery from .agents/skills/ (and legacy .skills/) (IDEAS: auto-connect skills).
    let project_skills = if project.is_some() && cfg.defaults.mode != "minimal" {
        projects::discover_project_skills(&roots)
    } else {
        Vec::new()
    };
    if !project_skills.is_empty() {
        let mut lines = vec!["Project skills available in .agents/skills/:".to_string()];
        for s in &project_skills {
            lines.push(format!("- {}: {}", s.id, s.description));
        }
        lines.push(
            "Call the connect_skill tool with a skill_id to load a skill's full instructions when a request matches a skill. Supporting files in a skill's directory can be read with read_file using the dir path returned by connect_skill."
                .to_string(),
        );
        let section = lines.join("\n");
        system = Some(match system {
            Some(s) => format!("{s}\n\n{section}"),
            None => section,
        });
    }

    // Worker agents from the DB: delegation tools + roster section (docs/07).
    // Minimal mode has no tools and no system prompt, so nothing is registered.
    let agent_contracts: Vec<_> = if cfg.defaults.mode != "minimal" {
        crate::db::agents::list_active(&pool)
            .await
            .unwrap_or_default()
            .iter()
            .map(|row| row.to_contract())
            .collect()
    } else {
        Vec::new()
    };
    if let Some(block) = agents::roster_block(&agent_contracts) {
        system = Some(match system {
            Some(s) => format!("{s}\n\n{block}"),
            None => block,
        });
    }

    // Re-inject persisted tasks each turn (also restores state after context
    // compaction) and nudge when an open task list has gone stale. Minimal mode
    // sends no system prompt and has no todo_write tool, so skip both there.
    if cfg.defaults.mode != "minimal" {
        let chat_tasks = tasks::list_tasks(&pool, &chat.id).await.unwrap_or_default();
        if chat_tasks.is_empty() || !chat_tasks.iter().any(is_open_task) {
            task_nudge.reset(&chat_id);
        } else if task_nudge.bump(&chat_id) >= TASK_NUDGE_TURNS {
            task_nudge.reset(&chat_id);
            let section = "<system_reminder>\n\
                You have open tasks that haven't been updated recently. Use the todo_write tool \
                to update their status before continuing.\n\
                </system_reminder>"
                .to_string();
            system = Some(match system {
                Some(s) => format!("{s}\n\n{section}"),
                None => section,
            });
        }
        if !chat_tasks.is_empty() {
            let mut lines = vec!["<current_tasks>".to_string()];
            for (i, t) in chat_tasks.iter().enumerate() {
                lines.push(format!("{}. [{}] {}", i + 1, t.status, t.content));
            }
            lines.push("</current_tasks>".to_string());
            let section = lines.join("\n");
            system = Some(match system {
                Some(s) => format!("{s}\n\n{section}"),
                None => section,
            });
        }
    }

    // Tools: builtins filtered by mode + in-project flag (docs/12, docs/08).
    // Cross-chat retrieval tools honor the project cross_chat setting ("off"):
    let cross_chat = project
        .as_ref()
        .map(rules::project_cross_chat_mode)
        .unwrap_or_default();
    let in_project = project.is_some();
    let mut registry =
        ToolRegistry::builtin_for_mode_ctx(&cfg.defaults.mode, in_project, &cross_chat);
    registry.remove_disabled(&cfg.defaults.disabled_tools);
    mcp.add_to_registry(&mut registry, &cfg.defaults.mode).await;
    if cfg.defaults.mode != "minimal"
        && crate::db::web_hooks::has_active(&pool)
            .await
            .unwrap_or(false)
    {
        registry.register(Box::new(crate::tools::builtin::WebHookList));
        registry.register(Box::new(crate::tools::builtin::WebHookRun));
    }
    agents::add_to_registry(&mut registry, &agent_contracts, &cfg.defaults.mode);
    if !project_skills.is_empty() {
        registry.register(Box::new(crate::tools::builtin::ConnectSkill::new(
            project_skills.clone(),
        )));
    }
    let tools_json = if registry.names().is_empty() {
        None
    } else {
        Some(registry.openai_tools())
    };

    emit_status(&app, &chat_id, ChatStatus::Running, None);

    let turn_ctx = Arc::new(TurnCtx {
        pool: pool.clone(),
        chat: chat.clone(),
        project: project.clone(),
        ask: ask.clone(),
        task_nudge: task_nudge.clone(),
        runner: runner.clone(),
        app: app.clone(),
        project_skills: project_skills.clone(),
    });

    let max_iters = cfg.defaults.max_turns.max(1) as usize;
    let mut last_message_id = String::new();
    let mut last_usage: Option<crate::providers::Usage> = None;
    let mut last_finish = "stop".to_string();
    let mut last_assistant_text = String::new();
    // Parent for the next message in the branch (advances as we go).
    let mut current_parent: String = leaf_id.clone();

    let mut broke = false;
    for iter in 0..max_iters {
        let assistant_id = Uuid::new_v4().to_string();
        last_message_id = assistant_id.clone();

        let req = CompleteRequest {
            model: model.clone(),
            messages: work.clone(),
            system: system.clone(),
            temperature: None,
            max_tokens: None,
            tools: tools_json.clone(),
        };

        let (tx, mut rx) = mpsc::channel::<CompleteEvent>(64);
        let stream_err_slot = Arc::new(Mutex::new(None::<crate::providers::ProviderError>));
        let stream_task = {
            let provider: Box<dyn Provider + Send> = provider_dyn(&pcfg);
            let slot = stream_err_slot.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = provider.stream_complete(req, tx).await {
                    error!("provider stream error: {e}");
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(e);
                    }
                }
            })
        };

        let mut text_acc = String::new();
        let mut thinking_acc = String::new();
        let mut block_kind: HashMap<String, BlockType> = HashMap::new();
        let mut tool_calls: Vec<ToolCallAccum> = Vec::new();
        let mut block_starts: HashMap<String, std::time::Instant> = HashMap::new();
        let mut thinking_ms: i64 = 0;
        let mut finish_reason = "stop".to_string();
        let mut usage: Option<crate::providers::Usage> = None;

        while let Some(ev) = rx.recv().await {
            match ev {
                CompleteEvent::BlockStart {
                    block_id,
                    block_type,
                    info,
                } => {
                    block_kind.insert(block_id.clone(), block_type);
                    block_starts.insert(block_id.clone(), std::time::Instant::now());
                    if block_type == BlockType::ToolUse {
                        if let Some(i) = &info {
                            tool_calls.push(ToolCallAccum {
                                block_id: block_id.clone(),
                                name: i.name.clone(),
                                id: i.tool_call_id.clone(),
                                args: String::new(),
                            });
                        }
                    }
                    let _ = app.emit(
                        "chat:block_start",
                        BlockStartPayload {
                            chat_id: chat_id.clone(),
                            message_id: assistant_id.clone(),
                            block_id,
                            block_type,
                            info,
                        },
                    );
                }
                CompleteEvent::BlockDelta {
                    block_id,
                    text,
                    partial_json,
                } => {
                    if let Some(t) = &text {
                        match block_kind.get(&block_id) {
                            Some(BlockType::Thinking) => thinking_acc.push_str(t),
                            _ => text_acc.push_str(t),
                        }
                    }
                    if let Some(pj) = &partial_json {
                        if let Some(tc) = tool_calls.iter_mut().find(|t| t.block_id == block_id) {
                            tc.args.push_str(pj);
                        }
                    }
                    let _ = app.emit(
                        "chat:block_delta",
                        BlockDeltaPayload {
                            chat_id: chat_id.clone(),
                            message_id: assistant_id.clone(),
                            block_id,
                            text,
                            partial_json,
                        },
                    );
                }
                CompleteEvent::BlockStop { block_id } => {
                    if let Some(start) = block_starts.remove(&block_id) {
                        if block_kind.get(&block_id) == Some(&BlockType::Thinking) {
                            thinking_ms += start.elapsed().as_millis() as i64;
                        }
                    }
                    let _ = app.emit(
                        "chat:block_stop",
                        BlockStopPayload {
                            chat_id: chat_id.clone(),
                            message_id: assistant_id.clone(),
                            block_id,
                        },
                    );
                }
                CompleteEvent::Done {
                    finish_reason: fr,
                    usage: u,
                } => {
                    finish_reason = fr;
                    usage = u;
                }
            }
        }
        let _ = stream_task.await;
        let stream_err = stream_err_slot.lock().ok().and_then(|mut g| g.take());
        if stream_err.is_some() {
            finish_reason = "error".to_string();
        }

        // Persist the assistant message for this iteration (text + tool_use blocks).
        let mut blocks_arr: Vec<serde_json::Value> = Vec::new();
        if !thinking_acc.is_empty() {
            blocks_arr.push(serde_json::json!({
                "id": "block-thinking",
                "type": "thinking",
                "text": thinking_acc,
            }));
        }
        if !text_acc.is_empty() {
            blocks_arr.push(serde_json::json!({
                "id": "block-text",
                "type": "text",
                "text": text_acc,
            }));
        }
        for tc in &tool_calls {
            blocks_arr.push(serde_json::json!({
                "id": tc.block_id,
                "type": "tool_use",
                "name": tc.name,
                "tool_call_id": tc.id,
                "input": tc.args,
            }));
        }
        let parts = serde_json::to_string(&serde_json::json!({ "blocks": blocks_arr }))?;
        let assistant_msg = Message {
            id: assistant_id.clone(),
            chat_id: chat_id.clone(),
            parent_id: Some(current_parent.clone()),
            role: "assistant".into(),
            content: text_acc.clone(),
            content_parts: Some(parts),
            model: Some(model.clone()),
            usage: usage
                .as_ref()
                .map(|u| serde_json::to_string(u).unwrap_or_default()),
            thinking_ms: Some(thinking_ms),
            finish_reason: Some(finish_reason.clone()),
            is_branch_root: 0,
            created_at: now_ms(),
        };
        models::insert_message(&pool, &assistant_msg, now_ms()).await?;
        models::touch_chat(&pool, &chat_id, now_ms()).await?;
        current_parent = assistant_id.clone();
        last_assistant_text = text_acc.clone();

        if let Some(err) = stream_err {
            let kind = err.kind_str().to_string();
            let detail = err.detail();
            let retryable = err.retryable();
            warn!("chat turn stream error: {chat_id}: {kind}: {detail}");
            let _ = app.emit(
                "chat:turn_error",
                TurnErrorPayload {
                    chat_id: chat_id.clone(),
                    message_id: assistant_id.clone(),
                    kind,
                    message: detail.clone(),
                    retryable,
                },
            );
            let _ = app.emit(
                "chat:message_done",
                MessageDonePayload {
                    chat_id: chat_id.clone(),
                    message_id: assistant_id.clone(),
                    usage: usage.clone(),
                    finish_reason: "error".into(),
                },
            );
            let final_meta = rules::with_active_leaf(&chat.meta, &assistant_id);
            let _ = models::set_chat_meta(&pool, &chat_id, &final_meta, now_ms()).await;
            crate::tools::clear_path_roots();
            crate::tools::set_trash_mode(false);
            if let Ok(mut map) = _active.lock() {
                map.remove(&chat_id);
            }
            return Ok(());
        }

        // Append this assistant message to the working conversation.
        let tool_calls_out: Vec<ToolCall> = tool_calls
            .iter()
            .map(|t| ToolCall {
                id: t.id.clone(),
                kind: "function".into(),
                function: ToolCallFunction {
                    name: t.name.clone(),
                    arguments: t.args.clone(),
                },
            })
            .collect();
        work.push(ChatMessage {
            role: "assistant".into(),
            content: text_acc.clone(),
            tool_call_id: None,
            tool_calls: if tool_calls_out.is_empty() {
                None
            } else {
                Some(tool_calls_out)
            },
            parts: None,
        });

        last_usage = usage.clone();
        last_finish = finish_reason.clone();

        if finish_reason != "tool_calls" || tool_calls.is_empty() {
            broke = true;
            break;
        }

        // Execute each tool through the permission gate (+ approval if Ask),
        // log to tool_calls, and feed results back.
        for tc in tool_calls {
            let mut args: serde_json::Value =
                serde_json::from_str(&tc.args).unwrap_or(serde_json::Value::Null);

            let log_id = Uuid::new_v4().to_string();
            let log_row = ToolCallRow {
                id: log_id.clone(),
                chat_id: chat_id.clone(),
                message_id: assistant_id.clone(),
                tool_call_id: Some(tc.id.clone()),
                tool_name: tc.name.clone(),
                source: "builtin".to_string(),
                arguments: Some(tc.args.clone()),
                result: None,
                status: "running".to_string(),
                error: None,
                started_at: now_ms(),
                finished_at: None,
            };
            let _ = models::insert_tool_call(&pool, &log_row).await;

            let category = registry.category_of(&tc.name);
            // Context tools (ask_user, todo_write, search_project_chats, read_chat, add_rule,
            // update_rule, toggle_rule, delete_rule, connect_skill) bypass the file permission
            // gate — they're interaction/readonly and need no path check.
            let is_ctx_tool = matches!(
                tc.name.as_str(),
                "ask_user"
                    | "todo_write"
                    | "search_project_chats"
                    | "read_chat"
                    | "add_rule"
                    | "update_rule"
                    | "toggle_rule"
                    | "delete_rule"
                    | "connect_skill"
            );
            let is_agent_tool = tc.name == "agent__run_batch" || tc.name.starts_with("agent__");
            let decision = if is_ctx_tool || is_agent_tool {
                Decision::Allow
            } else {
                match category {
                    Some(cat) => permission::gate(
                        cat,
                        &tc.name,
                        &args,
                        &cfg.defaults.mode,
                        &cfg.defaults.command_toggle,
                        &cfg.defaults.edit_toggle,
                        &cfg.permissions,
                    ),
                    None => Decision::Deny("tool not in registry".into()),
                }
            };

            let result = match decision {
                Decision::Allow => {
                    if is_agent_tool {
                        execute_agent_tool(
                            &app,
                            &chat_id,
                            &assistant_id,
                            &tc.block_id,
                            &tc.name,
                            args,
                            &turn_ctx,
                            &cfg,
                        )
                        .await
                    } else if is_ctx_tool {
                        execute_ctx_tool(
                            &app,
                            &chat_id,
                            &assistant_id,
                            &tc.block_id,
                            &tc.name,
                            args,
                            &turn_ctx,
                        )
                        .await
                    } else {
                        execute(
                            &app,
                            &chat_id,
                            &assistant_id,
                            &tc.block_id,
                            &tc.name,
                            args,
                            &registry,
                            &pty,
                        )
                        .await
                    }
                }
                Decision::Stage => {
                    for p in paths_for_tool(&tc.name, &args) {
                        pending.snapshot(&chat_id, &p).await;
                    }
                    let r = registry
                        .call(&tc.name, args)
                        .await
                        .unwrap_or_else(|| crate::tools::ToolResult::err("tool failed"));
                    let changes = pending.changes(&chat_id).await;
                    let _ = app.emit(
                        "chat:pending_update",
                        PendingUpdatePayload {
                            chat_id: chat_id.clone(),
                            changes,
                        },
                    );
                    if r.is_error {
                        r
                    } else {
                        crate::tools::ToolResult::ok(format!(
                            "{} (staged, pending approval)",
                            r.content
                        ))
                    }
                }
                Decision::Deny(reason) => {
                    crate::tools::ToolResult::err(format!("denied: {reason}"))
                }
                Decision::Ask { summary, preview } => {
                    // Enrich the approval card with the resolved absolute path
                    // (and, for delete_path on an escaping symlink, the link
                    // target + an escape warning requiring explicit consent).
                    let unlink = tc.name == "delete_path";
                    let (summary, path, symlink_target, escape) =
                        match args.get("path").and_then(|v| v.as_str()) {
                            Some(p) => {
                                let info = crate::tools::approval_path(p, unlink);
                                let esc = info.escapes && unlink;
                                let summary = if esc {
                                    let t = info
                                        .symlink_target
                                        .as_ref()
                                        .map(|t| t.display().to_string())
                                        .unwrap_or_default();
                                    format!(
                                    "delete symlink {} -> {} (OUTSIDE project; target untouched)",
                                    info.display.display(),
                                    t
                                )
                                } else {
                                    format!("{} {}", tc.name, info.display.display())
                                };
                                (
                                    summary,
                                    Some(info.display.to_string_lossy().to_string()),
                                    info.symlink_target.map(|t| t.to_string_lossy().to_string()),
                                    esc,
                                )
                            }
                            None => (summary, None, None, false),
                        };
                    let destructive_mode = if tc.name == "delete_path" {
                        Some(
                            if cfg.defaults.delete_to_trash {
                                "trash"
                            } else {
                                "permanent"
                            }
                            .to_string(),
                        )
                    } else {
                        None
                    };
                    let summary = if tc.name == "delete_path" {
                        let tag = if cfg.defaults.delete_to_trash {
                            " (move to trash)"
                        } else {
                            " (permanent delete)"
                        };
                        format!("{summary}{tag}")
                    } else {
                        summary
                    };
                    let (req_id, rx) = approvals.register();
                    approval::emit_request(
                        &app,
                        approval::ApprovalRequest {
                            request_id: req_id.clone(),
                            chat_id: chat_id.clone(),
                            message_id: assistant_id.clone(),
                            block_id: tc.block_id.clone(),
                            tool_name: tc.name.clone(),
                            summary,
                            preview,
                            path,
                            symlink_target,
                            escape: if escape { Some(true) } else { None },
                            destructive_mode,
                        },
                    );
                    emit_status(
                        &app,
                        &chat_id,
                        ChatStatus::Running,
                        Some("needs approval".into()),
                    );
                    let approved = rx.await.unwrap_or(false);
                    if approved {
                        if escape {
                            if let Some(obj) = args.as_object_mut() {
                                obj.insert(
                                    "_unlink_escape_authorized".into(),
                                    serde_json::Value::Bool(true),
                                );
                            }
                        }
                        execute(
                            &app,
                            &chat_id,
                            &assistant_id,
                            &tc.block_id,
                            &tc.name,
                            args,
                            &registry,
                            &pty,
                        )
                        .await
                    } else {
                        crate::tools::ToolResult::err("denied by user")
                    }
                }
            };

            let _ = models::finish_tool_call(
                &pool,
                &log_id,
                if result.is_error { "error" } else { "done" },
                Some(&result.content),
                if result.is_error {
                    Some(&result.content)
                } else {
                    None
                },
                now_ms(),
            )
            .await;
            let _ = app.emit(
                "chat:tool_result",
                ToolResultPayload {
                    chat_id: chat_id.clone(),
                    message_id: assistant_id.clone(),
                    block_id: tc.block_id.clone(),
                    result: result.content.clone(),
                    is_error: result.is_error,
                },
            );

            // Persist the tool result as a tool-role message (parented in the branch).
            let tool_parts = serde_json::to_string(&serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "is_error": result.is_error,
            }))?;
            let tool_msg_id = Uuid::new_v4().to_string();
            let tool_msg = Message {
                id: tool_msg_id.clone(),
                chat_id: chat_id.clone(),
                parent_id: Some(assistant_id.clone()),
                role: "tool".into(),
                content: result.content.clone(),
                content_parts: Some(tool_parts),
                model: None,
                usage: None,
                thinking_ms: None,
                finish_reason: None,
                is_branch_root: 0,
                created_at: now_ms(),
            };
            models::insert_message(&pool, &tool_msg, now_ms()).await?;
            current_parent = tool_msg_id.clone();
            work.push(ChatMessage {
                role: "tool".into(),
                content: result.content,
                tool_call_id: Some(tc.id.clone()),
                tool_calls: None,
                parts: None,
            });
        }
        warn!("tool turn iter {iter}: executed tools, continuing");
    }

    // Update active leaf to the last message in the branch.
    let final_meta = rules::with_active_leaf(&chat.meta, &current_parent);
    let _ = models::set_chat_meta(&pool, &chat_id, &final_meta, now_ms()).await;

    // Cross-chat summary (docs/08): derive a 1-line summary from the assistant
    // reply (no extra LLM call in MVP).
    if chat.project_id.is_some() && !last_assistant_text.is_empty() {
        let summary: String = last_assistant_text.chars().take(200).collect();
        let summary_meta = rules::with_summary(&Some(final_meta.clone()), &summary);
        let _ = models::set_chat_meta(&pool, &chat_id, &summary_meta, now_ms()).await;
    }

    crate::tools::clear_path_roots();
    crate::tools::set_trash_mode(false);

    let turn_ok = last_finish != "error";
    let _ = app.emit(
        "chat:message_done",
        MessageDonePayload {
            chat_id: chat_id.clone(),
            message_id: last_message_id.clone(),
            usage: last_usage,
            finish_reason: last_finish,
        },
    );
    emit_status(&app, &chat_id, ChatStatus::Idle, None);

    // Loop exhausted max_turns without a clean stop: the model kept issuing
    // tool calls. message_done was already emitted with finish_reason
    // "tool_calls"; surface a visible error so the user knows why it stopped.
    if !broke {
        warn!("chat turn hit max_turns limit ({max_iters}) for {chat_id}");
        let _ = app.emit(
            "chat:turn_error",
            TurnErrorPayload {
                chat_id: chat_id.clone(),
                message_id: last_message_id.clone(),
                kind: "turn_limit".to_string(),
                message: format!(
                    "Reached the tool-turn limit ({max_iters}). The task may be incomplete — press Retry to continue from here, or raise max_turns in Settings/config."
                ),
                retryable: true,
            },
        );
    }

    // Auto-generate a chat title via LLM after the first turn, if the title is
    // still the default. Best-effort, non-blocking; falls back to a first-line
    // heuristic on LLM failure so the chat is never stuck as "New chat".
    if chat.title == "New chat" && turn_ok {
        let app2 = app.clone();
        let pool2 = pool.clone();
        let config2 = config.clone();
        let chat_id2 = chat_id.clone();
        tauri::async_runtime::spawn(async move {
            // Re-check fresh: the user may have renamed the chat during the turn.
            let chat = match models::get_chat(&pool2, &chat_id2).await {
                Ok(Some(c)) => c,
                _ => return,
            };
            if chat.title != "New chat" {
                return;
            }
            let project = projects::project_for_chat(&pool2, &chat).await;
            let cfg = config2.read().unwrap().clone();
            let title = match generate_chat_title(&pool2, &cfg, &chat, project.as_ref()).await {
                Some(t) => t,
                None => {
                    // Fallback: first line of the first user message, truncated.
                    let hist = match models::list_active_branch(
                        &pool2,
                        &chat_id2,
                        rules::active_leaf(&chat.meta).as_deref(),
                    )
                    .await
                    {
                        Ok(h) => h,
                        Err(_) => return,
                    };
                    let first_user = hist
                        .iter()
                        .find(|m| m.role == "user")
                        .map(|m| m.content.as_str())
                        .unwrap_or("");
                    let fallback: String = first_user
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(60)
                        .collect();
                    if fallback.is_empty() {
                        return;
                    }
                    fallback
                }
            };
            if models::rename_chat(&pool2, &chat_id2, &title, now_ms())
                .await
                .is_err()
            {
                return;
            }
            let _ = app2.emit(
                "chat:renamed",
                serde_json::json!({ "chat_id": chat_id2, "title": title }),
            );
            info!("auto-generated title for chat {chat_id2}: {title}");
        });
    }

    // A task linked to this chat has finished execution → move it to "review".
    match crate::db::project_tasks::finish_by_chat(&pool, &chat_id).await {
        Ok(Some((project_id, _))) => {
            let _ = app.emit(
                "project_task_changed",
                ProjectTaskChangedPayload { project_id },
            );
        }
        Ok(None) => {}
        Err(e) => warn!("finish_by_chat failed: {e:#}"),
    }

    if let Ok(mut map) = _active.lock() {
        map.remove(&chat_id);
    }
    info!("chat turn done: {chat_id}");
    Ok(())
}

fn provider_dyn(pcfg: &crate::config::Provider) -> Box<dyn Provider + Send> {
    providers::build(pcfg).expect("provider build failed mid-turn")
}

/// Cancel a running turn (user pressed Stop).
pub fn cancel(app: AppHandle, active: ActiveTurns, chat_id: String) {
    let removed = if let Ok(mut map) = active.lock() {
        map.remove(&chat_id)
    } else {
        None
    };
    if let Some(handle) = removed {
        handle.abort();
        info!("chat turn cancelled: {chat_id}");
    }
    crate::tools::clear_path_roots();
    crate::tools::set_trash_mode(false);
    emit_status(&app, &chat_id, ChatStatus::Cancelled, None);
    emit_status(&app, &chat_id, ChatStatus::Idle, None);
}

fn paths_for_tool(name: &str, args: &serde_json::Value) -> Vec<String> {
    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|x| x.to_string());
    match name {
        "move_path" => [s("src"), s("dst")].into_iter().flatten().collect(),
        "write_file" | "edit_file" | "apply_patch" | "make_dir" | "set_file_mode"
        | "delete_path" => s("path").into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Execute a context-aware tool (ask_user, todo_write, search_project_chats,
/// read_chat, add_rule). These need DB/chat/project access (docs/08, docs/17).
async fn execute_ctx_tool(
    app: &AppHandle,
    chat_id: &str,
    assistant_id: &str,
    block_id: &str,
    tool_name: &str,
    args: serde_json::Value,
    ctx: &Arc<TurnCtx>,
) -> crate::tools::ToolResult {
    match tool_name {
        "ask_user" => {
            let question = args
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let options: Vec<String> = args
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let multi_select = args
                .get("multi_select")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let context = args
                .get("context")
                .and_then(|v| v.as_str())
                .map(String::from);
            let (req_id, rx) = ctx.ask.register();
            ask::emit_ask(
                app,
                ask::AskRequest {
                    request_id: req_id,
                    chat_id: chat_id.into(),
                    message_id: assistant_id.into(),
                    block_id: block_id.into(),
                    question: question.clone(),
                    options,
                    multi_select,
                    context,
                },
            );
            emit_status(
                app,
                chat_id,
                ChatStatus::Running,
                Some("waiting for user answer".into()),
            );
            let answer = rx.await.unwrap_or_default();
            crate::tools::ToolResult::ok(if answer.is_empty() {
                "(user skipped the question)".into()
            } else {
                answer
            })
        }
        "todo_write" => {
            let todos_val = args
                .get("todos")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![]));
            let items: Vec<TaskInput> = match &todos_val {
                // Some providers send the array as a JSON-encoded string.
                serde_json::Value::String(s) => match serde_json::from_str::<Vec<TaskInput>>(s) {
                    Ok(v) => v,
                    Err(e) => {
                        return crate::tools::ToolResult::err(format!(
                            "invalid todos payload (string): {e}"
                        ));
                    }
                },
                _ => match serde_json::from_value::<Vec<TaskInput>>(todos_val.clone()) {
                    Ok(v) => v,
                    Err(e) => {
                        return crate::tools::ToolResult::err(format!(
                            "invalid todos payload: {e}"
                        ));
                    }
                },
            };
            match tasks::replace_tasks(&ctx.pool, chat_id, assistant_id, items).await {
                Ok(replaced) => {
                    ctx.task_nudge.reset(chat_id);
                    // Emit the FULL chat list (internal + agent rows) so the
                    // unified view stays consistent when a run is active.
                    let all = match tasks::list_tasks(&ctx.pool, chat_id).await {
                        Ok(v) => v,
                        Err(_) => replaced,
                    };
                    let _ = app.emit(
                        "chat:tasks_update",
                        TasksUpdatePayload {
                            chat_id: chat_id.into(),
                            tasks: all,
                        },
                    );
                    crate::tools::ToolResult::ok("task list updated")
                }
                Err(e) => crate::tools::ToolResult::err(format!("failed to save tasks: {e}")),
            }
        }
        "search_project_chats" => {
            let Some(project) = ctx.project.as_ref() else {
                return crate::tools::ToolResult::err("not in a project");
            };
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            match models::fts_search(&ctx.pool, query, Some(&project.id)).await {
                Ok(hits) => {
                    if hits.is_empty() {
                        crate::tools::ToolResult::ok("no matching chats found")
                    } else {
                        let out = hits
                            .iter()
                            .map(|h| format!("[{}] (chat {}): {}", h.snippet, h.chat_id, h.chat_id))
                            .collect::<Vec<_>>()
                            .join("\n");
                        crate::tools::ToolResult::ok(out)
                    }
                }
                Err(e) => crate::tools::ToolResult::err(format!("search failed: {e}")),
            }
        }
        "read_chat" => {
            let target_id = args.get("chat_id").and_then(|v| v.as_str()).unwrap_or("");
            if target_id.is_empty() {
                return crate::tools::ToolResult::err("missing chat_id");
            }
            // Verify the target is a sibling in the same project.
            if let Some(project) = ctx.project.as_ref() {
                let siblings = models::list_project_chats(&ctx.pool, &project.id)
                    .await
                    .unwrap_or_default();
                if !siblings.iter().any(|c| c.id == target_id) {
                    return crate::tools::ToolResult::err("chat not in this project");
                }
            } else {
                return crate::tools::ToolResult::err("not in a project");
            }
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
            match models::list_messages(&ctx.pool, target_id).await {
                Ok(msgs) => {
                    let mut out = String::new();
                    for m in msgs
                        .iter()
                        .rev()
                        .take(limit)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        out.push_str(&format!("[{}] {}\n{}\n\n", m.role, m.created_at, m.content));
                    }
                    if out.is_empty() {
                        out = "(empty chat)".into();
                    }
                    crate::tools::ToolResult::ok(out)
                }
                Err(e) => crate::tools::ToolResult::err(format!("read_chat failed: {e}")),
            }
        }
        "add_rule" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("chat");
            if text.is_empty() {
                return crate::tools::ToolResult::err("missing rule text");
            }
            let (scope, scope_id) = match scope {
                "project" => {
                    let Some(p) = ctx.project.as_ref() else {
                        return crate::tools::ToolResult::err("no project for project-scoped rule");
                    };
                    ("project", p.id.clone())
                }
                _ => ("chat", ctx.chat.id.clone()),
            };
            let id = Uuid::new_v4().to_string();
            if let Err(e) = models::add_rule(
                &ctx.pool,
                &id,
                scope,
                &scope_id,
                title,
                text,
                0,
                Some("model"),
                now_ms(),
            )
            .await
            {
                return crate::tools::ToolResult::err(format!("add_rule failed: {e}"));
            }
            crate::tools::ToolResult::ok(format!("rule added ({} scope): {}", scope, text))
        }
        "update_rule" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() || text.is_empty() {
                return crate::tools::ToolResult::err("missing rule id or text");
            }
            if let Err(e) = models::update_rule(&ctx.pool, id, title, text, now_ms()).await {
                return crate::tools::ToolResult::err(format!("update_rule failed: {e}"));
            }
            crate::tools::ToolResult::ok(format!("rule {id} updated"))
        }
        "toggle_rule" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let enabled = args
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if id.is_empty() {
                return crate::tools::ToolResult::err("missing rule id");
            }
            if let Err(e) = models::toggle_rule(&ctx.pool, id, enabled, now_ms()).await {
                return crate::tools::ToolResult::err(format!("toggle_rule failed: {e}"));
            }
            crate::tools::ToolResult::ok(format!(
                "rule {id} {}",
                if enabled { "enabled" } else { "disabled" }
            ))
        }
        "delete_rule" => {
            let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                return crate::tools::ToolResult::err("missing rule id");
            }
            if let Err(e) = models::delete_rule(&ctx.pool, id).await {
                return crate::tools::ToolResult::err(format!("delete_rule failed: {e}"));
            }
            crate::tools::ToolResult::ok(format!("rule {id} deleted"))
        }
        "connect_skill" => {
            let skill_id = args.get("skill_id").and_then(|v| v.as_str()).unwrap_or("");
            if skill_id.is_empty() {
                return crate::tools::ToolResult::err("missing 'skill_id'");
            }
            let Some(skill) = ctx.project_skills.iter().find(|s| s.id == skill_id) else {
                return crate::tools::ToolResult::err(format!("skill not found: {skill_id}"));
            };
            crate::tools::ToolResult::ok(format!(
                "<skill id=\"{}\" title=\"{}\" dir=\"{}\">\n{}\n</skill>",
                skill.id,
                skill.title,
                skill.dir.display(),
                skill.body
            ))
        }
        _ => crate::tools::ToolResult::err("unknown context tool"),
    }
}

/// Execute a file/exec tool. run_command uses an interactive PTY (docs/12);
/// other tools go through the registry directly.
#[allow(clippy::too_many_arguments)]
async fn execute(
    app: &AppHandle,
    chat_id: &str,
    assistant_id: &str,
    block_id: &str,
    tool_name: &str,
    args: serde_json::Value,
    registry: &ToolRegistry,
    pty: &PtyManager,
) -> crate::tools::ToolResult {
    if tool_name == "run_command" {
        let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let cwd = args
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);
        let env: std::collections::HashMap<String, String> = args
            .get("env")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let timeout_ms = crate::tools::parse_timeout_ms(&args);
        match pty.spawn(command, cwd.as_deref(), &env) {
            Ok((sid, mut rx)) => {
                let _ = app.emit(
                    "chat:pty_start",
                    PtyStartPayload {
                        chat_id: chat_id.into(),
                        message_id: assistant_id.into(),
                        block_id: block_id.into(),
                        session_id: sid.clone(),
                        command: command.into(),
                    },
                );
                let mut acc = String::new();
                let deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
                let mut timed_out = false;
                loop {
                    tokio::select! {
                        chunk = rx.recv() => {
                            let Some(chunk) = chunk else { break; };
                            let txt = String::from_utf8_lossy(&chunk).into_owned();
                            acc.push_str(&txt);
                            let _ = app.emit(
                                "chat:pty_output",
                                PtyOutputPayload {
                                    chat_id: chat_id.into(),
                                    message_id: assistant_id.into(),
                                    block_id: block_id.into(),
                                    session_id: sid.clone(),
                                    data: txt,
                                },
                            );
                        }
                        _ = tokio::time::sleep_until(deadline), if !timed_out => {
                            timed_out = true;
                            let _ = pty.kill(&sid);
                        }
                    }
                }
                let (code, status_label) = pty.wait(&sid).await.unwrap_or((-1, "unknown".into()));
                let _ = app.emit(
                    "chat:pty_done",
                    PtyDonePayload {
                        chat_id: chat_id.into(),
                        message_id: assistant_id.into(),
                        block_id: block_id.into(),
                        session_id: sid,
                        code,
                    },
                );
                crate::tools::truncate_text(&mut acc, 32 * 1024);
                let prefix = if timed_out {
                    format!("timed out after {timeout_ms}ms ({status_label})")
                } else {
                    status_label
                };
                crate::tools::ToolResult::ok(format!("[{prefix}]\n{acc}"))
            }
            Err(e) => crate::tools::ToolResult::err(format!("pty spawn failed: {e}")),
        }
    } else {
        registry
            .call(tool_name, args)
            .await
            .unwrap_or_else(|| crate::tools::ToolResult::err("tool failed"))
    }
}

/// Execute an agent delegation tool: `agent__run_batch` (parallel fan-out)
/// or `agent__<id>__run` (single). Foreground: awaits all runs and returns
/// the combined results; with `background: true` returns immediately and a
/// supervisor emits `agent:batch_complete` (docs/07, docs/16).
async fn execute_agent_tool(
    app: &AppHandle,
    chat_id: &str,
    assistant_id: &str,
    block_id: &str,
    tool_name: &str,
    args: serde_json::Value,
    ctx: &Arc<TurnCtx>,
    cfg: &Config,
) -> crate::tools::ToolResult {
    let _ = app;
    let _ = block_id;

    // Active agents from the DB; the default agent is the first by position.
    let agent_contracts: Vec<_> = crate::db::agents::list_active(&ctx.pool)
        .await
        .unwrap_or_default()
        .iter()
        .map(|row| row.to_contract())
        .collect();
    let default_agent = agent_contracts.first().map(|a| a.id.clone());
    let Some(default_agent) = default_agent else {
        return crate::tools::ToolResult::err("no active agent is configured");
    };

    if tool_name == "agent__batch_results" {
        let parent_id = args
            .get("parent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if parent_id.is_empty() {
            return crate::tools::ToolResult::err("missing 'parent_id'");
        }
        let runs = match models::list_agent_runs_by_parent(&ctx.pool, &parent_id).await {
            Ok(r) => r,
            Err(e) => return crate::tools::ToolResult::err(format!("db error: {e}")),
        };
        if runs.is_empty() {
            return crate::tools::ToolResult::ok(format!(
                "No agent runs found for parent_id {parent_id}."
            ));
        }
        return format_batch_from_runs(&runs);
    }

    // Working directory: first project path (dir) if in a project, else "".
    let cwd = if let Some(p) = ctx.project.as_ref() {
        if let Ok(paths) = models::list_project_paths(&ctx.pool, &p.id).await {
            paths
                .into_iter()
                .find(|pp| pp.kind == "dir")
                .map(|pp| pp.path)
                .unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let mode = cfg.defaults.mode.clone();

    // Build the subtask list.
    struct Sub {
        agent: String,
        prompt: String,
        files: Vec<String>,
    }
    let mut subs: Vec<Sub> = Vec::new();
    if tool_name == "agent__run_batch" {
        if let Some(tasks) = args.get("tasks").and_then(|v| v.as_array()) {
            for t in tasks {
                let prompt = t
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if prompt.is_empty() {
                    continue;
                }
                let agent = t
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| default_agent.clone());
                let files = t
                    .get("files")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                subs.push(Sub {
                    agent,
                    prompt,
                    files,
                });
            }
        }
        if subs.is_empty() {
            // Fallback: a single prompt passed directly.
            if let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) {
                if !prompt.is_empty() {
                    subs.push(Sub {
                        agent: default_agent.clone(),
                        prompt: prompt.to_string(),
                        files: Vec::new(),
                    });
                }
            }
        }
    } else {
        let agent = agents::agent_id_from_tool(tool_name).unwrap_or_else(|| default_agent.clone());
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let files = args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if prompt.is_empty() {
            return crate::tools::ToolResult::err("missing 'prompt'");
        }
        subs.push(Sub {
            agent,
            prompt,
            files,
        });
    }
    if subs.is_empty() {
        return crate::tools::ToolResult::err("no subtasks provided");
    }

    let background = tool_name == "agent__run_batch"
        && args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let isolate = subs.len() > 1 && mode == "write";
    let parent_id = uuid::Uuid::new_v4().to_string();
    let mut handles: Vec<(
        usize,
        String,
        tauri::async_runtime::JoinHandle<agents::AgentResult>,
    )> = Vec::new();

    for (i, s) in subs.iter().enumerate() {
        let params = agents::runner::SpawnParams {
            chat_id: chat_id.to_string(),
            parent_tool_call_id: Some(block_id.to_string()),
            parent_id: Some(parent_id.clone()),
            agent_id: s.agent.clone(),
            prompt: s.prompt.clone(),
            subtask_index: i,
            cwd: cwd.clone(),
            mode: mode.clone(),
            files: s.files.clone(),
            isolate,
            message_id: Some(assistant_id.to_string()),
        };
        let contract = match agent_contracts.iter().find(|a| a.id == s.agent) {
            Some(a) => a.clone(),
            None => {
                return crate::tools::ToolResult::err(format!(
                    "agent '{}' not found or inactive",
                    s.agent
                ));
            }
        };
        match ctx
            .runner
            .spawn(ctx.app.clone(), ctx.pool.clone(), cfg, contract, params)
            .await
        {
            Ok((rid, h)) => handles.push((i, rid, h)),
            Err(e) => {
                return crate::tools::ToolResult::err(format!(
                    "failed to spawn agent '{}': {e}",
                    s.agent
                ));
            }
        }
    }

    // Background: detach — the supervisor awaits every run and fires
    // `agent:batch_complete` (docs/16).
    if background {
        let batch_handles: Vec<(
            String,
            usize,
            tauri::async_runtime::JoinHandle<agents::AgentResult>,
        )> = handles.into_iter().map(|(i, rid, h)| (rid, i, h)).collect();
        let run_ids: Vec<String> = batch_handles
            .iter()
            .map(|(rid, _, _)| rid.clone())
            .collect();
        let agent_id_for_event = subs.first().map(|s| s.agent.clone()).unwrap_or_default();
        ctx.runner.spawn_batch_supervisor(
            ctx.app.clone(),
            parent_id.clone(),
            chat_id.to_string(),
            agent_id_for_event,
            batch_handles,
        );
        let mut lines = vec![format!(
            "Background batch detached (parent_id: {}). Runs queued:",
            parent_id
        )];
        for (i, s) in subs.iter().enumerate() {
            let preview: String = s.prompt.chars().take(80).collect();
            let rid = run_ids.get(i).cloned().unwrap_or_default();
            lines.push(format!("  [{}] {} — {}", i, rid, preview));
        }
        lines.push(
            "A agent:batch_complete notification will fire when all runs finish and a synthesis turn will start automatically. You may continue your current turn."
                .to_string(),
        );
        return crate::tools::ToolResult::ok(lines.join("\n"));
    }

    // Foreground: await every run (docs/16).
    let mut parts: Vec<(usize, String, agents::AgentResult)> = Vec::new();
    for (i, rid, h) in handles {
        let result = h
            .await
            .unwrap_or_else(|_| agents::AgentResult::error("run task panicked"));
        parts.push((i, rid, result));
    }
    let (combined, any_error) = format_batch_from_results(&parts);
    if any_error {
        crate::tools::ToolResult {
            content: combined,
            is_error: false,
        }
    } else {
        crate::tools::ToolResult::ok(combined)
    }
}

fn format_batch_from_results(parts: &[(usize, String, agents::AgentResult)]) -> (String, bool) {
    let mut out_parts = Vec::new();
    let mut any_error = false;
    for (i, rid, result) in parts {
        if matches!(result.status.as_str(), "error" | "cancelled" | "timeout") {
            any_error = true;
        }
        let head = format!("Subtask {} (run {}): [{}]", i, rid, result.status);
        let body = if result.text.is_empty() {
            "(no text output)".to_string()
        } else {
            result.text.clone()
        };
        out_parts.push(format!("{head}\n{body}"));
    }
    (out_parts.join("\n\n---\n\n"), any_error)
}

fn format_batch_from_runs(runs: &[models::AgentRun]) -> crate::tools::ToolResult {
    let mut out_parts = Vec::new();
    let mut any_error = false;
    for r in runs {
        if matches!(r.status.as_str(), "error" | "cancelled" | "timeout") {
            any_error = true;
        }
        let head = format!("Subtask {} (run {}): [{}]", r.subtask_index, r.id, r.status);
        let body = r
            .result_summary
            .clone()
            .unwrap_or_else(|| "(no text output)".to_string());
        out_parts.push(format!("{head}\n{body}"));
    }
    let combined = out_parts.join("\n\n---\n\n");
    crate::tools::ToolResult {
        content: combined,
        is_error: any_error,
    }
}

/// Export a chat to Markdown (docs/08). Walks the active branch.
pub async fn export_markdown(pool: &SqlitePool, chat_id: &str) -> anyhow::Result<String> {
    let chat = models::get_chat(pool, chat_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("chat not found"))?;
    let leaf = rules::active_leaf(&chat.meta);
    let msgs = models::list_active_branch(pool, chat_id, leaf.as_deref()).await?;
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", chat.title));
    out.push_str(&format!(
        "_Exported: {}_\n\n",
        chrono::DateTime::from_timestamp_millis(now_ms())
            .map(|d| d.to_rfc3339())
            .unwrap_or_default()
    ));
    let provider_name = match chat.provider_id.as_deref().filter(|s| !s.is_empty()) {
        Some(pid) => crate::db::providers::get_provider(pool, pid)
            .await
            .ok()
            .flatten()
            .map(|p| p.name),
        None => None,
    };
    let model_display = match chat.model_id.as_deref().filter(|s| !s.is_empty()) {
        Some(mid) => crate::db::providers::get_model(pool, mid)
            .await
            .ok()
            .flatten()
            .map(|m| {
                if m.display_name.is_empty() {
                    m.name
                } else {
                    m.display_name
                }
            }),
        None => None,
    };
    let model_line = match (provider_name, model_display) {
        (Some(p), Some(m)) => format!("**Model:** {p} / {m}"),
        _ => "**Model:** Undefined".to_string(),
    };
    out.push_str(&model_line);
    out.push_str("\n\n");
    for m in &msgs {
        if m.role == "tool" {
            continue;
        }
        let header = match m.role.as_str() {
            "user" => "## 🧑 User",
            "assistant" => "## 🤖 Assistant",
            "system" => "## System",
            _ => "## Tool",
        };
        out.push_str(header);
        if let Some(model) = m.model.as_ref() {
            if !model.is_empty() {
                out.push_str(&format!(" · `{}`", model));
            }
        }
        out.push('\n');
        let blocks = parse_blocks(&m.content_parts);
        // Thinking blocks first, collapsed so they don't break the reading flow.
        let thinking: Vec<&serde_json::Value> = blocks
            .iter()
            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("thinking"))
            .collect();
        if !thinking.is_empty() {
            out.push_str("\u{3c}details>\n<summary>Thinking</summary>\n\n");
            for b in &thinking {
                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                    if !t.is_empty() {
                        out.push_str(t.trim());
                        out.push_str("\n\n");
                    }
                }
            }
            out.push_str("\u{3c}/details>\n\n");
        }
        // For assistant messages prefer the text blocks; fallback to content.
        let text: String = if !blocks.is_empty() {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("")
        } else {
            m.content.clone()
        };
        out.push('\n');
        out.push_str(&text);
        out.push_str("\n\n");
    }
    Ok(out)
}
