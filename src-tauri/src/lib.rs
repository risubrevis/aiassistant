mod agents;
mod approval;
mod ask;
mod chat;
mod config;
mod db;
mod envinfo;
mod logger;
mod mcp;
mod net;
mod pending;
mod permission;
mod projects;
mod providers;
mod pty;
mod rag;
mod rules;
mod secrets;
mod tools;
mod updater;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::WindowExt;
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use agents::AgentRunner;
use approval::ApprovalRegistry;
use ask::AskRegistry;
use chat::ActiveTurns;
use config::{spawn_config_watcher, Config, GlobalRule, ModelRef, Skill};
use db::mcp_servers::{McpBody, McpServerInput};
use db::models::{
    Chat, ChatInfo, ChatPath, ChatSession, ChatSummary, Message, Project, ProjectPath, Rule,
};
use db::providers::{ProviderInput, ProviderModelInput, ProviderRow};
use db::DbPool;
use mcp::McpManager;
use pending::PendingManager;
use projects::ProjectWatcher;
use providers::{ModelInfo, Provider as DynProvider};
use pty::PtyManager;

struct AppState {
    config: Arc<RwLock<Config>>,
    pool: DbPool,
    log_path: std::path::PathBuf,
    _log_guard: WorkerGuard,
    log_levels: logger::LevelReloader,
    active: ActiveTurns,
    approvals: ApprovalRegistry,
    ask: AskRegistry,
    task_nudge: chat::TaskNudge,
    watcher: ProjectWatcher,
    changes: projects::ChangeTracker,
    runner: AgentRunner,
    mcp: McpManager,
    pending: PendingManager,
    pty: PtyManager,
    pending_settings_tab: Mutex<Option<String>>,
}

#[derive(Serialize)]
struct PathsPayload {
    config_path: String,
    log_path: String,
    data_dir: String,
}

// --- config ---

#[tauri::command]
fn config_get(state: State<AppState>) -> Config {
    state.config.read().unwrap().clone()
}

#[tauri::command]
fn app_paths(state: State<'_, AppState>) -> PathsPayload {
    PathsPayload {
        config_path: config::config_path().to_string_lossy().into_owned(),
        log_path: state.log_path.to_string_lossy().into_owned(),
        data_dir: db::data_dir().to_string_lossy().into_owned(),
    }
}

#[tauri::command]
fn update_install_supported() -> bool {
    updater::install_supported()
}

#[tauri::command]
async fn update_changelog(
    from_version: String,
    to_version: String,
) -> Result<Vec<updater::CommitEntry>, String> {
    updater::fetch_changelog(&from_version, &to_version)
        .await
        .map_err(|e| e.to_string())
}

/// Return the tail of the current log file (Settings → Diagnostics, docs/19).
#[tauri::command]
fn logs_read(state: State<'_, AppState>, tail_lines: Option<u32>) -> String {
    let n = tail_lines.unwrap_or(500) as usize;
    match std::fs::read_to_string(&state.log_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        Err(_) => String::from("(log file not found)"),
    }
}

#[tauri::command]
fn set_theme(theme: String, state: State<AppState>, app: AppHandle) -> Result<(), String> {
    if !matches!(theme.as_str(), "light" | "dark" | "system") {
        return Err("invalid theme".into());
    }
    config::write_appearance_field("theme", &theme).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.appearance.theme = theme;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Deserialize)]
struct NetworkInput {
    proxy_enabled: bool,
    proxy_type: String,
    proxy_host: String,
    proxy_port: u16,
    proxy_username: String,
    password: String, // empty = keep existing keychain value
    no_proxy: String,
    test_url: String,
    verify_tls: bool,
    ca_cert_path: String,
    connect_timeout_ms: u64,
}

impl NetworkInput {
    fn to_network(&self) -> config::Network {
        config::Network {
            proxy_enabled: self.proxy_enabled,
            proxy_type: self.proxy_type.clone(),
            proxy_host: self.proxy_host.clone(),
            proxy_port: self.proxy_port,
            proxy_username: self.proxy_username.clone(),
            no_proxy: self.no_proxy.clone(),
            test_url: self.test_url.clone(),
            verify_tls: self.verify_tls,
            ca_cert_path: self.ca_cert_path.clone(),
            connect_timeout_ms: self.connect_timeout_ms,
        }
    }
}

#[tauri::command]
fn set_network(input: NetworkInput, state: State<AppState>, app: AppHandle) -> Result<(), String> {
    if !matches!(
        input.proxy_type.as_str(),
        "http" | "https" | "socks5" | "socks5h"
    ) {
        return Err("invalid proxy_type".into());
    }
    let net = input.to_network();
    config::write_network(&net).map_err(|e| e.to_string())?;
    // Save password only when the user entered a new one; empty = keep existing.
    if !input.password.is_empty() {
        crate::secrets::set_proxy_password(&input.password).map_err(|e| e.to_string())?;
    }
    if let Ok(mut w) = state.config.write() {
        w.network = net.clone();
    }
    net::init(&net);
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn network_test(input: NetworkInput) -> Result<net::NetworkTestResult, String> {
    let net = input.to_network();
    let pw = if input.password.is_empty() {
        None
    } else {
        Some(input.password.as_str())
    };
    Ok(net::test_connection(&net, pw).await)
}

#[tauri::command]
fn network_has_password() -> bool {
    crate::secrets::get_proxy_password().is_some()
}

#[tauri::command]
fn clear_proxy_password() -> Result<(), String> {
    crate::secrets::delete_proxy_password().map_err(|e| e.to_string())
}

/// Truncate the log file (Settings → Diagnostics). The tracing appender holds
/// the file open in append mode, so subsequent events continue at offset 0.
#[tauri::command]
fn logs_clear(state: State<'_, AppState>) -> Result<(), String> {
    std::fs::File::create(&state.log_path).map_err(|e| e.to_string())?;
    tracing::info!("log file cleared from Settings");
    Ok(())
}

/// Persist new console/file log levels to config.toml and apply them live via
/// the tracing reload handles stored in AppState (takes effect immediately;
/// also used on the next startup after config load).
#[tauri::command]
fn set_log_level(
    level: String,
    file_level: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    const VALID: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];
    let level = level.trim().to_ascii_lowercase();
    if !VALID.contains(&level.as_str()) {
        return Err("invalid log level".into());
    }
    let file_level = file_level
        .as_deref()
        .map(str::trim)
        .map(String::from)
        .unwrap_or_else(|| state.config.read().unwrap().logging.file_level.clone())
        .to_ascii_lowercase();
    if !VALID.contains(&file_level.as_str()) {
        return Err("invalid file log level".into());
    }
    config::write_logging_field("level", &level).map_err(|e| e.to_string())?;
    config::write_logging_field("file_level", &file_level).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.logging.level = level.clone();
        w.logging.file_level = file_level.clone();
    }
    (state.log_levels)(&level, &file_level);
    tracing::info!("log level changed: console={level} file={file_level}");
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

// --- settings window ---

