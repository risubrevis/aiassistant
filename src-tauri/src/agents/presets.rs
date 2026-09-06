use crate::config::AgentContract;
use std::collections::HashMap;

/// Built-in presets for known agents (docs/07). Disabled until the user
/// enables them in Settings.
pub fn presets() -> Vec<AgentContract> {
    vec![claude_code(), opencode(), cline(), ollama_agent()]
}

fn claude_code() -> AgentContract {
    let mut mode_flags = HashMap::new();
    mode_flags.insert(
        "plan".into(),
        vec!["--permission-mode".into(), "plan".into()],
    );
    mode_flags.insert(
        "write".into(),
        vec!["--permission-mode".into(), "acceptEdits".into()],
    );
    AgentContract {
        id: "claude-code".into(),
        name: "Claude Code".into(),
        description: "Agentic coding assistant for complex multi-file changes.".into(),
        capabilities: vec![
            "code_edit".into(),
            "run_commands".into(),
            "plan".into(),
            "diff".into(),
        ],
        kind: "subprocess".into(),
        command: "claude".into(),
        args: vec!["-p".into(), "--output-format".into(), "stream-json".into()],
        prompt_mode: "stdin_json".into(),
        cwd: String::new(),
        env: HashMap::new(),
        output_format: "stream_json".into(),
        event_schema: None,
        mode_flags,
        resume_flag: "--session {session}".into(),
        timeout_ms: 600_000,
        max_turns: 50,
        default_model: String::new(),
    }
}

fn opencode() -> AgentContract {
    let mut mode_flags = HashMap::new();
    mode_flags.insert("plan".into(), vec!["--agent".into(), "plan".into()]);
    mode_flags.insert("write".into(), vec!["--auto".into()]);
    AgentContract {
        id: "opencode".into(),
        name: "OpenCode".into(),
        description: "Provider-agnostic coding agent.".into(),
        capabilities: vec!["code_edit".into(), "run_commands".into(), "plan".into()],
        kind: "subprocess".into(),
        command: "opencode".into(),
        args: vec!["run".into(), "--format".into(), "json".into()],
        prompt_mode: "arg".into(),
        cwd: String::new(),
        env: HashMap::new(),
        output_format: "ndjson".into(),
        event_schema: None,
        mode_flags,
        resume_flag: "--session {session}".into(),
        timeout_ms: 300_000,
        max_turns: 50,
        default_model: String::new(),
    }
}

fn cline() -> AgentContract {
    let mut mode_flags = HashMap::new();
    mode_flags.insert("plan".into(), vec!["-p".into()]);
    mode_flags.insert("write".into(), vec!["--auto-approve".into(), "true".into()]);
    AgentContract {
        id: "cline".into(),
        name: "Cline".into(),
        description: "VS Code-style coding agent with plan and auto-approve modes.".into(),
        capabilities: vec!["code_edit".into(), "run_commands".into(), "plan".into()],
        kind: "subprocess".into(),
        command: "cline".into(),
        args: Vec::new(),
        prompt_mode: "arg".into(),
        cwd: String::new(),
        env: HashMap::new(),
        output_format: "ndjson".into(),
        event_schema: None,
        mode_flags,
        resume_flag: "--id {session}".into(),
        timeout_ms: 300_000,
        max_turns: 50,
        default_model: String::new(),
    }
}

fn ollama_agent() -> AgentContract {
    AgentContract {
        id: "ollama-agent".into(),
        name: "Ollama Agent".into(),
        description: "Local Ollama-based coding agent with tool execution, RAG, and MCP support. Passes the prompt via -p and the model via -m.".into(),
        capabilities: vec!["code_edit".into(), "run_commands".into(), "plan".into()],
        kind: "subprocess".into(),
        command: "ollama-agent".into(),
        args: vec![
            "-y".into(),
            "-m".into(),
            "{model}".into(),
            "-p".into(),
            "{prompt}".into(),
        ],
        prompt_mode: "arg".into(),
        cwd: String::new(),
        env: HashMap::new(),
        output_format: "raw_text".into(),
        event_schema: None,
        mode_flags: HashMap::new(),
        resume_flag: "none".into(),
        timeout_ms: 300_000,
        max_turns: 50,
        default_model: String::new(),
    }
}

/// Resolve a command name to an absolute path via PATH lookup, if it exists.
pub fn detect_binary(command: &str) -> Option<String> {
    let out = std::process::Command::new("which")
        .arg(command)
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if p.is_empty() {
            None
        } else {
            Some(p)
        }
    } else {
        None
    }
}
