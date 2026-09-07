use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Permissions {
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// pattern -> "allow" | "ask" | "deny" (last match wins). docs/12, docs/17.
    #[serde(default)]
    pub read: HashMap<String, String>,
    #[serde(default)]
    pub edit: HashMap<String, String>,
    #[serde(default)]
    pub bash: HashMap<String, String>,
}

impl Permissions {
    pub fn defaults() -> Self {
        let mut read = HashMap::new();
        read.insert(".env".into(), "deny".into());
        read.insert(".env.*".into(), "deny".into());
        read.insert("**/.ssh/**".into(), "deny".into());
        read.insert("**/secrets/**".into(), "deny".into());
        read.insert("*".into(), "allow".into());
        let mut edit = HashMap::new();
        edit.insert(".env*".into(), "deny".into());
        edit.insert("**/.git/**".into(), "deny".into());
        edit.insert("*".into(), "ask".into());
        let mut bash = HashMap::new();
        bash.insert("rm -rf *".into(), "deny".into());
        bash.insert("sudo *".into(), "deny".into());
        bash.insert("*".into(), "ask".into());
        Self {
            allowed_paths: Vec::new(),
            read,
            edit,
            bash,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub hotkeys: Hotkeys,
    #[serde(default = "Permissions::defaults")]
    pub permissions: Permissions,
    #[serde(default)]
    pub rules: Vec<GlobalRule>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub agents_limits: AgentsLimits,
    #[serde(default)]
    pub worktree: Worktree,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default)]
    pub network: Network,
}

fn default_config_version() -> u32 {
    1
}

/// A global rule (applies to all chats). Project/chat rules live in DB (docs/08).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalRule {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A global skill: a reusable prompt snippet the user attaches to a single
/// chat message. Bodies are injected into the user message text on send.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// Declarative contract for an external CLI agent (docs/07). The app builds the
/// command from this contract; the LLM only sees `agent__<id>__run({prompt})`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContract {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Tags for the LLM description + UI: code_edit, run_commands, plan, diff.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default = "default_agent_kind")]
    pub kind: String,
    #[serde(default)]
    pub command: String,
    /// Argument templates with {prompt}/{cwd}/{session}/{model} placeholders.
    #[serde(default)]
    pub args: Vec<String>,
    /// arg | stdin | stdin_json
    #[serde(default = "default_prompt_mode")]
    pub prompt_mode: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// stream_json (claude-code) | ndjson | raw_text
    #[serde(default = "default_output_format")]
    pub output_format: String,
    /// Custom NDJSON key mapping (docs/07).
    #[serde(default)]
    pub event_schema: Option<EventSchema>,
    /// "plan"/"write"/"auto" -> extra argv flags for our mode.
    #[serde(default)]
    pub mode_flags: HashMap<String, Vec<String>>,
    /// Resume template like "--session {session}"; empty = no resume.
    #[serde(default)]
    pub resume_flag: String,
    #[serde(default = "default_agent_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub max_turns: u32,
    /// Default model passed to the agent via the {model} argv placeholder.
    #[serde(default)]
    pub default_model: String,
}

fn default_agent_kind() -> String {
    "subprocess".into()
}
fn default_prompt_mode() -> String {
    "arg".into()
}
fn default_output_format() -> String {
    "raw_text".into()
}
fn default_agent_timeout() -> u64 {
    300_000
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventSchema {
    #[serde(default)]
    pub text_key: Option<String>,
    #[serde(default)]
    pub diff_key: Option<String>,
    #[serde(default)]
    pub command_key: Option<String>,
}

/// Parallel sub-agent limits (docs/16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsLimits {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    #[serde(default = "default_max_concurrent")]
    pub per_chat_max: u32,
    #[serde(default = "default_isolation")]
    pub isolation: String,
}

fn default_max_concurrent() -> u32 {
    5
}
fn default_isolation() -> String {
    "worktree".into()
}

