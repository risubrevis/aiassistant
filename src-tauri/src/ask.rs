use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

/// Pending ask_user questions: request_id -> sender that resumes the turn with
/// the user's answer. The turn task awaits the receiver; the UI resolves it via
/// the `ask_user_reply` command (docs/17 ask_user).
#[derive(Clone, Default)]
pub struct AskRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
}

impl AskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self) -> (String, oneshot::Receiver<String>) {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.senders.lock().unwrap().insert(id.clone(), tx);
        (id, rx)
    }

    pub fn resolve(&self, id: &str, answer: String) -> bool {
        if let Some(tx) = self.senders.lock().unwrap().remove(id) {
            let _ = tx.send(answer);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AskRequest {
    pub request_id: String,
    pub chat_id: String,
    pub message_id: String,
    pub block_id: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default)]
    pub context: Option<String>,
}

pub fn emit_ask(app: &AppHandle, req: AskRequest) {
    let _ = app.emit("chat:ask_user", req);
}
