mod model;
mod watch;
mod write;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
// Re-exported for the settings surface; no internal consumer yet.
#[allow(unused_imports)]
pub use model::General;
pub use model::{
    AgentContract, AppConfig as Config, EventSchema, ModelRef, Network, Permissions, Provider,
    Skill, WebSearch,
};
use tracing::warn;

pub use watch::spawn_config_watcher;
pub use write::write_max_turns;
pub use write::{
    write_add_environment_info, write_auto_collapse_context_pct, write_auto_pull_changes,
    write_defaults_field, write_defaults_model_ref, write_delete_to_trash, write_disabled_tools,
    write_environment_info, write_logging_field, write_mode, write_network,
    write_notifications_enabled, write_rag_enabled, write_remember_window_state,
    write_send_on_enter, write_system_prompt, write_vision_model_enabled, write_web_search,
};

/// App config directory: `config_dir/aiassistant` per-OS (XDG/AppData/Library).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aiassistant")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn system_prompt_path() -> PathBuf {
    config_dir().join("system_prompt.md")
}

pub fn environment_path() -> PathBuf {
    config_dir().join("environment.md")
}

/// Load the config from disk, creating a commented default file if it's missing.
/// `system_prompt.md` overrides the legacy value in config.toml; a missing file
/// is seeded from it on first run.
/// Unknown fields are not fatal (forward-compat); they are logged as warnings.
pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        write_default_config(&path)?;
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let mut config: Config = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config at {}", path.display()))?;
    merge_system_prompt(&mut config)?;
    for warning in validate(&config) {
        warn!(config.path = %path.display(), "config validation: {warning}");
    }
    Ok(config)
}

/// `system_prompt.md` overrides `[defaults].system_prompt`; a missing file is
/// seeded from the parsed config.toml value (legacy import or built-in default).
fn merge_system_prompt(config: &mut Config) -> Result<()> {
    let path = system_prompt_path();
    if path.exists() {
        config.defaults.system_prompt = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read system prompt at {}", path.display()))?;
        Ok(())
    } else {
        write::write_system_prompt(&config.defaults.system_prompt)
    }
}

/// Atomically write the default commented config (temp file + rename).
pub fn write_default_config(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, DEFAULT_CONFIG_TOML)
        .with_context(|| format!("failed to write default config to {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Update a single `[appearance]` string field in `config.toml` while
/// preserving the user's comments and formatting (via `toml_edit`).
pub fn write_appearance_field(key: &str, value: &str) -> Result<()> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = raw
        .parse()
        .with_context(|| format!("failed to parse config at {}", path.display()))?;

    let table = doc
        .entry("appearance")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .context("[appearance] is not a table")?;
    table.insert(key, toml_edit::value(value));

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Lightweight validation of enum sanity.
/// Returns a list of human-readable warnings (non-fatal).
pub fn validate(config: &Config) -> Vec<String> {
    let mut warnings = Vec::new();

    if !matches!(
        config.appearance.theme.as_str(),
        "light" | "dark" | "system"
    ) {
        warnings.push(format!(
            "appearance.theme has invalid value: {}",
            config.appearance.theme
        ));
    }

    warnings
}

/// Default commented config written on first run. Mirrors docs/14-config-format.md
/// (minimal subset for the walking skeleton).
const DEFAULT_CONFIG_TOML: &str = r#"# AIAssistant configuration.

config_version = 1

[appearance]
theme = "system"            # light | dark | system
accent = ""                 # hex; empty = system/default
font_size = 14
mono_font = ""              # empty = ui-monospace stack
language = "en"             # i18n
show_thinking = false
compact = false

[general]
send_on_enter = true         # Enter sends a message, Shift+Enter for a new line. false = Enter inserts a newline (send via Cmd/Ctrl+Enter hotkey).
notifications_enabled = true # desktop notification when a response / agent task completes
remember_window_state = true # restore window size & position on launch

[defaults]
mode = "plan"               # minimal | plan | write
command_toggle = "manual"   # manual | auto
edit_toggle = "ask"         # ask | auto
# Default models (configured in Settings → Models). Values are
# provider/model UUIDs from the DB. Empty/unset = not selected.
# main_model = { provider = "<provider-uuid>", model = "<model-uuid>" }
# secondary_model = { provider = "<provider-uuid>", model = "<model-uuid>" }
# embedding_model = { provider = "<provider-uuid>", model = "<model-uuid>" }
# vision_model_enabled = false   # route image analysis to a separate vision model (analyze_image tool)
# vision_model = { provider = "<provider-uuid>", model = "<model-uuid>" }
# System prompt: system_prompt.md (same dir).
auto_collapse_context_pct = 90
max_turns = 50              # max LLM tool-call round-trips per turn (built-in agent loop); 0 = unlimited; raise for long multi-step tasks
disabled_tools = []          # builtin tool names hidden from the LLM (Settings → Tools)
auto_pull_changes = true     # inject changed project files into context at turn start
delete_to_trash = false      # delete_path: move to OS trash/recycle bin instead of permanent removal
add_environment_info = true  # inject environment.md (user-editable OS info) into the prompt
rag_enabled = true           # RAG master switch: index attachments/project files + retrieval
stream_retries = 3           # auto-retry attempts on retryable stream errors (network/rate-limit/server); 0 = fail fast to manual Retry

[hotkeys]
new_chat = "CmdOrCtrl+N"
new_project = "CmdOrCtrl+Shift+N"
palette = "CmdOrCtrl+K"
send = "CmdOrCtrl+Enter"
toggle_sidebar = "CmdOrCtrl+B"
settings = "CmdOrCtrl+Comma"

[logging]
level = "info"
file_level = "debug"
redact_secrets = true
max_bytes = 52428800

[network]
# Outbound HTTP proxy. The proxy password is kept in the OS keychain.
proxy_enabled = false    # master switch
proxy_type = "http"      # http | https | socks5 | socks5h  (socks5h = DNS via proxy, use for TOR)
proxy_host = ""          # e.g. 127.0.0.1
proxy_port = 0           # e.g. 9050 for TOR, 1080 for a SOCKS5 proxy, 8080 for HTTP
proxy_username = ""      # empty = no auth
# no_proxy: hosts/IPs that bypass the proxy (comma-separated)
no_proxy = "localhost,127.0.0.1,::1"
test_url = "https://www.google.com"   # hit by the Test button
verify_tls = true        # set false only for proxies with self-signed certificates
ca_cert_path = ""        # optional PEM/DER file with a corporate proxy CA
connect_timeout_ms = 10000

[web_search]
# User-Agent for the builtin web_search tool. Search engines block naive /
# outdated UAs with a CAPTCHA, so this defaults to a current Chrome desktop
# string. Override if a provider blocks the default.
user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36"
accept_language = "en-US,en;q=0.9"
# Extra request headers, one per line as `Key: Value` (appended after the
# built-in browser-like headers; last wins on conflict). Empty = none.
extra_headers = ""
timeout_ms = 20000

# Permission patterns by path/command. Last match wins.
# allow | ask | deny. Combined with mode/toggles.
[permissions]
allowed_paths = []
read  = { ".env" = "deny", ".env.*" = "deny", "**/.ssh/**" = "deny", "**/secrets/**" = "deny", "*" = "allow" }
edit  = { ".env*" = "deny", "**/.git/**" = "deny", "*" = "ask" }
bash  = { "rm -rf *" = "deny", "sudo *" = "deny", "*" = "ask" }
"#;
