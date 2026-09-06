use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use super::contract;
use super::parser;
use super::{AgentBridge, AgentEvent, AgentResult, AgentTask, DiffEntry};
use crate::config::AgentContract;

/// Subprocess bridge: builds the command from the contract, spawns the agent
/// process, streams parsed events, and returns the final result (docs/07).
pub struct SubprocessBridge {
    contract: AgentContract,
    children: Arc<Mutex<HashMap<String, tokio::process::Child>>>,
}

impl SubprocessBridge {
    pub fn new(contract: AgentContract) -> Self {
        Self {
            contract,
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl AgentBridge for SubprocessBridge {
    fn kind(&self) -> &str {
        &self.contract.kind
    }

    async fn run(
        &self,
        run_id: &str,
        task: AgentTask,
        sink: mpsc::Sender<AgentEvent>,
    ) -> AgentResult {
        let argv = contract::build_command(&self.contract, &task);
        if argv.is_empty() || self.contract.command.is_empty() {
            return AgentResult::error("agent contract has no command configured");
        }

        let cwd = if task.cwd.is_empty() {
            self.contract.cwd.clone()
        } else {
            task.cwd.clone()
        };

        let mut cmd = tokio::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        if !cwd.is_empty() {
            cmd.current_dir(&cwd);
        }
        for (k, v) in &self.contract.env {
            cmd.env(k, v);
        }
        for (k, v) in &task.env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return AgentResult::error(format!("failed to spawn '{}': {e}", argv[0])),
        };

        // Take stdio handles before moving the child into the registry.
        let stdin_opt = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        self.children
            .lock()
            .unwrap()
            .insert(run_id.to_string(), child);

        // Feed the prompt per prompt_mode (docs/07).
        let mode = self.contract.prompt_mode.clone();
        if let Some(stdin) = stdin_opt {
            let mut stdin = stdin;
            let prompt = task.prompt.clone();
            let write_task = async move {
                match mode.as_str() {
                    "stdin" => {
                        let _ = stdin.write_all(prompt.as_bytes()).await;
                        let _ = stdin.write_all(b"\n").await;
                    }
                    "stdin_json" => {
                        let msg = serde_json::json!({
                            "type": "user",
                            "message": prompt,
                        });
                        let mut line = serde_json::to_string(&msg).unwrap_or_default();
                        line.push('\n');
                        let _ = stdin.write_all(line.as_bytes()).await;
                    }
                    _ => {} // "arg": prompt is already in argv
                }
                let _ = stdin.shutdown().await;
            };
            tauri::async_runtime::spawn(write_task);
        }

        let stdout = stdout.unwrap();
        let stderr = stderr.unwrap();
        let format = self.contract.output_format.clone();
        let schema = self.contract.event_schema.clone();

        let mut text_acc = String::new();
        let mut diffs: Vec<DiffEntry> = Vec::new();
        let mut session: Option<String> = None;

        let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
        let (err_tx, mut err_rx) = mpsc::channel::<String>(64);

        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if out_tx.send(line).await.is_err() {
                    break;
                }
            }
        });
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if err_tx.send(line).await.is_err() {
                    break;
                }
            }
        });

        let deadline = tokio::time::Duration::from_millis(task.timeout_ms.max(1000));
        let result_status = tokio::select! {
            _ = async {
                loop {
                    tokio::select! {
                        Some(line) = out_rx.recv() => {
                            if let Some(ev) = parser::parse_line(&format, &line, schema.as_ref()) {
                                match &ev {
                                    AgentEvent::Text { text } => text_acc.push_str(text),
                                    AgentEvent::Diff { path, patch } => diffs.push(DiffEntry {
                                        path: path.clone(),
                                        patch: patch.clone(),
                                    }),
                                    _ => {}
                                }
                                if let Some(sid) = parser::extract_session(&format, &line) {
                                    session = Some(sid);
                                }
                                let _ = sink.send(ev).await;
                            }
                        }
                        Some(line) = err_rx.recv() => {
                            // Non-fatal: surface stderr as progress events.
                            let _ = sink.send(AgentEvent::Progress { text: line }).await;
                        }
                        else => break,
                    }
                }
            } => String::from("done"),
            _ = tokio::time::sleep(deadline) => String::from("timeout"),
        };

        // Terminate the process if still running.
        let mut status_code: Option<i32> = None;
        let taken = self.children.lock().unwrap().remove(run_id);
        if let Some(mut child) = taken {
            if result_status == "timeout" {
                let _ = child.kill().await;
            }
            if let Ok(st) = child.wait().await {
                status_code = st.code();
            }
        }

        if result_status == "timeout" {
            return AgentResult {
                text: text_acc,
                diffs,
                status: "timeout".into(),
                session,
            };
        }
        if text_acc.is_empty() && !diffs.is_empty() {
            text_acc = format!(
                "Agent produced {} diff(s) without a text summary.",
                diffs.len()
            );
        }
        if let Some(code) = status_code {
            if code != 0 && text_acc.is_empty() {
                return AgentResult {
                    text: format!("agent exited with code {code}"),
                    diffs,
                    status: "error".into(),
                    session,
                };
            }
        }
        AgentResult {
            text: text_acc,
            diffs,
            status: result_status,
            session,
        }
    }

    async fn cancel(&self, run_id: &str) {
        let child = self.children.lock().unwrap().remove(run_id);
        if let Some(mut child) = child {
            let _ = child.kill().await;
        }
    }
}