impl Default for AgentsLimits {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            per_chat_max: default_max_concurrent(),
            isolation: default_isolation(),
        }
    }
}

/// Git worktree isolation settings (docs/16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    /// head (local HEAD) | fresh (origin/<default>)
    #[serde(default = "default_base_ref")]
    pub base_ref: String,
    #[serde(default = "default_true")]
    pub symlink_gitignored: bool,
}

fn default_base_ref() -> String {
    "head".into()
}

impl Default for Worktree {
    fn default() -> Self {
        Self {
            base_ref: default_base_ref(),
            symlink_gitignored: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_version: 1,
            appearance: Appearance::default(),
            defaults: Defaults::default(),
            hotkeys: Hotkeys::default(),
            permissions: Permissions::defaults(),
            rules: Vec::new(),
            skills: Vec::new(),
            agents_limits: AgentsLimits::default(),
            worktree: Worktree::default(),
            logging: Logging::default(),
            network: Network::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default)]
    pub mono_font: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub show_thinking: bool,
    #[serde(default)]
    pub compact: bool,
}

fn default_theme() -> String {
    "system".into()
}
fn default_font_size() -> u32 {
    14
}
fn default_language() -> String {
    "en".into()
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            accent: String::new(),
            font_size: default_font_size(),
            mono_font: String::new(),
            language: default_language(),
            show_thinking: false,
            compact: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_command_toggle")]
    pub command_toggle: String,
    #[serde(default = "default_edit_toggle")]
    pub edit_toggle: String,
    #[serde(default)]
    pub main_model: Option<ModelRef>,
    #[serde(default)]
    pub secondary_model: Option<ModelRef>,
    #[serde(default)]
    pub embedding_model: Option<ModelRef>,
    /// Master switch for retrieval-augmented generation (RAG): index chat
    /// attachments and project files, then inject relevant snippets into the
    /// system prompt. When false, no indexing or retrieval happens even if an
    /// embedding model is configured (existing vectors are preserved).
    #[serde(default = "default_true")]
    pub rag_enabled: bool,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_collapse_pct")]
    pub auto_collapse_context_pct: u32,
    /// Disabled builtin tools (Settings → Tools toggles); empty = all enabled.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Auto-pull changed project files into context at turn start (docs/08).
    #[serde(default = "default_true")]
    pub auto_pull_changes: bool,
    /// `delete_path` moves to the OS trash/recycle bin instead of permanent removal.
    #[serde(default)]
    pub delete_to_trash: bool,
    /// Inject user-editable OS/environment info into the system prompt (plan/write modes).
    #[serde(default = "default_true")]
    pub add_environment_info: bool,
    /// Max LLM tool-call round-trips per turn (built-in agent loop).
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
}

fn default_mode() -> String {
    "plan".into()
}
fn default_command_toggle() -> String {
    "manual".into()
}
fn default_edit_toggle() -> String {
    "ask".into()
}
fn default_collapse_pct() -> u32 {
    90
}
fn default_max_turns() -> u32 {
    50
}
/// A sane, provider-agnostic starter system prompt (editable in Settings → Prompts).
fn default_system_prompt() -> String {
    DEFAULT_SYSTEM_PROMPT.into()
}

/// Canonical default system prompt text (also written into the default config.toml
/// and mirrored in the frontend for pre-filling empty configs).
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a helpful, knowledgeable assistant in a desktop app for chatting and working on code projects.

Guidelines:
- Be clear and concise; avoid filler and unnecessary restatement.
- If a request is ambiguous or lacks details, ask a short clarifying question instead of guessing.
- For code, prefer correct, minimal solutions and explain non-obvious decisions briefly.
- Use the available tools when they help, and briefly state what you are doing.
- When unsure about facts, say so rather than fabricating."#;

