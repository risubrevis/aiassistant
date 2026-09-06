use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

/// Pending approval requests: request_id -> sender to resume the turn task.
/// The turn task awaits the receiver; the UI resolves it via `approve_request`.
#[derive(Clone, Default)]
pub struct ApprovalRegistry {
    senders: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl ApprovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pending approval and return the receiver the turn waits on.
    pub fn register(&self) -> (String, oneshot::Receiver<bool>) {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.senders.lock().unwrap().insert(id.clone(), tx);
        (id, rx)
    }

    /// Resolve a pending approval (true = approve, false = reject). Returns
    /// whether a pending request with that id existed.
    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        if let Some(tx) = self.senders.lock().unwrap().remove(id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub chat_id: String,
    pub message_id: String,
    pub block_id: String,
    pub tool_name: String,
    pub summary: String,
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escape: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_mode: Option<String>,
}

pub fn emit_request(app: &AppHandle, req: ApprovalRequest) {
    let _ = app.emit("chat:approval_request", req);
}
