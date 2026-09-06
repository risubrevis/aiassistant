use serde::Serialize;
use serde_json::Value;

use crate::config::Permissions;
use crate::tools::ToolCategory;

/// Permission gate decision (docs/12, docs/17).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    /// Stage a file mutation (snapshot + apply + Pending changes panel). docs/12.
    Stage,
    /// Ask the user (approval card). `summary` is shown; `preview` optional (diff/command).
    Ask {
        summary: String,
        preview: Option<String>,
    },
    /// Denied — returned to the LLM as a tool result so it can adapt.
    Deny(String),
}

/// Translate a glob pattern to a regex (segment-aware for paths).
/// `*` -> `[^/]*`, `**` -> `.*`, `?` -> `[^/]`, others escaped.
fn glob_to_regex(pat: &str, segment: bool) -> String {
    let mut out = String::with_capacity(pat.len() + 2);
    out.push('^');
    let bytes = pat.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'*' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    out.push_str(".*");
                    i += 2;
                    continue;
                }
                if segment {
                    out.push_str("[^/]*");
                } else {
                    out.push_str(".*");
                }
            }
            b'?' => {
                if segment {
                    out.push_str("[^/]");
                } else {
                    out.push('.');
                }
            }
            c if matches!(
                c,
                b'.' | b'^' | b'$' | b'+' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'\\'
            ) =>
            {
                out.push('\\');
                out.push(c as char);
            }
            c => out.push(c as char),
        }
        i += 1;
    }
    out.push('$');
    out
}

fn matches(pat: &str, value: &str, segment: bool) -> bool {
    let re = match regex::Regex::new(&glob_to_regex(pat, segment)) {
        Ok(r) => r,
        Err(_) => return pat == value,
    };
    if re.is_match(value) || re.is_match(&value.replace('\\', "/")) {
        return true;
    }
    // For path patterns also match against the basename (e.g. ".env" matches "/tmp/.env").
    if segment {
        if let Some(b) = std::path::Path::new(value).file_name() {
            if re.is_match(&b.to_string_lossy()) {
                return true;
            }
        }
    }
    false
}

/// Longest matching pattern wins (so `.env`/`src/**` beat `*`).
fn match_rules(
    rules: &std::collections::HashMap<String, String>,
    value: &str,
    segment: bool,
) -> Option<String> {
    let mut best: Option<(&str, &str)> = None;
    for (pat, decision) in rules {
        if matches(pat, value, segment) {
            match best {
                None => best = Some((pat, decision)),
                Some((bp, _)) if pat.len() > bp.len() => best = Some((pat, decision)),
                _ => {}
            }
        }
    }
    best.map(|(_, d)| normalize_decision(d))
}