impl Default for Defaults {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            command_toggle: default_command_toggle(),
            edit_toggle: default_edit_toggle(),
            main_model: None,
            secondary_model: None,
            embedding_model: None,
            rag_enabled: true,
            system_prompt: default_system_prompt(),
            auto_collapse_context_pct: default_collapse_pct(),
            disabled_tools: Vec::new(),
            auto_pull_changes: true,
            delete_to_trash: false,
            add_environment_info: true,
            max_turns: default_max_turns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotkeys {
    #[serde(default = "hk_new_chat")]
    pub new_chat: String,
    #[serde(default = "hk_new_project")]
    pub new_project: String,
    #[serde(default = "hk_palette")]
    pub palette: String,
    #[serde(default = "hk_send")]
    pub send: String,
    #[serde(default = "hk_toggle_sidebar")]
    pub toggle_sidebar: String,
    #[serde(default = "hk_settings")]
    pub settings: String,
}

fn hk_new_chat() -> String {
    "CmdOrCtrl+N".into()
}
fn hk_new_project() -> String {
    "CmdOrCtrl+Shift+N".into()
}
fn hk_palette() -> String {
    "CmdOrCtrl+K".into()
}
fn hk_send() -> String {
    "CmdOrCtrl+Enter".into()
}
fn hk_toggle_sidebar() -> String {
    "CmdOrCtrl+B".into()
}
fn hk_settings() -> String {
    "CmdOrCtrl+Comma".into()
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            new_chat: hk_new_chat(),
            new_project: hk_new_project(),
            palette: hk_palette(),
            send: hk_send(),
            toggle_sidebar: hk_toggle_sidebar(),
            settings: hk_settings(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_ref: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_provider_kind() -> String {
    "openai".into()
}
fn default_timeout_ms() -> u64 {
    30000
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_file_level")]
    pub file_level: String,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
}

fn default_log_level() -> String {
    "info".into()
}
fn default_file_level() -> String {
    "debug".into()
}
fn default_max_bytes() -> u64 {
    52_428_800
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_level: default_file_level(),
            redact_secrets: true,
            max_bytes: default_max_bytes(),
        }
    }
}

/// Network/proxy settings (Settings → Network). The proxy password is NOT
/// stored here — it lives in the OS keychain (secrets::get_proxy_password).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    #[serde(default)]
    pub proxy_enabled: bool,
    /// "http" | "https" | "socks5" | "socks5h"
    #[serde(default = "default_proxy_type")]
    pub proxy_type: String,
    #[serde(default)]
    pub proxy_host: String,
    #[serde(default)]
    pub proxy_port: u16,
    #[serde(default)]
    pub proxy_username: String,
    /// Comma/newline-separated hosts/IPs that bypass the proxy.
    #[serde(default = "default_no_proxy")]
    pub no_proxy: String,
    /// URL hit by the Test button.
    #[serde(default = "default_test_url")]
    pub test_url: String,
    /// When false, TLS certificate errors are ignored (self-signed MITM proxies).
    #[serde(default = "default_true")]
    pub verify_tls: bool,
    /// Optional path to a PEM/DER CA certificate (corporate proxy CA).
    #[serde(default)]
    pub ca_cert_path: String,
    /// Connect timeout for the Test button, in ms.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
}

fn default_proxy_type() -> String {
    "http".into()
}
fn default_no_proxy() -> String {
    "localhost,127.0.0.1,::1".into()
}
fn default_test_url() -> String {
    "https://www.google.com".into()
}
fn default_connect_timeout_ms() -> u64 {
    10_000
}

impl Default for Network {
    fn default() -> Self {
        Self {
            proxy_enabled: false,
            proxy_type: default_proxy_type(),
            proxy_host: String::new(),
            proxy_port: 0,
            proxy_username: String::new(),
            no_proxy: default_no_proxy(),
            test_url: default_test_url(),
            verify_tls: true,
            ca_cert_path: String::new(),
            connect_timeout_ms: default_connect_timeout_ms(),
        }
    }
}
