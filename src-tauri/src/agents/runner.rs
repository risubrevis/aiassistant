use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::task_extract::TaskExtractor;
use super::worktree::{self, WorktreeDiff, WorktreeHandle};
use super::{bridge_for, AgentBridge, AgentEvent, AgentResult, AgentTask};
use crate::config::{AgentContract, Config};
use crate::db::models::{self, AgentRun};
use crate::db::tasks::{self, TaskInput};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Persist a run's extracted task list and push the chat's full list to the UI.
async fn persist_run_tasks(
    pool: &SqlitePool,
    run_id: &str,
    chat_id: &str,
    message_id: Option<&str>,
    items: Vec<TaskInput>,
    app: &AppHandle,
) -> anyhow::Result<()> {
    tasks::replace_agent_tasks(pool, chat_id, run_id, message_id.unwrap_or(""), items).await?;
    let all = tasks::list_tasks(pool, chat_id).await?;
    let _ = app.emit(
        "chat:tasks_update",
        crate::chat::TasksUpdatePayload {
            chat_id: chat_id.to_string(),
            tasks: all,
        },
    );
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressPayload {
    pub run_id: String,
    pub chat_id: String,
    pub parent_id: Option<String>,
    pub agent_id: String,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusPayload {
    pub run_id: String,
    pub chat_id: String,
    pub parent_id: Option<String>,
    pub agent_id: String,
    pub status: String,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchRunSummary {
    pub run_id: String,
    pub subtask_index: usize,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchCompletePayload {
    pub parent_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub run_ids: Vec<String>,
    pub any_error: bool,
    pub summaries: Vec<BatchRunSummary>,
}

/// Parameters for spawning one agent run (docs/16).
pub struct SpawnParams {
    pub chat_id: String,
    pub parent_tool_call_id: Option<String>,
    pub parent_id: Option<String>,
    pub agent_id: String,
    pub prompt: String,
    pub subtask_index: usize,
    pub cwd: String,
    /// Our chat mode: "plan" | "write" (translated into agent mode_flags).
    pub mode: String,
    pub files: Vec<String>,
    /// Create an isolated git worktree (parallel Write batches).
    pub isolate: bool,
    /// Assistant message owning the delegation (used to attribute agent tasks).
    pub message_id: Option<String>,
}

struct RunHandle {
    bridge: Arc<dyn AgentBridge>,
    #[allow(dead_code)]
    parent_id: Option<String>,
}

/// Agent runner: a global process pool with `max_concurrent` slots, DB
/// bookkeeping per run, and event streaming to the UI (docs/16).
#[derive(Clone)]
pub struct AgentRunner {
    semaphore: Arc<tokio::sync::Semaphore>,
    handles: Arc<Mutex<HashMap<String, RunHandle>>>,
    worktrees: Arc<Mutex<HashMap<String, WorktreeHandle>>>,
    task_extractor: Arc<TaskExtractor>,
}

impl AgentRunner {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent.max(1) as usize)),
            handles: Arc::new(Mutex::new(HashMap::new())),
            worktrees: Arc::new(Mutex::new(HashMap::new())),
            task_extractor: Arc::new(super::task_extract::TaskExtractor::new()),
        }
    }

    /// Spawn a single agent run with an explicitly resolved contract. Returns
    /// (run_id, JoinHandle to await the result). Runs queue behind the global
    /// semaphore; events stream via `agent:progress` (docs/16).
    pub async fn spawn(
        &self,
        app: AppHandle,
        pool: SqlitePool,
        config: &Config,
        contract: AgentContract,
        p: SpawnParams,
    ) -> anyhow::Result<(String, tauri::async_runtime::JoinHandle<AgentResult>)> {
        let run_id = Uuid::new_v4().to_string();

        // Worktree isolation for parallel Write runs (docs/16): git repos only.
        let mut task_cwd = p.cwd.clone();
        let mut worktree_branch: Option<String> = None;
        if p.isolate && p.mode != "plan" && worktree::is_git_repo(&p.cwd) {
            match worktree::create(&p.cwd, &config.worktree.base_ref, &run_id).await {
                Ok(handle) => {
                    if config.worktree.symlink_gitignored {
                        let _ = worktree::symlink_gitignored(&handle.path, &p.cwd).await;
                    }
                    task_cwd = handle.path.clone();
                    worktree_branch = Some(handle.branch.clone());
                    self.worktrees
                        .lock()
                        .unwrap()
                        .insert(run_id.clone(), handle);
                }
                Err(e) => {
                    // Not fatal: fall back to the shared cwd.
                    let _ = app.emit(
                        "agent:progress",
                        ProgressPayload {
                            run_id: run_id.clone(),
                            chat_id: p.chat_id.clone(),
                            parent_id: p.parent_id.clone(),
                            agent_id: p.agent_id.clone(),
                            event: AgentEvent::Progress {
                                text: format!("worktree isolation unavailable: {e}"),
                            },
                        },
                    );
                }
            }
        }

        let run = AgentRun {
            id: run_id.clone(),
            chat_id: p.chat_id.clone(),
            parent_tool_call_id: p.parent_tool_call_id.clone(),
            parent_id: p.parent_id.clone(),
            agent_connection_id: p.agent_id.clone(),
            subtask_index: p.subtask_index as i64,
            subtask_prompt: p.prompt.clone(),
            cwd: task_cwd.clone(),
            worktree_branch: worktree_branch.clone(),
            agent_session_id: None,
            status: "queued".into(),
            result_summary: None,
            started_at: None,
            ended_at: None,
            created_at: now_ms(),
        };
        models::insert_agent_run(&pool, &run).await?;

        let mut task = AgentTask::new(p.prompt.clone(), task_cwd);
        task.files = p.files.clone();
        task.mode = p.mode.clone();
        task.timeout_ms = contract.timeout_ms;
        task.max_turns = contract.max_turns;

        let bridge = bridge_for(&contract);
        let bridge_run = bridge.clone();
        let parent_id = p.parent_id.clone();
        let parent_for_cancel = p.parent_id.clone();
        let chat_id = p.chat_id.clone();
        let agent_id = p.agent_id.clone();
        let run_id_fwd = run_id.clone();
        let chat_fwd = p.chat_id.clone();
        let parent_fwd = p.parent_id.clone();
        let agent_fwd = p.agent_id.clone();
        let app_fwd = app.clone();
        let sem = self.semaphore.clone();
        let pool_c = pool.clone();
        let app_c = app.clone();
        let run_id_c = run_id.clone();
        let extractor_fwd = self.task_extractor.clone();
        let extractor_c = self.task_extractor.clone();
        let pool_fwd = pool.clone();
        let message_id_fwd = p.message_id.clone();
        let message_id_c = p.message_id.clone();

        let handle = tauri::async_runtime::spawn(async move {
            // Queue until a global slot frees up (docs/16).
            let _permit = sem.acquire().await;
            models::update_agent_run_status(
                &pool_c,
                &run_id_c,
                "running",
                None,
                None,
                None,
                Some(now_ms()),
                None,
            )
            .await
            .ok();

            let (tx, mut rx) = mpsc::channel::<AgentEvent>(64);
            let _fwd = tauri::async_runtime::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    if let Some(items) = extractor_fwd.ingest(&run_id_fwd, &ev) {
                        if let Err(e) = persist_run_tasks(
                            &pool_fwd,
                            &run_id_fwd,
                            &chat_fwd,
                            message_id_fwd.as_deref(),
                            items,
                            &app_fwd,
                        )
                        .await
                        {
                            tracing::warn!("agent task extraction failed: {e:#}");
                        }
                    }
                    let _ = app_fwd.emit(
                        "agent:progress",
                        ProgressPayload {
                            run_id: run_id_fwd.clone(),
                            chat_id: chat_fwd.clone(),
                            parent_id: parent_fwd.clone(),
                            agent_id: agent_fwd.clone(),
                            event: ev,
                        },
                    );
                }
            });

            let result = bridge_run.run(&run_id_c, task, tx).await;

            let summary: String = result.text.chars().take(4000).collect();
            models::update_agent_run_status(
                &pool_c,
                &run_id_c,
                &result.status,
                Some(&summary),
                result.session.as_deref(),
                None,
                None,
                Some(now_ms()),
            )
            .await
            .ok();
            // Final emit for agents whose task list only completes at EOF
            // (raw_text markdown checklists).
            if let Some(items) = extractor_c.finalize(&run_id_c) {
                if !items.is_empty() {
                    if let Err(e) = persist_run_tasks(
                        &pool_c,
                        &run_id_c,
                        &chat_id,
                        message_id_c.as_deref(),
                        items,
                        &app_c,
                    )
                    .await
                    {
                        tracing::warn!("agent task finalization failed: {e:#}");
                    }
                }
            }
            let _ = app_c.emit(
                "agent:status",
                StatusPayload {
                    run_id: run_id_c,
                    chat_id,
                    parent_id,
                    agent_id,
                    status: result.status.clone(),
                    result_summary: Some(summary),
                },
            );
            result
        });

        self.handles.lock().unwrap().insert(
            run_id.clone(),
            RunHandle {
                bridge,
                parent_id: parent_for_cancel,
            },
        );

        Ok((run_id, handle))
    }

    /// Await every run of a background batch and emit `agent:batch_complete`
    /// when all finish (docs/16 background detach).
    pub fn spawn_batch_supervisor(
        &self,
        app: AppHandle,
        parent_id: String,
        chat_id: String,
        agent_id: String,
        handles: Vec<(String, usize, tauri::async_runtime::JoinHandle<AgentResult>)>,
    ) {
        tauri::async_runtime::spawn(async move {
            let mut run_ids = Vec::new();
            let mut summaries = Vec::new();
            let mut any_error = false;
            for (run_id, idx, h) in handles {
                let result = h
                    .await
                    .unwrap_or_else(|_| AgentResult::error("run task panicked"));
                let status = result.status.clone();
                if matches!(status.as_str(), "error" | "cancelled" | "timeout") {
                    any_error = true;
                }
                let summary: String = result.text.chars().take(4000).collect();
                run_ids.push(run_id.clone());
                summaries.push(BatchRunSummary {
                    run_id,
                    subtask_index: idx,
                    status,
                    summary,
                });
            }
            let _ = app.emit(
                "agent:batch_complete",
                BatchCompletePayload {
                    parent_id,
                    chat_id,
                    agent_id,
                    run_ids,
                    any_error,
                    summaries,
                },
            );
        });
    }

    /// Cancel one run (kill the subprocess + abort the await task).
    pub async fn cancel(&self, run_id: &str) {
        let bridge = self
            .handles
            .lock()
            .unwrap()
            .remove(run_id)
            .map(|h| h.bridge);
        if let Some(b) = bridge {
            b.cancel(run_id).await;
        }
    }

    /// Cancel every run of a batch (docs/16 `agent:cancel_batch`).
    #[allow(dead_code)]
    pub async fn cancel_batch(&self, parent_id: &str) {
        let ids: Vec<String> = self
            .handles
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, h)| h.parent_id.as_deref() == Some(parent_id))
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.cancel(&id).await;
        }
    }

    /// Approve a finished Write run: merge its worktree branch back into the
    /// base repo (docs/16). Returns the new status.
    pub async fn approve(&self, pool: &SqlitePool, run_id: &str) -> anyhow::Result<String> {
        let handle = self.worktrees.lock().unwrap().remove(run_id);
        if let Some(h) = handle {
            worktree::merge_into_base(&h)
                .await
                .map_err(|e| anyhow::anyhow!("merge failed: {e}"))?;
            models::update_agent_run_status(
                pool,
                run_id,
                "merged",
                None,
                None,
                None,
                None,
                Some(now_ms()),
            )
            .await?;
            Ok("merged".into())
        } else {
            // No worktree (Plan run or shared cwd) — nothing to merge.
            models::update_agent_run_status(
                pool,
                run_id,
                "done",
                None,
                None,
                None,
                None,
                Some(now_ms()),
            )
            .await?;
            Ok("done".into())
        }
    }

    /// Reject a finished Write run: remove the worktree and branch.
    pub async fn reject(&self, pool: &SqlitePool, run_id: &str) -> anyhow::Result<()> {
        let handle = self.worktrees.lock().unwrap().remove(run_id);
        if let Some(h) = handle {
            worktree::remove(&h).await?;
        }
        models::update_agent_run_status(
            pool,
            run_id,
            "cancelled",
            None,
            None,
            None,
            None,
            Some(now_ms()),
        )
        .await?;
        Ok(())
    }

    /// Compute the live diff of a run's worktree (or shared cwd) for in-app
    /// Approve/Reject review. Reconstructs the worktree handle from the DB when
    /// the in-memory entry is gone (e.g. after app restart).
    pub async fn run_diff(&self, pool: &SqlitePool, run_id: &str) -> anyhow::Result<WorktreeDiff> {
        let in_memory = self.worktrees.lock().unwrap().get(run_id).cloned();
        if let Some(h) = in_memory {
            return worktree::diff(&h).await;
        }
        let Some(run) = models::get_agent_run(pool, run_id).await? else {
            anyhow::bail!("agent run not found: {run_id}");
        };
        let branch = run.worktree_branch.clone().unwrap_or_default();
        if !branch.is_empty() && !run.cwd.is_empty() {
            let handle = WorktreeHandle {
                path: run.cwd.clone(),
                branch,
            };
            worktree::diff(&handle).await
        } else if !run.cwd.is_empty() {
            worktree::diff_against_head(&run.cwd).await
        } else {
            Ok(WorktreeDiff::default())
        }
    }
}