fn normalize_decision(d: &str) -> String {
    match d {
        "allow" | "ask" | "deny" => d.to_string(),
        _ => "ask".to_string(),
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Evaluate the permission gate for a tool call.
pub fn gate(
    category: ToolCategory,
    name: &str,
    args: &Value,
    mode: &str,
    command_toggle: &str,
    edit_toggle: &str,
    perms: &Permissions,
) -> Decision {
    // 1. Mode gate.
    if mode == "minimal" {
        return Decision::Deny("tools are disabled in Minimal mode".into());
    }
    if mode == "plan"
        && matches!(
            category,
            ToolCategory::Write | ToolCategory::Exec | ToolCategory::Destructive
        )
    {
        return Decision::Deny(format!("{name} is not allowed in Plan mode"));
    }

    match category {
        ToolCategory::Readonly | ToolCategory::Interaction => {
            let path = arg_str(args, "path").unwrap_or_default();
            if path.is_empty() {
                return Decision::Allow;
            }
            match match_rules(&perms.read, &path, true).as_deref() {
                Some("allow") => Decision::Allow,
                Some("ask") => Decision::Ask {
                    summary: format!("read {name}: {path}"),
                    preview: None,
                },
                Some("deny") => Decision::Deny(format!("read denied by policy: {path}")),
                Some(_) | None => Decision::Allow,
            }
        }
        ToolCategory::Write => {
            let path = arg_str(args, "path").unwrap_or_default();
            if path.is_empty() {
                return Decision::Allow;
            }
            match match_rules(&perms.edit, &path, true).as_deref() {
                Some("deny") => Decision::Deny(format!("edit denied by policy: {path}")),
                // edit_toggle=auto -> apply immediately; ask -> stage (Pending changes).
                _ => {
                    if edit_toggle == "auto" {
                        Decision::Allow
                    } else {
                        Decision::Stage
                    }
                }
            }
        }
        ToolCategory::Destructive => {
            let path = arg_str(args, "path").unwrap_or_default();
            // Always Ask, even in Write+Auto (docs/12).
            Decision::Ask {
                summary: format!("{name} {path}"),
                preview: edit_preview(name, args),
            }
        }
        ToolCategory::Exec => {
            let cmd = arg_str(args, "command").unwrap_or_default();
            if cmd.is_empty() {
                return Decision::Allow;
            }
            match match_rules(&perms.bash, &cmd, false).as_deref() {
                Some("deny") => Decision::Deny(format!("command denied by policy: {cmd}")),
                Some("allow") => Decision::Allow,
                Some(_) | None => {
                    if command_toggle == "auto" {
                        Decision::Allow
                    } else {
                        Decision::Ask {
                            summary: format!("run: {cmd}"),
                            preview: Some(cmd.clone()),
                        }
                    }
                }
            }
        }
        ToolCategory::Network => Decision::Ask {
            summary: name.to_string(),
            preview: None,
        },
    }
}

fn edit_preview(name: &str, args: &Value) -> Option<String> {
    match name {
        "write_file" => arg_str(args, "content"),
        "edit_file" => {
            let path = arg_str(args, "path").unwrap_or_default();
            let ops = args.get("operations").and_then(|v| v.as_array());
            let mut s = String::new();
            if let Some(ops) = ops {
                for op in ops {
                    let o = op.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
                    let n = op.get("new_string").and_then(|v| v.as_str()).unwrap_or("");
                    s.push_str(&format!("--- {}\n+++ {}\n", o, n));
                }
            }
            if s.is_empty() {
                None
            } else {
                Some(format!("{path}\n{s}"))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Permissions;
    use crate::tools::ToolCategory;
    use serde_json::json;

    fn perms() -> Permissions {
        Permissions {
            allowed_paths: vec![],
            read: [(".env", "deny"), ("**/.ssh/**", "deny"), ("*", "allow")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            edit: [(".env*", "deny"), ("src/**", "allow"), ("*", "ask")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            bash: [("rm -rf *", "deny"), ("git *", "allow"), ("*", "ask")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn plan_blocks_write_and_exec() {
        let p = perms();
        let args = json!({ "path": "/tmp/x" });
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "write_file",
                &args,
                "plan",
                "manual",
                "ask",
                &p
            ),
            Decision::Deny(_)
        ));
        assert!(matches!(
            gate(
                ToolCategory::Exec,
                "run_command",
                &json!({"command":"ls"}),
                "plan",
                "manual",
                "ask",
                &p
            ),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn readonly_allows_and_denies_env() {
        let p = perms();
        assert!(matches!(
            gate(
                ToolCategory::Readonly,
                "read_file",
                &json!({"path":"/tmp/x"}),
                "plan",
                "manual",
                "ask",
                &p
            ),
            Decision::Allow
        ));
        assert!(matches!(
            gate(
                ToolCategory::Readonly,
                "read_file",
                &json!({"path":"/tmp/.env"}),
                "plan",
                "manual",
                "ask",
                &p
            ),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn edit_pattern_and_toggle() {
        let p = perms();
        // src/** allow, edit_toggle=auto -> Allow
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "write_file",
                &json!({"path":"src/main.rs"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Allow
        ));
        // src/** allow but edit_toggle=ask -> Stage (Pending changes)
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "write_file",
                &json!({"path":"src/main.rs"}),
                "write",
                "manual",
                "ask",
                &p
            ),
            Decision::Stage
        ));
        // .env* deny
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "write_file",
                &json!({"path":".env"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn exec_manual_asks_and_bash_patterns() {
        let p = perms();
        // manual toggle -> Ask
        assert!(matches!(
            gate(
                ToolCategory::Exec,
                "run_command",
                &json!({"command":"ls"}),
                "write",
                "manual",
                "ask",
                &p
            ),
            Decision::Ask { .. }
        ));
        // auto toggle + git * allow -> Allow
        assert!(matches!(
            gate(
                ToolCategory::Exec,
                "run_command",
                &json!({"command":"git status"}),
                "write",
                "auto",
                "ask",
                &p
            ),
            Decision::Allow
        ));
        // auto toggle but rm -rf deny -> Deny
        assert!(matches!(
            gate(
                ToolCategory::Exec,
                "run_command",
                &json!({"command":"rm -rf /"}),
                "write",
                "auto",
                "ask",
                &p
            ),
            Decision::Deny(_)
        ));
    }
}