/// Open (or focus) the standalone native Settings window. `tab` optionally
/// selects a settings section: for an already-open window it is delivered via
/// the `settings:navigate` event; for a freshly created window via
/// `take_settings_tab` (the frontend reads it on mount).
#[tauri::command]
fn open_settings_window(
    app: AppHandle,
    state: State<'_, AppState>,
    tab: Option<String>,
) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        if let Some(t) = tab {
            let _ = app.emit_to("settings", "settings:navigate", t);
        }
    } else {
        if let Some(t) = tab.as_ref() {
            *state.pending_settings_tab.lock().unwrap() = Some(t.clone());
        }
        let win = tauri::WebviewWindowBuilder::new(
            &app,
            "settings",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("Settings")
        .inner_size(640.0, 460.0)
        .min_inner_size(480.0, 320.0)
        .resizable(true)
        .decorations(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
        let _ = win.restore_state(tauri_plugin_window_state::StateFlags::all());
    }
    Ok(())
}

#[tauri::command]
fn take_settings_tab(state: State<'_, AppState>) -> Option<String> {
    state.pending_settings_tab.lock().unwrap().take()
}

// --- chats ---

#[tauri::command]
async fn chat_create(state: State<'_, AppState>) -> Result<Chat, String> {
    let cfg = state.config.read().unwrap().clone();
    chat::create_chat(&state.pool, &cfg)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_create_in_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Chat, String> {
    let cfg = state.config.read().unwrap().clone();
    chat::create_project_chat(&state.pool, &cfg, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_list(state: State<'_, AppState>) -> Result<Vec<ChatSummary>, String> {
    db::models::list_chats(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_chats(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatSummary>, String> {
    db::models::list_project_chats(&state.pool, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_messages(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    db::models::list_messages(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_rename(
    chat_id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::rename_chat(&state.pool, &chat_id, &title, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_delete(chat_id: String, state: State<'_, AppState>) -> Result<(), String> {
    if let Err(e) = db::attachments::delete_chat_attachments(&state.pool, &chat_id).await {
        tracing::warn!("attachment cleanup failed for chat {chat_id}: {e}");
    }
    if let Err(e) = rag::clear_chat(&state.pool, &chat_id).await {
        tracing::warn!("rag clear chat failed for chat {chat_id}: {e}");
    }
    db::models::delete_chat(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_info(chat_id: String, state: State<'_, AppState>) -> Result<ChatInfo, String> {
    db::models::chat_info(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn activity_daily(
    state: State<'_, AppState>,
) -> Result<Vec<db::models::DayActivity>, String> {
    db::models::activity_daily(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn activity_day_detail(
    date: String,
    state: State<'_, AppState>,
) -> Result<db::models::DayDetail, String> {
    db::models::activity_day_detail(&state.pool, &date)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_set_pinned(
    chat_id: String,
    pinned: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::set_chat_pinned(&state.pool, &chat_id, pinned, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_reorder(ordered_ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    db::models::reorder_chats(&state.pool, &ordered_ids)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_set_model(
    chat_id: String,
    provider_id: Option<String>,
    model_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let provider_id = provider_id.filter(|s| !s.is_empty());
    let model_id = model_id.filter(|s| !s.is_empty());
    db::models::set_chat_model(
        &state.pool,
        &chat_id,
        provider_id.as_deref(),
        model_id.as_deref(),
        now_ms(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_set_project(
    chat_id: String,
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::set_chat_project(&state.pool, &chat_id, project_id.as_deref(), now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_set_system_prompt(
    chat_id: String,
    system_prompt: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::set_chat_system_prompt(&state.pool, &chat_id, system_prompt.as_deref(), now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn chat_send(
    chat_id: String,
    text: String,
    attachment_ids: Vec<String>,
    skill_ids: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) {
    chat::send(
        app,
        state.pool.clone(),
        state.config.clone(),
        state.active.clone(),
        state.approvals.clone(),
        state.mcp.clone(),
        state.pending.clone(),
        state.pty.clone(),
        state.ask.clone(),
        state.task_nudge.clone(),
        state.runner.clone(),
        state.changes.clone(),
        chat_id,
        text,
        attachment_ids,
        skill_ids,
    );
}

#[tauri::command]
async fn attachment_add(
    chat_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let att = db::attachments::create(&state.pool, &chat_id, &file_path)
        .await
        .map_err(|e| e.to_string())?;
    // Index text attachments for RAG (no-op when no embedding model is set).
    let cfg = state.config.read().unwrap().clone();
    if rag::embed_enabled(&cfg) {
        let pool_c = state.pool.clone();
        let att_c = att.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = rag::index_attachment(&pool_c, &cfg, &att_c).await {
                tracing::warn!("rag index attachment failed: {e}");
            }
        });
    }
    serde_json::to_value(att).map_err(|e| e.to_string())
}

#[tauri::command]
async fn attachment_remove(
    attachment_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Err(e) = rag::clear_attachment(&state.pool, &attachment_id).await {
        tracing::warn!("rag clear attachment failed: {e}");
    }
    db::attachments::remove(&state.pool, &attachment_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn attachments_for_chat(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let list = db::attachments::list_for_chat(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())?;
    list.iter()
        .map(|a| serde_json::to_value(a).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
async fn attachments_for_message(
    message_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let list = db::attachments::list_for_message(&state.pool, &message_id)
        .await
        .map_err(|e| e.to_string())?;
    list.iter()
        .map(|a| serde_json::to_value(a).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
async fn attachment_read_data_url(
    attachment_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    use base64::Engine;
    let att = db::attachments::get(&state.pool, &attachment_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "attachment not found".to_string())?;
    let bytes = std::fs::read(&att.storage_path).map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", att.mime_type, b64))
}

#[tauri::command]
fn chat_regenerate(
    chat_id: String,
    message_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) {
    chat::regenerate(
        app,
        state.pool.clone(),
        state.config.clone(),
        state.active.clone(),
        state.approvals.clone(),
        state.mcp.clone(),
        state.pending.clone(),
        state.pty.clone(),
        state.ask.clone(),
        state.task_nudge.clone(),
        state.runner.clone(),
        state.changes.clone(),
        chat_id,
        message_id,
    );
}

#[tauri::command]
fn chat_edit_message(
    chat_id: String,
    message_id: String,
    new_text: String,
    state: State<'_, AppState>,
    app: AppHandle,
) {
    chat::edit_message(
        app,
        state.pool.clone(),
        state.config.clone(),
        state.active.clone(),
        state.approvals.clone(),
        state.mcp.clone(),
        state.pending.clone(),
        state.pty.clone(),
        state.ask.clone(),
        state.task_nudge.clone(),
        state.runner.clone(),
        state.changes.clone(),
        chat_id,
        message_id,
        new_text,
    );
}

#[tauri::command]
async fn chat_export_markdown(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    chat::export_markdown(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, &content).map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_branches(
    _chat_id: String,
    parent_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    db::models::list_children(&state.pool, &parent_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_roots(chat_id: String, state: State<'_, AppState>) -> Result<Vec<Message>, String> {
    db::models::list_roots(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_set_active_leaf(
    chat_id: String,
    leaf_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let chat = db::models::get_chat(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found".to_string())?;
    let meta = rules::with_active_leaf(&chat.meta, &leaf_id);
    db::models::set_chat_meta(&state.pool, &chat_id, &meta, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_sessions_list(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatSession>, String> {
    db::models::list_chat_sessions(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_compact(
    chat_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ChatSession, String> {
    let chat = db::models::get_chat(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found".to_string())?;
    let cfg = state.config.read().unwrap().clone();
    let project = projects::project_for_chat(&state.pool, &chat).await;
    let (main_pcfg, main_model, main_model_uuid) =
        match chat::resolve_provider(&state.pool, &chat, project.as_ref(), &cfg).await {
            Some(v) => v,
            None => return Err("no provider/model configured for this chat".into()),
        };
    // The tail budget is derived from the chat's main model context window
    // (that model continues the conversation after compaction).
    let mut context_window =
        db::models::resolve_context_window(&state.pool, &main_model_uuid).await;
    // Manual compaction must work even when no context window is configured for
    // the model (no override and never cached): use a conservative default so
    // the tail budget stays reasonable.
    if context_window == 0 {
        context_window = 16_384;
    }
    // Summarizer model: the configured secondary (fast) model if present,
    // otherwise the chat's main model. The context window above stays tied to
    // the main model so the kept tail matches the continuation model's capacity.
    let (pcfg, model) = if let Some(sm) = cfg
        .defaults
        .secondary_model
        .as_ref()
        .filter(|m| !m.provider.is_empty() && !m.model.is_empty())
    {
        let p = db::providers::get_provider(&state.pool, &sm.provider)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no provider for secondary model".to_string())?
            .to_config_provider();
        let m = db::providers::get_model(&state.pool, &sm.model)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no model for secondary model".to_string())?
            .name;
        (p, m)
    } else {
        (main_pcfg.clone(), main_model.clone())
    };
    let leaf_id = rules::active_leaf(&chat.meta);
    let history = db::models::list_active_branch(&state.pool, &chat_id, leaf_id.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let prior = db::models::latest_chat_session(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())?
        .map(|s| s.summary);
    let session = chat::run_compaction(
        &state.pool,
        &pcfg,
        &model,
        &history,
        prior.as_deref(),
        context_window,
        &chat_id,
    )
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "conversation too short to compact".to_string())?;
    let _ = app.emit("chat:compacted", &session);
    Ok(session)
}

// --- rag ---

#[tauri::command]
async fn rag_reindex_project(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let cfg = state.config.read().unwrap().clone();
    rag::reindex_project(&state.pool, &cfg, &project_id)
        .await
        .map(|n| n as i64)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rag_clear_project(project_id: String, state: State<'_, AppState>) -> Result<(), String> {
    rag::clear_project(&state.pool, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rag_status(
    project_id: Option<String>,
    chat_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let (chat, project) = rag::status(&state.pool, chat_id.as_deref(), project_id.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let total = rag::count_all(&state.pool).await.unwrap_or(0);
    Ok(serde_json::json!({ "chat_chunks": chat, "project_chunks": project, "total_chunks": total }))
}

#[tauri::command]
async fn rag_clear_all(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    rag::clear_all(&state.pool)
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("rag:cleared", ());
    Ok(())
}

#[tauri::command]
async fn chat_context_window(chat_id: String, state: State<'_, AppState>) -> Result<u64, String> {
    let chat = db::models::get_chat(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found".to_string())?;
    let cfg = state.config.read().unwrap().clone();
    let project = projects::project_for_chat(&state.pool, &chat).await;
    let Some((_, _, model_uuid)) =
        chat::resolve_provider(&state.pool, &chat, project.as_ref(), &cfg).await
    else {
        return Ok(0);
    };
    Ok(db::models::resolve_context_window(&state.pool, &model_uuid).await)
}

#[tauri::command]
fn approve_request(request_id: String, approved: bool, state: State<'_, AppState>) -> bool {
    state.approvals.resolve(&request_id, approved)
}

#[tauri::command]
fn ask_user_reply(request_id: String, answer: String, state: State<'_, AppState>) -> bool {
    state.ask.resolve(&request_id, answer)
}

// --- agents (docs/07, docs/16) ---

#[derive(Serialize)]
pub struct AgentInfo {
    pub row: db::agents::AgentRow,
    pub status: String,
    pub status_detail: String,
}

fn validate_agent_input(input: &db::agents::AgentInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("agent name is required".into());
    }
    if input.command.trim().is_empty() {
        return Err("agent command is required".into());
    }
    if input.args.iter().any(|a| a.contains("{model}")) && input.default_model.trim().is_empty() {
        return Err("default_model is required when args contain {model}".into());
    }
    Ok(())
}

fn agent_status(command: &str, is_active: bool) -> (String, String) {
    if !is_active {
        return ("inactive".into(), "inactive".into());
    }
    if command.trim().is_empty() {
        return ("error".into(), "command not set".into());
    }
    match agents::presets::detect_binary(command) {
        Some(_) => ("ok".into(), "connected".into()),
        None => ("error".into(), "binary not found".into()),
    }
}

#[tauri::command]
async fn agent_list(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    let rows = db::agents::list(&state.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let (status, status_detail) = agent_status(&row.command, row.is_active);
            AgentInfo {
                row,
                status,
                status_detail,
            }
        })
        .collect())
}

#[tauri::command]
async fn agent_presets() -> Result<Vec<db::agents::AgentInput>, String> {
    Ok(agents::presets::presets()
        .iter()
        .map(db::agents::AgentInput::from_contract)
        .collect())
}

#[tauri::command]
async fn agent_detect(command: String) -> Result<Option<String>, String> {
    Ok(agents::presets::detect_binary(&command))
}

#[tauri::command]
async fn agent_create(
    input: db::agents::AgentInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<db::agents::AgentRow, String> {
    validate_agent_input(&input)?;
    let row = db::agents::create(&state.pool, input)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("agents:changed", ()).map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
async fn agent_update(
    id: String,
    input: db::agents::AgentInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<db::agents::AgentRow, String> {
    validate_agent_input(&input)?;
    let row = db::agents::update(&state.pool, &id, input)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("agents:changed", ()).map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
async fn agent_delete(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::agents::delete(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("agents:changed", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn agent_reorder(
    ordered_ids: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::agents::reorder(&state.pool, &ordered_ids)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("agents:changed", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn agent_set_active(
    id: String,
    is_active: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::agents::set_active(&state.pool, &id, is_active)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("agents:changed", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn agent_test(id: String, state: State<'_, AppState>) -> Result<String, String> {
    let row = db::agents::get(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("agent '{id}' not found"))?;
    let contract = row.to_contract();
    let bridge = agents::bridge_for(&contract);
    let mut task =
        agents::AgentTask::new("Reply with the single word: ready", contract.cwd.clone());
    task.timeout_ms = 20_000;
    task.mode = "plan".into();
    let (tx, _rx) = tokio::sync::mpsc::channel::<agents::AgentEvent>(16);
    let result = bridge.run("test", task, tx).await;
    Ok(format!("[{}] {}", result.status, result.text))
}

#[tauri::command]
async fn agent_test_input(input: db::agents::AgentInput) -> Result<String, String> {
    if input.command.trim().is_empty() {
        return Err("agent command is required".into());
    }
    let contract = input.to_contract("test");
    let bridge = agents::bridge_for(&contract);
    let mut task =
        agents::AgentTask::new("Reply with the single word: ready", contract.cwd.clone());
    task.timeout_ms = 20_000;
    task.mode = "plan".into();
    let (tx, _rx) = tokio::sync::mpsc::channel::<agents::AgentEvent>(16);
    let result = bridge.run("test", task, tx).await;
    Ok(format!("[{}] {}", result.status, result.text))
}

#[tauri::command]
async fn agent_runs_list(
    chat_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<db::models::AgentRun>, String> {
    db::models::list_agent_runs(&state.pool, chat_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn agent_run_cancel(run_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.runner.cancel(&run_id).await;
    Ok(())
}

#[tauri::command]
async fn agent_run_approve(run_id: String, state: State<'_, AppState>) -> Result<String, String> {
    state
        .runner
        .approve(&state.pool, &run_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn agent_run_reject(run_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .runner
        .reject(&state.pool, &run_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn agent_run_diff(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<agents::worktree::WorktreeDiff, String> {
    state
        .runner
        .run_diff(&state.pool, &run_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn task_list(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<db::tasks::Task>, String> {
    db::tasks::list_tasks(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn task_clear(chat_id: String, state: State<'_, AppState>) -> Result<(), String> {
    db::tasks::clear_tasks(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

// --- project tasks (kanban) ---

#[tauri::command]
async fn project_task_list(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<db::project_tasks::ProjectTask>, String> {
    db::project_tasks::list_for_project(&state.pool, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_task_create(
    project_id: String,
    title: String,
    description: String,
    status: String,
    priority: String,
    state: State<'_, AppState>,
) -> Result<db::project_tasks::ProjectTask, String> {
    db::project_tasks::create(
        &state.pool,
        &project_id,
        &title,
        &description,
        &status,
        &priority,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_task_update(
    id: String,
    title: String,
    description: String,
    status: String,
    priority: String,
    state: State<'_, AppState>,
) -> Result<db::project_tasks::ProjectTask, String> {
    db::project_tasks::update(&state.pool, &id, &title, &description, &status, &priority)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_task_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    db::project_tasks::delete(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_task_move(
    id: String,
    to_status: String,
    to_position: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::project_tasks::move_task(&state.pool, &id, &to_status, to_position)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_task_run(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Chat, String> {
    let task = db::project_tasks::get(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "task not found".to_string())?;
    // Already linked to a chat — return it without re-sending.
    if let Some(chat_id) = task.chat_id.as_deref() {
        return db::models::get_chat(&state.pool, chat_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "linked chat not found".to_string());
    }
    let cfg = state.config.read().unwrap().clone();
    let chat = chat::create_project_chat(&state.pool, &cfg, &task.project_id)
        .await
        .map_err(|e| e.to_string())?;
    db::models::rename_chat(&state.pool, &chat.id, &task.title, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    db::project_tasks::link_chat(&state.pool, &id, &chat.id)
        .await
        .map_err(|e| e.to_string())?;
    let text = if task.description.trim().is_empty() {
        task.title.clone()
    } else {
        format!("# {}\n\n{}", task.title, task.description)
    };
    chat::send(
        app,
        state.pool.clone(),
        state.config.clone(),
        state.active.clone(),
        state.approvals.clone(),
        state.mcp.clone(),
        state.pending.clone(),
        state.pty.clone(),
        state.ask.clone(),
        state.task_nudge.clone(),
        state.runner.clone(),
        state.changes.clone(),
        chat.id.clone(),
        text,
        vec![],
        vec![],
    );
    db::models::get_chat(&state.pool, &chat.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found after create".to_string())
}

// --- prompts ---

#[tauri::command]
async fn prompt_list(state: State<'_, AppState>) -> Result<Vec<db::prompts::Prompt>, String> {
    db::prompts::list_all(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_list_favorites(
    state: State<'_, AppState>,
) -> Result<Vec<db::prompts::Prompt>, String> {
    db::prompts::list_favorites(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_create(
    title: String,
    body: String,
    project_id: Option<String>,
    attach_files: Vec<String>,
    skill_ids: Vec<String>,
    is_favorite: bool,
    state: State<'_, AppState>,
) -> Result<db::prompts::Prompt, String> {
    db::prompts::create(
        &state.pool,
        &title,
        &body,
        project_id.as_deref(),
        &attach_files,
        &skill_ids,
        is_favorite,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn prompt_update(
    id: String,
    title: String,
    body: String,
    project_id: Option<String>,
    attach_files: Vec<String>,
    skill_ids: Vec<String>,
    is_favorite: bool,
    state: State<'_, AppState>,
) -> Result<db::prompts::Prompt, String> {
    db::prompts::update(
        &state.pool,
        &id,
        &title,
        &body,
        project_id.as_deref(),
        &attach_files,
        &skill_ids,
        is_favorite,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    db::prompts::delete(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_set_favorite(
    id: String,
    is_favorite: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::prompts::set_favorite(&state.pool, &id, is_favorite)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_move(
    id: String,
    to_position: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::prompts::move_prompt(&state.pool, &id, to_position)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prompt_run(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Chat, String> {
    let prompt = db::prompts::get(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "prompt not found".to_string())?;
    // Attached files must exist before a chat is created.
    let missing: Vec<String> = prompt
        .attach_files
        .iter()
        .filter(|p| !std::path::Path::new(p).exists())
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(format!("missing attached files: {}", missing.join(", ")));
    }
    let cfg = state.config.read().unwrap().clone();
    let chat = match prompt.project_id.as_deref() {
        Some(pid) => chat::create_project_chat(&state.pool, &cfg, pid).await,
        None => chat::create_chat(&state.pool, &cfg).await,
    }
    .map_err(|e| e.to_string())?;
    db::models::rename_chat(&state.pool, &chat.id, &prompt.title, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    let mut attachment_ids = Vec::with_capacity(prompt.attach_files.len());
    for path in &prompt.attach_files {
        let att = db::attachments::create(&state.pool, &chat.id, path)
            .await
            .map_err(|e| e.to_string())?;
        // Index text attachments for RAG (no-op when no embedding model is set).
        if rag::embed_enabled(&cfg) {
            let pool_c = state.pool.clone();
            let cfg_c = cfg.clone();
            let att_c = att.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = rag::index_attachment(&pool_c, &cfg_c, &att_c).await {
                    tracing::warn!("rag index attachment failed: {e}");
                }
            });
        }
        attachment_ids.push(att.id);
    }
    let text = if prompt.body.trim().is_empty() {
        prompt.title.clone()
    } else {
        format!("# {}\n\n{}", prompt.title, prompt.body)
    };
    chat::send(
        app,
        state.pool.clone(),
        state.config.clone(),
        state.active.clone(),
        state.approvals.clone(),
        state.mcp.clone(),
        state.pending.clone(),
        state.pty.clone(),
        state.ask.clone(),
        state.task_nudge.clone(),
        state.runner.clone(),
        state.changes.clone(),
        chat.id.clone(),
        text,
        attachment_ids,
        prompt.skill_ids.clone(),
    );
    db::models::get_chat(&state.pool, &chat.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "chat not found after create".to_string())
}

// --- projects ---

#[tauri::command]
async fn project_create(
    name: String,
    color: String,
    state: State<'_, AppState>,
) -> Result<Project, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let p = db::models::create_project(&state.pool, &id, &name, &color, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    Ok(p)
}

#[tauri::command]
async fn project_list(state: State<'_, AppState>) -> Result<Vec<Project>, String> {
    db::models::list_projects(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_get(id: String, state: State<'_, AppState>) -> Result<Option<Project>, String> {
    db::models::get_project(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_update(
    project: Project,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::models::update_project(&state.pool, &project, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    state.watcher.restart_for_project(
        app,
        state.pool.clone(),
        state.changes.clone(),
        project.id.clone(),
    );
    Ok(())
}

#[tauri::command]
async fn project_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.watcher.stop_project(&id);
    state.changes.clear(&id);
    if let Err(e) = rag::clear_project_and_chats(&state.pool, &id).await {
        tracing::warn!("rag clear project+chats failed for project {id}: {e}");
    }
    db::models::delete_project(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_set_pinned(
    project_id: String,
    pinned: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::set_project_pinned(&state.pool, &project_id, pinned, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_reorder(
    ordered_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::reorder_projects(&state.pool, &ordered_ids)
        .await
        .map_err(|e| e.to_string())
}

// --- project paths ---

#[tauri::command]
async fn project_paths_list(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectPath>, String> {
    db::models::list_project_paths(&state.pool, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn project_path_add(
    project_id: String,
    path: String,
    kind: String,
    watch: bool,
    exclude_globs: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::models::add_project_path(
        &state.pool,
        &id,
        &project_id,
        &path,
        &kind,
        watch,
        exclude_globs.as_deref(),
        now_ms(),
    )
    .await
    .map_err(|e| e.to_string())?;
    // Auto-import known rule files (AGENTS.md, CLAUDE.md, ...) as project rules.
    if kind == "dir" {
        let dir = std::path::Path::new(&path);
        let existing = db::models::list_rules(&state.pool, "project", &project_id)
            .await
            .map_err(|e| e.to_string())?;
        for (_marker, filename, _full, content) in projects::detect_rule_files(dir) {
            let header = format!("[{}]\n", filename);
            // Skip when an auto-imported rule for this file already exists.
            if existing
                .iter()
                .any(|r| r.added_by.as_deref() == Some("auto") && r.text.starts_with(&header))
            {
                continue;
            }
            let rule_id = uuid::Uuid::new_v4().to_string();
            let text = format!("{}{}", header, content);
            db::models::add_rule(
                &state.pool,
                &rule_id,
                "project",
                &project_id,
                filename.as_str(),
                &text,
                0,
                Some("auto"),
                now_ms(),
            )
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    let pid_c = project_id.clone();
    state
        .watcher
        .restart_for_project(app, state.pool.clone(), state.changes.clone(), project_id);
    // Index the new path for RAG (no-op when no embedding model is set).
    let cfg = state.config.read().unwrap().clone();
    if rag::embed_enabled(&cfg) {
        let pool_c = state.pool.clone();
        let path_c = path.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = rag::index_project_path(&pool_c, &cfg, &pid_c, &path_c).await {
                tracing::warn!("rag index project path failed: {e}");
            }
        });
    }
    Ok(())
}

#[tauri::command]
async fn project_path_delete(
    id: String,
    project_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let path_row = db::models::get_project_path(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(path_row) = path_row {
        if path_row.kind == "dir" {
            // Auto-rules don't track their source dir, so match by known rule-file
            // headers. If several attached dirs had the same rule file, removing one
            // drops the shared auto-rule; re-adding the other dir re-imports it.
            let auto_rules = db::models::list_rules(&state.pool, "project", &project_id)
                .await
                .map_err(|e| e.to_string())?;
            for name in projects::RULE_FILES {
                let header = format!("[{}]\n", name);
                for r in auto_rules.iter().filter(|r| {
                    r.added_by.as_deref() == Some("auto") && r.text.starts_with(&header)
                }) {
                    let _ = db::models::delete_rule(&state.pool, &r.id).await;
                }
            }
        }
    }
    db::models::delete_project_path(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    state
        .watcher
        .restart_for_project(app, state.pool.clone(), state.changes.clone(), project_id);
    Ok(())
}

// --- chat paths ---

#[tauri::command]
async fn chat_paths_list(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ChatPath>, String> {
    db::models::list_chat_paths(&state.pool, &chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_path_add(
    chat_id: String,
    path: String,
    kind: String,
    watch: bool,
    exclude_globs: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::models::add_chat_path(
        &state.pool,
        &id,
        &chat_id,
        &path,
        &kind,
        watch,
        exclude_globs.as_deref(),
        now_ms(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_path_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    db::models::delete_chat_path(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

// --- rules ---

// --- global prompts & rules (Settings → Prompts) ---

#[tauri::command]
async fn set_system_prompt(
    text: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    config::write_system_prompt(&text).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.system_prompt = text;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn environment_get() -> String {
    std::fs::read_to_string(config::environment_path()).unwrap_or_default()
}

#[tauri::command]
fn environment_save(text: String, app: AppHandle) -> Result<(), String> {
    config::write_environment_info(&text).map_err(|e| e.to_string())?;
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn environment_detect() -> Result<String, String> {
    Ok(envinfo::detect())
}

#[tauri::command]
async fn global_rules_list(state: State<'_, AppState>) -> Result<Vec<GlobalRule>, String> {
    db::models::list_global_rules(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn skills_list(state: State<'_, AppState>) -> Result<Vec<Skill>, String> {
    db::models::list_skills(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn skills_save(
    skills: Vec<Skill>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::models::save_skills(&state.pool, &skills)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("skills:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn global_rules_save(
    rules: Vec<GlobalRule>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::models::replace_global_rules(&state.pool, &rules)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("rules:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn web_search_providers_list(
    state: State<'_, AppState>,
) -> Result<Vec<db::models::WebSearchProviderView>, String> {
    db::models::list_web_search_providers(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn web_search_providers_save(
    providers: Vec<db::models::WebSearchProviderInput>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::models::replace_web_search_providers(&state.pool, &providers)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("web_search:reloaded", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn rules_list(
    scope: String,
    scope_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Rule>, String> {
    db::models::list_rules(&state.pool, &scope, &scope_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rule_add(
    scope: String,
    scope_id: String,
    title: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().to_string();
    db::models::add_rule(
        &state.pool,
        &id,
        &scope,
        &scope_id,
        &title,
        &text,
        0,
        Some("user"),
        now_ms(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rule_update(
    id: String,
    title: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    db::models::update_rule(&state.pool, &id, &title, &text, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rule_toggle(id: String, enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    db::models::toggle_rule(&state.pool, &id, enabled, now_ms())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rule_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    db::models::delete_rule(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

// --- FTS search ---

#[tauri::command]
async fn search_messages(
    query: String,
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<db::models::FtsHit>, String> {
    db::models::fts_search(&state.pool, &query, project_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mcp_list(state: State<'_, AppState>) -> Result<Vec<mcp::McpServerInfo>, String> {
    Ok(state.mcp.list().await)
}

#[tauri::command]
async fn mcp_refresh(state: State<'_, AppState>) -> Result<(), String> {
    state.mcp.recheck_all().await;
    Ok(())
}

#[tauri::command]
async fn mcp_test_def(
    body: McpBody,
    state: State<'_, AppState>,
) -> Result<mcp::TestDefResult, String> {
    Ok(state.mcp.test_def(&body).await)
}

#[tauri::command]
async fn mcp_create(
    input: McpServerInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("mcp server name must not be empty".into());
    }
    if db::mcp_servers::get_by_name(&state.pool, &input.name)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err(format!("mcp server name already exists: {}", input.name));
    }
    let row = db::mcp_servers::create(&state.pool, input)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.sync_server(&row).await;
    if row.is_active_bool() {
        if let Err(e) = state.mcp.connect(&row.id).await {
            warn!("mcp connect '{}' failed: {e}", row.id);
            state.mcp.set_error(&row.id, e.to_string()).await;
        }
    }
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_update(
    id: String,
    input: McpServerInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("mcp server name must not be empty".into());
    }
    if let Some(other) = db::mcp_servers::get_by_name(&state.pool, &input.name)
        .await
        .map_err(|e| e.to_string())?
    {
        if other.id != id {
            return Err(format!("mcp server name already exists: {}", input.name));
        }
    }
    let row = db::mcp_servers::update(&state.pool, &id, input)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.sync_server(&row).await;
    if row.is_active_bool() {
        if let Err(e) = state.mcp.connect(&row.id).await {
            warn!("mcp connect '{}' failed: {e}", row.id);
            state.mcp.set_error(&row.id, e.to_string()).await;
        }
    }
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_delete(id: String, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    state.mcp.delete_server(&id).await;
    db::mcp_servers::delete(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    let _ = mcp::webui::remove_server_cache(&app, &id);
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_reorder(
    ordered_ids: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::mcp_servers::reorder(&state.pool, &ordered_ids)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.set_positions(&ordered_ids).await;
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_set_active(
    id: String,
    is_active: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::mcp_servers::set_active(&state.pool, &id, is_active)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.set_active(&id, is_active).await;
    if is_active {
        if let Err(e) = state.mcp.connect(&id).await {
            warn!("mcp connect '{id}' failed: {e}");
            state.mcp.set_error(&id, e.to_string()).await;
        }
    }
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_webui_save(
    id: String,
    url: String,
    favicon: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("webui url must not be empty".into());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("webui url must start with http:// or https://".into());
    }
    let icon = favicon.as_deref().unwrap_or("").trim();
    db::mcp_servers::set_webui(&state.pool, &id, url.trim(), icon)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.set_webui(&id, url.trim(), icon).await;
    if icon.is_empty() {
        mcp::webui::remove_icon(&app, &id);
    } else {
        let _ = mcp::webui::cache_favicon(&app, &id, icon).await;
    }
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_webui_delete(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::mcp_servers::clear_webui(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    state.mcp.set_webui(&id, "", "").await;
    let _ = mcp::webui::remove_server_cache(&app, &id);
    let _ = app.emit("mcp:changed", ());
    Ok(())
}

#[tauri::command]
async fn mcp_webui_list(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<mcp::webui::McpWebUiEntry>, String> {
    mcp::webui::list_entries(&app, &state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mcp_webui_open(
    id: String,
    theme: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let row = db::mcp_servers::get(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown mcp server: {id}"))?;
    if row.webui_url.is_empty() {
        return Err("no webui configured".into());
    }
    mcp::webui::open_window(&app, &row.id, &row.title, &row.webui_url, theme.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mcp_webui_detect_favicon(
    site_url: String,
    current: Option<String>,
) -> Result<mcp::webui::DetectResult, String> {
    Ok(mcp::webui::detect_favicon(site_url, current).await)
}

#[tauri::command]
async fn pending_list(
    chat_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<pending::ChangeInfo>, String> {
    Ok(state.pending.changes(&chat_id).await)
}

#[tauri::command]
async fn pending_approve(chat_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.pending.approve(&chat_id);
    Ok(())
}

#[tauri::command]
async fn pending_reject(chat_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.pending.reject(&chat_id).await;
    Ok(())
}

#[derive(serde::Serialize)]
struct ToolInfoOut {
    name: String,
    category: String,
    description: String,
    enabled: bool,
}

#[tauri::command]
fn tools_list(state: State<AppState>) -> Vec<ToolInfoOut> {
    let disabled = state.config.read().unwrap().defaults.disabled_tools.clone();
    tools::builtin_all_specs()
        .into_iter()
        .map(|(name, category, description)| {
            let enabled = !disabled.contains(&name);
            ToolInfoOut {
                name,
                category,
                description,
                enabled,
            }
        })
        .collect()
}

#[tauri::command]
fn tools_set_enabled(
    name: String,
    enabled: bool,
    state: State<AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let mut disabled = state.config.read().unwrap().defaults.disabled_tools.clone();
    if enabled {
        disabled.retain(|n| n != &name);
    } else if !disabled.contains(&name) {
        disabled.push(name);
    }
    config::write_disabled_tools(&disabled).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.disabled_tools = disabled;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn pty_input(session_id: String, data: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .pty
        .write_input(&session_id, data.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn chat_cancel(chat_id: String, state: State<'_, AppState>, app: AppHandle) {
    chat::cancel(app, state.active.clone(), chat_id);
}

// --- providers / models (DB-backed, multi-provider) ---

/// Live provider status for the Settings indicator ("active" | "inactive" |
/// "unreachable" with an error detail).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    pub state: String,
    pub detail: Option<String>,
}

#[tauri::command]
async fn providers_list(state: State<'_, AppState>) -> Result<Vec<ProviderRow>, String> {
    db::providers::list_providers(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn providers_create(
    input: ProviderInput,
    api_key: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProviderRow, String> {
    let row = db::providers::create_provider(&state.pool, input)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(k) = api_key.as_deref().filter(|s| !s.is_empty()) {
        secrets::set_api_key(&row.api_key_ref, k).map_err(|e| e.to_string())?;
    }
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
async fn providers_update(
    id: String,
    input: ProviderInput,
    api_key: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProviderRow, String> {
    let row = db::providers::update_provider(&state.pool, &id, input)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(k) = api_key.as_deref().filter(|s| !s.is_empty()) {
        secrets::set_api_key(&row.api_key_ref, k).map_err(|e| e.to_string())?;
    }
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
async fn providers_delete(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if let Some(row) = db::providers::get_provider(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
    {
        if !row.api_key_ref.is_empty() {
            let _ = secrets::delete_api_key(&row.api_key_ref);
        }
    }
    db::providers::delete_provider(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?;
    // Defaults may reference the deleted provider — both stores change.
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn providers_reorder(
    ordered_ids: Vec<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::providers::reorder_providers(&state.pool, &ordered_ids)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn providers_set_active(
    id: String,
    is_active: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::providers::set_provider_active(&state.pool, &id, is_active)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn provider_models_list(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<db::providers::ProviderModel>, String> {
    db::providers::list_provider_models(&state.pool, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// Live fetch of the provider's model list from its endpoint ("Test and fetch").
#[tauri::command]
async fn provider_models_fetch(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelInfo>, String> {
    let row = db::providers::get_provider(&state.pool, &provider_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("provider not found: {provider_id}"))?;
    let pcfg = row.to_config_provider();
    let provider: Box<dyn DynProvider> = providers::build(&pcfg)
        .ok_or_else(|| format!("unsupported provider kind: {}", pcfg.kind))?;
    provider.list_models().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn provider_models_save(
    provider_id: String,
    models: Vec<ProviderModelInput>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    db::providers::replace_provider_models(&state.pool, &provider_id, &models)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("providers:changed", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Reachability status of a provider endpoint for the Settings list indicator.
#[tauri::command]
async fn provider_status(id: String, state: State<'_, AppState>) -> Result<ProviderStatus, String> {
    let row = db::providers::get_provider(&state.pool, &id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("provider not found: {id}"))?;
    if !row.is_active {
        return Ok(ProviderStatus {
            state: "inactive".into(),
            detail: None,
        });
    }
    let pcfg = row.to_config_provider();
    match providers::build(&pcfg) {
        None => Ok(ProviderStatus {
            state: "unreachable".into(),
            detail: Some(format!("unsupported provider kind: {}", pcfg.kind)),
        }),
        Some(provider) => match provider.list_models().await {
            Ok(_) => Ok(ProviderStatus {
                state: "active".into(),
                detail: None,
            }),
            Err(e) => Ok(ProviderStatus {
                state: "unreachable".into(),
                detail: Some(e.detail()),
            }),
        },
    }
}

#[tauri::command]
async fn providers_active_models(
    state: State<'_, AppState>,
) -> Result<Vec<db::providers::ModelOption>, String> {
    db::providers::list_active_model_options(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn providers_all_models(
    state: State<'_, AppState>,
) -> Result<Vec<db::providers::ModelOption>, String> {
    db::providers::list_all_model_options(&state.pool)
        .await
        .map_err(|e| e.to_string())
}

// --- keyring ---

/// Live model status for the Settings model indicator: asks the provider
/// endpoint whether the model is loaded/reachable (providers::model_status).
#[tauri::command]
async fn provider_model_status(
    provider_id: String,
    model_id: String,
    state: State<'_, AppState>,
) -> Result<providers::ModelStatus, String> {
    if model_id.trim().is_empty() {
        return Ok(providers::ModelStatus {
            state: "unknown".into(),
            detail: serde_json::json!({ "reason": "no model selected" }),
        });
    }
    let pcfg = match db::providers::get_provider(&state.pool, &provider_id)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(row) => row.to_config_provider(),
        None => {
            return Ok(providers::ModelStatus::error(serde_json::json!({
                "error": format!("provider not found: {provider_id}"),
            })));
        }
    };
    let Some(m) = db::providers::get_model(&state.pool, &model_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(providers::ModelStatus::error(serde_json::json!({
            "error": "provider/model not found",
        })));
    };
    Ok(providers::model_status(&pcfg, &m.name).await)
}

#[tauri::command]
fn set_api_key(ref_id: String, key: String) -> Result<(), String> {
    secrets::set_api_key(&ref_id, &key).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_api_key(ref_id: String) -> Result<(), String> {
    secrets::delete_api_key(&ref_id).map_err(|e| e.to_string())
}

// --- provider/model config editing (Settings → Providers/Models) ---

#[tauri::command]
async fn set_defaults_model(
    field: String,
    provider: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if !matches!(
        field.as_str(),
        "main_model" | "secondary_model" | "embedding_model"
    ) {
        return Err(format!("invalid defaults model field: {field}"));
    }
    let mr = match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => Some(ModelRef {
            provider: p,
            model: m,
        }),
        _ => None,
    };
    // Changing the embedding model invalidates existing vectors (dimension
    // mismatch); clear all embeddings so retrieval doesn't silently break.
    let embedding_changed = field == "embedding_model" && {
        let r = state.config.read().unwrap();
        r.defaults.embedding_model.as_ref() != mr.as_ref()
    };
    config::write_defaults_model_ref(&field, mr.as_ref()).map_err(|e| e.to_string())?;
    {
        let mut w = state.config.write().unwrap();
        match field.as_str() {
            "main_model" => w.defaults.main_model = mr,
            "secondary_model" => w.defaults.secondary_model = mr,
            "embedding_model" => w.defaults.embedding_model = mr,
            _ => unreachable!(),
        }
    }
    if embedding_changed {
        if let Err(e) = rag::clear_all(&state.pool).await {
            tracing::warn!("rag clear all on embedding model change failed: {e}");
        }
        let _ = app.emit("rag:cleared", ());
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_mode(mode: String, state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    if !matches!(mode.as_str(), "minimal" | "plan" | "write") {
        return Err("invalid mode".into());
    }
    config::write_mode(&mode).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.mode = mode;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_command_toggle(
    value: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if !matches!(value.as_str(), "manual" | "auto") {
        return Err("invalid command_toggle".into());
    }
    config::write_defaults_field("command_toggle", &value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.command_toggle = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_edit_toggle(
    value: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if !matches!(value.as_str(), "ask" | "auto") {
        return Err("invalid edit_toggle".into());
    }
    config::write_defaults_field("edit_toggle", &value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.edit_toggle = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_auto_collapse_context_pct(
    value: u32,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if value > 100 {
        return Err("value must be 0..=100".into());
    }
    config::write_auto_collapse_context_pct(value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.auto_collapse_context_pct = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_auto_pull_changes(
    value: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    config::write_auto_pull_changes(value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.auto_pull_changes = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_delete_to_trash(
    value: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    config::write_delete_to_trash(value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.delete_to_trash = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn set_add_environment_info(
    value: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    config::write_add_environment_info(value).map_err(|e| e.to_string())?;
    if let Ok(mut w) = state.config.write() {
        w.defaults.add_environment_info = value;
    }
    app.emit("config:reloaded", ()).map_err(|e| e.to_string())?;
    Ok(())
}

/// On-request view of tracked project file changes (docs/08).
#[tauri::command]
async fn project_changed_files(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<projects::ChangedFileView>, String> {
    let project = db::models::get_project(&state.pool, &project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let snap = state.changes.snapshot(&project_id);
    Ok(projects::changed_file_views(&state.pool, &project, snap).await)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_linux_decorations();
    let (log_path, log_guard, log_levels) = logger::init();

    // Crash intercept: log panics to the log file (docs/19).
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("panic: {info}\n{backtrace}");
    }));

    let loaded = config::load().unwrap_or_else(|e| {
        info!("config load failed, using defaults: {e}");
        Config::default()
    });
    info!(
        "config loaded: mode={}, theme={}",
        loaded.defaults.mode, loaded.appearance.theme
    );

    let config_lock = Arc::new(RwLock::new(loaded));
    net::init(&config_lock.read().unwrap().network);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let pool = tauri::async_runtime::block_on(async { db::init().await })?;
            crate::tools::set_db_pool(pool.clone());
            let active: ActiveTurns = Arc::new(Mutex::new(HashMap::new()));

            // Cleanup attachments never linked to a message (24h grace).
            let pool_for_gc = pool.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = db::attachments::remove_orphans(&pool_for_gc, 86_400_000).await {
                    tracing::warn!("attachment orphan cleanup failed: {e}");
                }
                if let Err(e) = rag::gc_orphans(&pool_for_gc).await {
                    tracing::warn!("rag orphan cleanup failed: {e}");
                }
            });

            let mcp = McpManager::new();
            let mcp_bg = mcp.clone();
            let pool_for_mcp = pool.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mcp_bg.reload(&pool_for_mcp).await {
                    tracing::warn!("mcp reload failed: {e}");
                }
                mcp_bg.connect_all().await;
            });

            let watcher = ProjectWatcher::new();
            let changes = projects::ChangeTracker::new();
            let watcher_c = watcher.clone();
            let changes_c = changes.clone();
            let pool_c = pool.clone();
            let app_c = handle.clone();

            handle.manage(AppState {
                config: config_lock.clone(),
                pool,
                log_path: log_path.clone(),
                _log_guard: log_guard,
                log_levels,
                active,
                approvals: ApprovalRegistry::new(),
                ask: AskRegistry::new(),
                task_nudge: chat::TaskNudge::new(),
                watcher,
                changes,
                runner: AgentRunner::new(config_lock.read().unwrap().agents_limits.max_concurrent),
                mcp,
                pending: PendingManager::new(),
                pty: PtyManager::new(),
                pending_settings_tab: Mutex::new(None),
            });

            // Restart watchers for all existing projects on startup.
            tauri::async_runtime::spawn(async move {
                if let Ok(projs) = db::models::list_projects(&pool_c).await {
                    for p in projs {
                        watcher_c.restart_for_project(
                            app_c.clone(),
                            pool_c.clone(),
                            changes_c.clone(),
                            p.id,
                        );
                    }
                }
            });

            spawn_config_watcher(handle.clone(), config_lock.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config_get,
            app_paths,
            logs_read,
            logs_clear,
            set_log_level,
            set_theme,
            set_network,
            network_test,
            network_has_password,
            clear_proxy_password,
            chat_create,
            chat_create_in_project,
            chat_list,
            project_chats,
            chat_messages,
            chat_rename,
            chat_delete,
            chat_info,
            activity_daily,
            activity_day_detail,
            chat_set_pinned,
            chat_reorder,
            chat_set_model,
            chat_set_project,
            chat_set_system_prompt,
            chat_send,
            attachment_add,
            attachment_remove,
            attachments_for_chat,
            attachments_for_message,
            attachment_read_data_url,
            chat_regenerate,
            chat_edit_message,
            chat_export_markdown,
            write_text_file,
            chat_branches,
            chat_roots,
            chat_set_active_leaf,
            chat_sessions_list,
            chat_compact,
            chat_context_window,
            chat_cancel,
            approve_request,
            ask_user_reply,
            task_list,
            task_clear,
            project_task_list,
            project_task_create,
            project_task_update,
            project_task_delete,
            project_task_move,
            project_task_run,
            prompt_list,
            prompt_list_favorites,
            prompt_create,
            prompt_update,
            prompt_delete,
            prompt_set_favorite,
            prompt_move,
            prompt_run,
            agent_list,
            agent_presets,
            agent_detect,
            agent_create,
            agent_update,
            agent_delete,
            agent_reorder,
            agent_set_active,
            agent_test,
            agent_test_input,
            agent_runs_list,
            agent_run_cancel,
            agent_run_approve,
            agent_run_reject,
            agent_run_diff,
            project_create,
            project_list,
            project_get,
            project_update,
            project_delete,
            project_set_pinned,
            project_reorder,
            project_paths_list,
            project_path_add,
            project_path_delete,
            chat_paths_list,
            chat_path_add,
            chat_path_delete,
            rules_list,
            rule_add,
            rule_update,
            rule_toggle,
            rule_delete,
            set_system_prompt,
            environment_get,
            environment_detect,
            environment_save,
            global_rules_list,
            global_rules_save,
            web_search_providers_list,
            web_search_providers_save,
            skills_list,
            skills_save,
            search_messages,
            mcp_list,
            mcp_refresh,
            mcp_test_def,
            mcp_create,
            mcp_update,
            mcp_delete,
            mcp_reorder,
            mcp_set_active,
            mcp_webui_save,
            mcp_webui_delete,
            mcp_webui_list,
            mcp_webui_open,
            mcp_webui_detect_favicon,
            pending_list,
            pending_approve,
            pending_reject,
            tools_list,
            tools_set_enabled,
            pty_input,
            providers_list,
            providers_create,
            providers_update,
            providers_delete,
            providers_reorder,
            providers_set_active,
            provider_models_list,
            provider_models_fetch,
            provider_models_save,
            provider_status,
            provider_model_status,
            providers_active_models,
            providers_all_models,
            set_defaults_model,
            rag_reindex_project,
            rag_clear_project,
            rag_status,
            rag_clear_all,
            set_mode,
            set_command_toggle,
            set_edit_toggle,
            set_add_environment_info,
            set_auto_collapse_context_pct,
            set_auto_pull_changes,
            set_delete_to_trash,
            project_changed_files,
            set_api_key,
            delete_api_key,
            open_settings_window,
            take_settings_tab,
            update_install_supported,
            update_changelog
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            use tauri_plugin_window_state::AppHandleExt;
            let _ = app_handle.save_window_state(tauri_plugin_window_state::StateFlags::all());
        }
    });
}

/// On Linux, Tauri windows are GTK windows. On KDE (Wayland) GTK draws its own
/// client-side title bar, which looks foreign on a Qt desktop. Force server-side
/// decorations by letting KWin draw the native Breeze title bar: switch GDK to
/// X11/XWayland and disable GTK CSD. On GNOME/other DEs we keep native behavior.
fn configure_linux_decorations() {
    if cfg!(target_os = "linux") {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if desktop.to_lowercase().contains("kde") {
            std::env::set_var("GDK_BACKEND", "x11");
            std::env::set_var("GTK_CSD", "0");
            tracing::info!("KDE detected: using KWin server-side window decorations");
        }
    }
}
