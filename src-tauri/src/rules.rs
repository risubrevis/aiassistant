use sqlx::SqlitePool;

use crate::config::{environment_path, Config};
use crate::db::models::{self, Chat, Project};

/// Assemble the effective system prompt for a chat turn (docs/08):
///   [base prompt: project overrides global] + [chat prompt]
///   + [Environment info, for plan/write modes]
///   + [Rules: global / project]
///   + [cross-chat sibling summaries, if hybrid/summary].
pub async fn effective_system_prompt(
    pool: &SqlitePool,
    config: &Config,
    chat: &Chat,
    project: Option<&Project>,
    mode: &str,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Base prompt: project overrides global when non-empty.
    let base = project
        .and_then(|p| {
            if !p.system_prompt.is_empty() {
                Some(&p.system_prompt)
            } else {
                None
            }
        })
        .unwrap_or(&config.defaults.system_prompt);
    if !base.is_empty() {
        parts.push(base.clone());
    }
    if let Some(sp) = chat.system_prompt.as_ref() {
        if !sp.is_empty() {
            parts.push(sp.clone());
        }
    }

    // Live-read project context files (AGENTS.md, CLAUDE.md, …) from attached
    // dirs. Read fresh each turn so on-disk edits reach the model without a
    // re-attach. (Replaces the old one-time DB auto-import.)
    if let Some(p) = project {
        if let Ok(paths) = models::list_project_paths(pool, &p.id).await {
            let roots: Vec<std::path::PathBuf> = paths
                .into_iter()
                .map(|pp| std::path::PathBuf::from(pp.path))
                .collect();
            let files = crate::projects::discover_rule_files(&roots);
            if !files.is_empty() {
                let mut blocks = Vec::with_capacity(files.len());
                for (filename, content) in &files {
                    blocks.push(format!("[{}]\n{}", filename, content.trim()));
                }
                parts.push(format!("Project context files:\n{}", blocks.join("\n\n")));
            }
        }
    }

    // Environment info helps the model pick correct shell commands and paths
    // when it may write files or run commands (plan/write modes only).
    if (mode == "plan" || mode == "write") && config.defaults.add_environment_info {
        parts.push(environment_info_block(pool, project).await);
    }

    // Rules block: global rules unless the project opts out, plus project rules.
    let mut rules_lines: Vec<String> = Vec::new();
    let apply_global = project.map_or(true, |p| p.include_global_rules != 0);
    if apply_global {
        if let Ok(global) = models::list_global_rules(pool).await {
            let active: Vec<_> = global.iter().filter(|r| r.is_active == 1).collect();
            if !active.is_empty() {
                rules_lines.push("Global rules:".into());
                for r in active {
                    rules_lines.push(format!("- {}", r.text));
                }
            }
        }
    }
    if let Some(p) = project {
        if let Ok(pr) = models::list_project_rules(pool, &p.id).await {
            let active: Vec<_> = pr.iter().filter(|r| r.is_active == 1).collect();
            if !active.is_empty() {
                rules_lines.push("Project rules:".into());
                for r in active {
                    rules_lines.push(format!("- {}", r.text));
                }
            }
        }
    }
    if !rules_lines.is_empty() {
        parts.push(format!("Rules:\n{}", rules_lines.join("\n")));
    }

    // Cross-chat sibling summaries (hybrid/summary modes).
    if let Some(p) = project {
        let mode = project_cross_chat_mode(p);
        if mode == "hybrid" || mode == "summary" {
            if let Ok(siblings) = models::list_project_chats(pool, &p.id).await {
                let siblings: Vec<_> = siblings.into_iter().filter(|c| c.id != chat.id).collect();
                if !siblings.is_empty() {
                    let mut lines = vec!["Other chats in this project:".to_string()];
                    for c in siblings.iter().take(12) {
                        let summary =
                            meta_summary(&c.meta).unwrap_or_else(|| "(no summary yet)".into());
                        lines.push(format!("- {} ({}): {}", c.title, c.id, summary));
                    }
                    parts.push(lines.join("\n"));
                }
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Read the cross-chat mode from project settings JSON (default: hybrid).
pub fn project_cross_chat_mode(project: &Project) -> String {
    if let Some(s) = project.settings.as_ref() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            if let Some(m) = v.get("cross_chat").and_then(|x| x.as_str()) {
                return m.to_string();
            }
        }
    }
    "hybrid".to_string()
}

/// os-release pretty name (Linux), sw_vers (macOS) or the plain const.
fn os_pretty_name() -> String {
    match std::env::consts::OS {
        "linux" => std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_string())
                    .or_else(|| {
                        s.lines()
                            .find_map(|l| l.strip_prefix("NAME="))
                            .map(|v| v.trim_matches('"').to_string())
                    })
            })
            .unwrap_or_else(|| "Linux".into()),
        "macos" => std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|v| format!("macOS {}", v.trim()))
            .unwrap_or_else(|| "macOS".into()),
        "windows" => "Windows".into(),
        other => other.to_string(),
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default()
}

