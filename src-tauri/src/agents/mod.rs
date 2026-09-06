pub mod bridge;
pub mod contract;
pub mod parser;
pub mod presets;
pub mod runner;
pub mod task_extract;
pub mod worktree;

pub use runner::AgentRunner;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::config::AgentContract;

/// Task handed to an external agent (docs/07).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub prompt: String,
    pub cwd: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Our mode translated for the agent: "plan" | "write" | "auto" (docs/12).
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Resume id (agent session), if continuing a previous run.
    #[serde(default)]
    pub session: Option<String>,
}

fn default_timeout_ms() -> u64 {
    300_000
}
fn default_max_turns() -> u32 {
    50
}
fn default_mode() -> String {
    "plan".into()
}

impl AgentTask {
    pub fn new(prompt: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            cwd: cwd.into(),
            files: Vec::new(),
            env: HashMap::new(),
            timeout_ms: default_timeout_ms(),
            max_turns: default_max_turns(),
            mode: default_mode(),
            session: None,
        }
    }
}

/// Normalized agent event stream (docs/07). Tagged JSON over IPC.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    Text { text: String },
    ToolAction { name: String, args: String },
    Diff { path: String, patch: String },
    Progress { text: String },
    Error { text: String },
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiffEntry {
    pub path: String,
    pub patch: String,
}

/// Final outcome of an agent run.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    /// Final text answer (aggregated Text events or raw stdout).
    pub text: String,
    /// Collected diffs, if the agent produced structured diff events.
    pub diffs: Vec<DiffEntry>,
    /// done | error | cancelled | timeout
    pub status: String,
    /// Session id to resume this agent later, if any.
    pub session: Option<String>,
}

impl AgentResult {
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            diffs: Vec::new(),
            status: "error".into(),
            session: None,
        }
    }
}

/// Bridge to one external agent (subprocess in MVP, docs/07).
#[async_trait]
pub trait AgentBridge: Send + Sync {
    #[allow(dead_code)]
    fn kind(&self) -> &str;
    /// Run a task; stream events into `sink`, return the final result.
    async fn run(
        &self,
        run_id: &str,
        task: AgentTask,
        sink: mpsc::Sender<AgentEvent>,
    ) -> AgentResult;
    /// Cancel a running task (SIGTERM the process).
    async fn cancel(&self, run_id: &str);
}

/// Build the bridge for a configured agent contract.
pub fn bridge_for(contract: &AgentContract) -> std::sync::Arc<dyn AgentBridge> {
    std::sync::Arc::new(bridge::SubprocessBridge::new(contract.clone()))
}

/// Tool name exposed to the LLM for an agent (docs/07).
pub fn tool_name(agent_id: &str) -> String {
    format!("agent__{}__run", agent_id)
}

/// Extract the agent id from a tool name like `agent__openclaw__run`.
pub fn agent_id_from_tool(tool: &str) -> Option<String> {
    tool.strip_prefix("agent__")?
        .strip_suffix("__run")
        .map(|s| s.to_string())
}

/// Adapter exposing an agent as a builtin-style tool (docs/07). Execution is
/// special-cased in `chat.rs` (needs the runner + DB); the registry entry only
/// makes the tool visible to the LLM with the agent's description.
pub struct AgentToolAdapter {
    pub name: String,
    pub description: String,
}

#[async_trait::async_trait]
impl crate::tools::Tool for AgentToolAdapter {
    fn category(&self) -> crate::tools::ToolCategory {
        // Delegation is always available in Plan/Write (the agent itself enforces
        // mode via mode_flags); never blocked by the file-path gate.
        crate::tools::ToolCategory::Interaction
    }
    fn spec(&self) -> crate::tools::ToolSpec {
        let parameters = match self.name.as_str() {
            "agent__run_batch" => serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "agent": { "type": "string", "description": "Agent id (default = the chat's selected agent)" },
                    "files": { "type": "array", "items": { "type": "string" } },
                    "tasks": {
                        "type": "array",
                        "description": "For agent__run_batch: list of subtasks to run in parallel.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "prompt": { "type": "string" },
                                "agent": { "type": "string" },
                                "files": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["prompt"]
                        }
                    },
                    "background": {
                        "type": "boolean",
                        "description": "If true, detach the batch: return immediately with run_ids and parent_id; a agent:batch_complete event fires on completion and a synthesis turn follows."
                    }
                },
                "required": ["prompt"]
            }),
            "agent__batch_results" => serde_json::json!({
                "type": "object",
                "properties": {
                    "parent_id": { "type": "string", "description": "parent_id of a background agent__run_batch" }
                },
                "required": ["parent_id"]
            }),
            _ => serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "agent": { "type": "string", "description": "Agent id (default = the chat's selected agent)" },
                    "files": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["prompt"]
            }),
        };
        crate::tools::ToolSpec {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters,
        }
    }
    async fn execute(&self, _args: serde_json::Value) -> crate::tools::ToolResult {
        crate::tools::ToolResult::err("agent tools are executed by the chat turn loop")
    }
}

/// Register `agent__run_batch`, `agent__batch_results` and one `agent__<id>__run`
/// tool per active agent (docs/07, docs/16). Skipped in Minimal mode or with no agents.
pub fn add_to_registry(
    registry: &mut crate::tools::Registry,
    agents: &[AgentContract],
    mode: &str,
) {
    if mode == "minimal" || agents.is_empty() {
        return;
    }
    registry.register(Box::new(AgentToolAdapter {
        name: "agent__run_batch".into(),
        description: "Delegate a batch of coding subtasks to external agents running in parallel (isolated git worktrees in Write mode).".into(),
    }));
    registry.register(Box::new(AgentToolAdapter {
        name: "agent__batch_results".into(),
        description: "Retrieve the collected results of a background agent__run_batch by its parent_id. Use this to synthesize a summary after a background batch completes.".into(),
    }));
    for a in agents {
        registry.register(Box::new(AgentToolAdapter {
            name: tool_name(&a.id),
            description: format!(
                "Delegate a coding task to {}: {}. Runs in the project directory.",
                a.name, a.description
            ),
        }));
    }
}

/// System-prompt section listing the available worker agents (docs/07).
/// `None` when there are no active agents.
pub fn roster_block(agents: &[AgentContract]) -> Option<String> {
    if agents.is_empty() {
        return None;
    }
    let mut lines = vec![
        "Available worker agents (delegate coding tasks via agent__<id>__run tools):".to_string(),
    ];
    for a in agents {
        lines.push(format!("- {} (id={}): {}", a.name, a.id, a.description));
    }
    Some(lines.join("\n"))
}
