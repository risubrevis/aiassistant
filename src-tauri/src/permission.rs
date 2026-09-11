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

/// In Write mode with Commands on Auto, readonly and network research tools
/// run without an approval card — less friction during exploration. Explicit
/// `deny` rules still win; only `ask` is downgraded to `allow`.
fn is_auto_research(mode: &str, command_toggle: &str) -> bool {
    mode == "write" && command_toggle == "auto"
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
                Some("deny") => Decision::Deny(format!("read denied by policy: {path}")),
                Some("ask") => {
                    // Write + Commands Auto: don't interrupt research with
                    // read approval cards. `deny` above still blocks.
                    if is_auto_research(mode, command_toggle) {
                        Decision::Allow
                    } else {
                        Decision::Ask {
                            summary: format!("read {name}: {path}"),
                            preview: None,
                        }
                    }
                }
                Some(_) | None => Decision::Allow,
            }
        }
        ToolCategory::Write => {
            // Collect the paths this tool touches. move_path uses src/dst; other
            // write tools use path. Path-less writes (e.g. web_hook_add/modify,
            // which mutate the Settings DB) have no file to snapshot.
            let paths: Vec<String> = match name {
                "move_path" => [arg_str(args, "src"), arg_str(args, "dst")]
                    .into_iter()
                    .flatten()
                    .collect(),
                _ => arg_str(args, "path").into_iter().collect(),
            };
            // `deny` rules block regardless of the toggle.
            for p in &paths {
                if !p.is_empty()
                    && matches!(match_rules(&perms.edit, p, true).as_deref(), Some("deny"))
                {
                    return Decision::Deny(format!("edit denied by policy: {p}"));
                }
            }
            if edit_toggle == "auto" {
                Decision::Allow
            } else if paths.iter().any(|p| !p.is_empty()) {
                // edit_toggle=ask -> stage (Pending changes snapshot+revert).
                Decision::Stage
            } else {
                // Path-less write (e.g. a config mutation): no file to snapshot,
                // so ask directly instead of staging.
                Decision::Ask {
                    summary: name.to_string(),
                    preview: None,
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
        ToolCategory::Network => {
            // web_search / web_fetch / web_hook_run are research/network actions.
            // In Write + Commands Auto they run without an approval card to cut
            // friction during exploration. (Mutating web_hook_add/modify/delete
            // are no longer in this category — they are Write/Destructive.)
            if is_auto_research(mode, command_toggle) {
                return Decision::Allow;
            }
            // The tool name is already shown as the card header; the summary
            // carries the concrete target (search query / fetched URL) so the
            // approval card reads e.g. "web_search\nlatest rust async news"
            // instead of "web_search\nweb_search".
            let detail = match name {
                "web_search" => arg_str(args, "query"),
                "web_fetch" => arg_str(args, "url"),
                _ => None,
            };
            let summary = match detail {
                Some(d) if !d.is_empty() => d,
                _ => name.to_string(),
            };
            Decision::Ask {
                summary,
                preview: None,
            }
        }
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

    #[test]
    fn readonly_ask_downgraded_in_write_auto() {
        let mut p = perms();
        // Explicit "ask" rule for a path.
        p.read.insert("secret/**".into(), "ask".into());
        // Write + Commands Auto -> Allow (research shouldn't be interrupted).
        assert!(matches!(
            gate(
                ToolCategory::Readonly,
                "read_file",
                &json!({"path":"secret/key.pem"}),
                "write",
                "auto",
                "ask",
                &p
            ),
            Decision::Allow
        ));
        // Write + Commands Manual -> still Ask.
        assert!(matches!(
            gate(
                ToolCategory::Readonly,
                "read_file",
                &json!({"path":"secret/key.pem"}),
                "write",
                "manual",
                "ask",
                &p
            ),
            Decision::Ask { .. }
        ));
        // Plan mode -> still Ask (plan blocks write/exec, not readonly).
        assert!(matches!(
            gate(
                ToolCategory::Readonly,
                "read_file",
                &json!({"path":"secret/key.pem"}),
                "plan",
                "auto",
                "ask",
                &p
            ),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn network_auto_research_allows() {
        let p = perms();
        // web_search in Write + Auto -> Allow (no approval card).
        assert!(matches!(
            gate(
                ToolCategory::Network,
                "web_search",
                &json!({"query": "latest rust async news"}),
                "write",
                "auto",
                "auto",
                &p
            ),
            Decision::Allow
        ));
        // web_fetch in Write + Auto -> Allow.
        assert!(matches!(
            gate(
                ToolCategory::Network,
                "web_fetch",
                &json!({"url": "https://example.com/page"}),
                "write",
                "auto",
                "auto",
                &p
            ),
            Decision::Allow
        ));
        // web_hook_run in Write + Auto -> Allow (user-configured endpoint).
        assert!(matches!(
            gate(
                ToolCategory::Network,
                "web_hook_run",
                &json!({"name": "deploy"}),
                "write",
                "auto",
                "auto",
                &p
            ),
            Decision::Allow
        ));
        // Write + Commands Manual -> web_search still asks.
        assert!(matches!(
            gate(
                ToolCategory::Network,
                "web_search",
                &json!({"query": "rust"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn web_hook_config_tools_recategorized() {
        let p = perms();
        // web_hook_add/modify are now Write (path-less config mutations):
        // Write + Edits Auto -> Allow; Write + Edits Ask -> Ask (no file to stage).
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "web_hook_add",
                &json!({"name": "x"}),
                "write",
                "auto",
                "auto",
                &p
            ),
            Decision::Allow
        ));
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "web_hook_modify",
                &json!({"name": "x"}),
                "write",
                "manual",
                "ask",
                &p
            ),
            Decision::Ask { .. }
        ));
        // Plan mode blocks Write -> Deny.
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "web_hook_add",
                &json!({"name": "x"}),
                "plan",
                "auto",
                "auto",
                &p
            ),
            Decision::Deny(_)
        ));
        // web_hook_delete is Destructive -> always Ask, even in Write + Auto.
        assert!(matches!(
            gate(
                ToolCategory::Destructive,
                "web_hook_delete",
                &json!({"name": "x"}),
                "write",
                "auto",
                "auto",
                &p
            ),
            Decision::Ask { .. }
        ));
        // Plan mode blocks Destructive -> Deny.
        assert!(matches!(
            gate(
                ToolCategory::Destructive,
                "web_hook_delete",
                &json!({"name": "x"}),
                "plan",
                "auto",
                "auto",
                &p
            ),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn move_path_respects_edit_rules_and_toggle() {
        let p = perms();
        // move_path uses src/dst, not path. Previously the gate saw an empty
        // path and unconditionally Allowed — bypassing deny rules + the toggle.
        // .env* deny on dst -> Deny.
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "move_path",
                &json!({"src": "a.txt", "dst": ".env"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Deny(_)
        ));
        // .env* deny on src -> Deny.
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "move_path",
                &json!({"src": ".env", "dst": "b.txt"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Deny(_)
        ));
        // No deny rule, edit_toggle=ask -> Stage (was silently Allow before).
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "move_path",
                &json!({"src": "a.txt", "dst": "b.txt"}),
                "write",
                "manual",
                "ask",
                &p
            ),
            Decision::Stage
        ));
        // No deny rule, edit_toggle=auto -> Allow.
        assert!(matches!(
            gate(
                ToolCategory::Write,
                "move_path",
                &json!({"src": "a.txt", "dst": "b.txt"}),
                "write",
                "manual",
                "auto",
                &p
            ),
            Decision::Allow
        ));
    }

    #[test]
    fn network_summary_carries_query_and_url() {
        let p = perms();
        // web_search -> summary is the query, not the tool name. Use Commands
        // Manual so the gate still produces an Ask (Write + Auto now allows).
        let d = gate(
            ToolCategory::Network,
            "web_search",
            &json!({"query": "latest rust async news"}),
            "write",
            "manual",
            "auto",
            &p,
        );
        match d {
            Decision::Ask { summary, .. } => {
                assert_eq!(summary, "latest rust async news");
            }
            other => panic!("expected Ask, got {other:?}"),
        }

        // web_fetch -> summary is the URL, not the tool name.
        let d = gate(
            ToolCategory::Network,
            "web_fetch",
            &json!({"url": "https://example.com/page"}),
            "write",
            "manual",
            "auto",
            &p,
        );
        match d {
            Decision::Ask { summary, .. } => {
                assert_eq!(summary, "https://example.com/page");
            }
            other => panic!("expected Ask, got {other:?}"),
        }

        // unknown network tool without detail -> falls back to the tool name.
        let d = gate(
            ToolCategory::Network,
            "some_net_tool",
            &json!({}),
            "write",
            "manual",
            "auto",
            &p,
        );
        match d {
            Decision::Ask { summary, .. } => {
                assert_eq!(summary, "some_net_tool");
            }
            other => panic!("expected Ask, got {other:?}"),
        }
    }
}
