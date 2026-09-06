use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use similar::{ChangeTag, TextDiff};

/// One snapshot of a path before a write tool mutated it (docs/12: snapshot+revert).
#[derive(Clone, Debug)]
struct Snapshot {
    original: Option<Vec<u8>>,
    original_mode: Option<u32>,
    existed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangeInfo {
    pub path: String,
    pub kind: ChangeKind,
    pub diff: String,
}

#[derive(Clone, Default)]
struct ChatPending {
    snapshots: HashMap<String, Snapshot>,
    order: Vec<String>,
}

/// Per-chat pending file changes with snapshot+revert semantics (docs/12).
/// In Write+Ask, write-tools snapshot the original, apply to disk (so the model
/// can read its own changes), and record here; Approve keeps, Reject reverts.
#[derive(Clone, Default)]
pub struct PendingManager {
    chats: Arc<Mutex<HashMap<String, ChatPending>>>,
}

impl PendingManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot a path before it is mutated. Idempotent per path per chat.
    /// File I/O happens outside the lock (std Mutex is not held across await).
    pub async fn snapshot(&self, chat_id: &str, path: &str) {
        {
            let chats = self.chats.lock().unwrap();
            if let Some(p) = chats.get(chat_id) {
                if p.snapshots.contains_key(path) {
                    return;
                }
            }
        }
        let p = PathBuf::from(path);
        let meta = tokio::fs::metadata(&p).await.ok();
        let existed = meta.is_some();
        let original = if existed {
            tokio::fs::read(&p).await.ok()
        } else {
            None
        };
        let original_mode = metadata_mode(&meta);

        let mut chats = self.chats.lock().unwrap();
        let pending = chats.entry(chat_id.to_string()).or_default();
        if pending.snapshots.contains_key(path) {
            return;
        }
        pending.snapshots.insert(
            path.to_string(),
            Snapshot {
                original,
                original_mode,
                existed,
            },
        );
        pending.order.push(path.to_string());
    }

    /// Build the list of changes with a line diff (current vs snapshot).
    pub async fn changes(&self, chat_id: &str) -> Vec<ChangeInfo> {
        let (order, snaps) = {
            let chats = self.chats.lock().unwrap();
            let Some(pending) = chats.get(chat_id) else {
                return Vec::new();
            };
            (pending.order.clone(), pending.snapshots.clone())
        };
        let mut out = Vec::new();
        for path in &order {
            let Some(snap) = snaps.get(path) else {
                continue;
            };
            let current = tokio::fs::read(path).await.ok();
            let kind = match (snap.existed, current.is_some()) {
                (false, true) => ChangeKind::Created,
                (true, false) => ChangeKind::Deleted,
                (true, true) | (false, false) => ChangeKind::Modified,
            };
            let old = snap
                .original
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).into_owned());
            let new = current
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).into_owned());
            let diff = diff_text(old.as_deref(), new.as_deref());
            out.push(ChangeInfo {
                path: path.clone(),
                kind,
                diff,
            });
        }
        out
    }

    /// Approve: drop snapshots, keep changes on disk.
    pub fn approve(&self, chat_id: &str) {
        self.chats.lock().unwrap().remove(chat_id);
    }

    /// Reject: revert every snapshotted path to its original state.
    pub async fn reject(&self, chat_id: &str) {
        let (order, snaps) = {
            let mut chats = self.chats.lock().unwrap();
            let Some(pending) = chats.get_mut(chat_id) else {
                return;
            };
            (
                std::mem::take(&mut pending.order),
                std::mem::take(&mut pending.snapshots),
            )
        };
        for path in order.iter().rev() {
            if let Some(snap) = snaps.get(path) {
                let _ = revert_path(path, snap).await;
            }
        }
        self.chats.lock().unwrap().remove(chat_id);
    }
}

fn metadata_mode(meta: &Option<std::fs::Metadata>) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.as_ref().map(|m| m.mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

async fn revert_path(path: &str, snap: &Snapshot) -> std::io::Result<()> {
    if snap.existed {
        if let Some(bytes) = &snap.original {
            tokio::fs::write(path, bytes).await?;
        }
        #[cfg(unix)]
        if let Some(mode) = snap.original_mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await;
        }
    } else {
        let meta = tokio::fs::metadata(path).await;
        if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            let _ = tokio::fs::remove_dir_all(path).await;
        } else {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    Ok(())
}

fn diff_text(old: Option<&str>, new: Option<&str>) -> String {
    let old = old.unwrap_or("");
    let new = new.unwrap_or("");
    if old == new {
        return String::new();
    }
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            ChangeTag::Insert => '+',
            ChangeTag::Delete => '-',
            ChangeTag::Equal => ' ',
        };
        out.push(prefix);
        out.push_str(change.value());
    }
    out
}