fn shell_name() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC")
            .ok()
            .and_then(|p| {
                std::path::Path::new(&p)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "powershell".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

/// Working directory: the first project dir path, else the app's cwd.
async fn working_directory(pool: &SqlitePool, project: Option<&Project>) -> String {
    if let Some(p) = project {
        if let Ok(paths) = models::list_project_paths(pool, &p.id).await {
            if let Some(dir) = paths.iter().find(|pp| pp.kind == "dir") {
                return dir.path.clone();
            }
        }
    }
    std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

async fn environment_block(pool: &SqlitePool, project: Option<&Project>) -> String {
    let os = os_pretty_name();
    let host = hostname();
    let shell = shell_name();
    let cwd = working_directory(pool, project).await;
    let date = chrono::Local::now().format("%Y-%m-%d");
    let mut lines = vec![
        "Environment:".to_string(),
        format!("- OS: {} ({})", os, std::env::consts::ARCH),
    ];
    if !host.is_empty() {
        lines.push(format!("- Host: {}", host));
    }
    lines.push(format!("- Shell: {}", shell));
    if !cwd.is_empty() {
        lines.push(format!("- Working directory: {}", cwd));
    }
    lines.push(format!("- Date: {}", date));
    lines.join("\n")
}

/// Prefer the user-editable `environment.md`; fall back to the auto-detected
/// block while it is missing or empty.
async fn environment_info_block(pool: &SqlitePool, project: Option<&Project>) -> String {
    if let Ok(content) = std::fs::read_to_string(environment_path()) {
        let content = content.trim();
        if !content.is_empty() {
            return format!("Environment info:\n{}", content);
        }
    }
    environment_block(pool, project).await
}

/// Extract a 1-line summary from chat meta JSON (`meta.summary`).
pub fn meta_summary(meta: &Option<String>) -> Option<String> {
    let s = meta.as_ref()?;
    let v = serde_json::from_str::<serde_json::Value>(s).ok()?;
    let sum = v.get("summary").and_then(|x| x.as_str())?;
    if sum.is_empty() {
        None
    } else {
        Some(sum.to_string())
    }
}

/// Set `summary` in a chat meta JSON, preserving other keys.
pub fn with_summary(meta: &Option<String>, summary: &str) -> String {
    let mut v = meta
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("summary".into(), serde_json::json!(summary));
    } else {
        v = serde_json::json!({ "summary": summary });
    }
    v.to_string()
}

/// Set `active_leaf_id` in a chat meta JSON, preserving other keys.
pub fn with_active_leaf(meta: &Option<String>, leaf_id: &str) -> String {
    let mut v = meta
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("active_leaf_id".into(), serde_json::json!(leaf_id));
    } else {
        v = serde_json::json!({ "active_leaf_id": leaf_id });
    }
    v.to_string()
}

/// Read `active_leaf_id` from chat meta JSON.
pub fn active_leaf(meta: &Option<String>) -> Option<String> {
    let s = meta.as_ref()?;
    let v = serde_json::from_str::<serde_json::Value>(s).ok()?;
    v.get("active_leaf_id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}
