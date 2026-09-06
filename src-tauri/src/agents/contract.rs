use super::AgentTask;
use crate::config::AgentContract;

/// Substitute placeholders in one argv template element (docs/07).
/// Returns None when the element must be dropped entirely
/// ({session} occurs but the task has no session).
fn substitute(template: &str, task: &AgentTask) -> Option<String> {
    if template.contains("{session}") {
        let session = task.session.as_deref()?;
        return Some(template.replace("{session}", session));
    }
    Some(
        template
            .replace("{prompt}", &task.prompt)
            .replace("{cwd}", &task.cwd),
    )
}

/// Build the full argv (program + args) for an agent run (docs/07).
/// Placeholders {prompt}/{cwd}/{session}/{model} are substituted; mode flags
/// are appended per our chat mode, then the resume flag if resuming.
pub fn build_command(contract: &AgentContract, task: &AgentTask) -> Vec<String> {
    if contract.command.is_empty() {
        return Vec::new();
    }
    // {model} resolves to the agent's configured model (Settings → Agent), or
    // "default" when unset. The user places it in args (e.g. `-m {model}`).
    let model = if contract.default_model.is_empty() {
        "default"
    } else {
        &contract.default_model
    };
    let mut argv = vec![contract.command.clone()];
    for template in &contract.args {
        if let Some(arg) = substitute(template, task) {
            argv.push(arg.replace("{model}", model));
        }
    }
    if let Some(flags) = contract.mode_flags.get(&task.mode) {
        for template in flags {
            if let Some(arg) = substitute(template, task) {
                argv.push(arg.replace("{model}", model));
            }
        }
    }
    // "none" is the docs/07 marker for "no resume support".
    if !contract.resume_flag.is_empty() && contract.resume_flag != "none" {
        if let Some(session) = &task.session {
            for token in contract.resume_flag.split_whitespace() {
                argv.push(token.replace("{session}", session));
            }
        }
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn base_contract() -> AgentContract {
        AgentContract {
            id: "test".into(),
            name: "Test".into(),
            description: String::new(),
            capabilities: Vec::new(),
            kind: "subprocess".into(),
            command: "agent-cli".into(),
            args: vec![
                "run".into(),
                "--format".into(),
                "json".into(),
                "{prompt}".into(),
            ],
            prompt_mode: "arg".into(),
            cwd: String::new(),
            env: HashMap::new(),
            output_format: "ndjson".into(),
            event_schema: None,
            mode_flags: HashMap::new(),
            resume_flag: String::new(),
            timeout_ms: 300_000,
            max_turns: 50,
            default_model: String::new(),
        }
    }

    #[test]
    fn base_args_and_mode_flags() {
        let mut contract = base_contract();
        contract
            .mode_flags
            .insert("plan".into(), vec!["--plan".into()]);
        let task = AgentTask::new("fix the bug", "/tmp/proj");
        assert_eq!(
            build_command(&contract, &task),
            vec![
                "agent-cli",
                "run",
                "--format",
                "json",
                "fix the bug",
                "--plan"
            ]
        );
    }

    #[test]
    fn unknown_mode_adds_no_flags() {
        let mut contract = base_contract();
        contract
            .mode_flags
            .insert("plan".into(), vec!["--plan".into()]);
        let mut task = AgentTask::new("p", "/tmp");
        task.mode = "auto".into();
        assert_eq!(
            build_command(&contract, &task),
            vec!["agent-cli", "run", "--format", "json", "p"]
        );
    }

    #[test]
    fn empty_command_yields_empty_argv() {
        let mut contract = base_contract();
        contract.command.clear();
        let task = AgentTask::new("p", "/tmp");
        assert!(build_command(&contract, &task).is_empty());
    }

    #[test]
    fn cwd_and_model_placeholders() {
        let mut contract = base_contract();
        contract.args = vec![
            "--cwd".into(),
            "{cwd}".into(),
            "--model".into(),
            "{model}".into(),
        ];
        let task = AgentTask::new("p", "/tmp/proj");
        assert_eq!(
            build_command(&contract, &task),
            vec!["agent-cli", "--cwd", "/tmp/proj", "--model", "default"]
        );
    }

    #[test]
    fn session_element_dropped_without_session() {
        let mut contract = base_contract();
        contract.args = vec!["--session={session}".into(), "{prompt}".into()];
        let task = AgentTask::new("p", "/tmp");
        assert_eq!(build_command(&contract, &task), vec!["agent-cli", "p"]);
    }

    #[test]
    fn resume_flag_appended_with_session() {
        let mut contract = base_contract();
        contract.resume_flag = "--id {session}".into();
        let mut task = AgentTask::new("p", "/tmp");
        task.session = Some("abc".into());
        assert_eq!(
            build_command(&contract, &task),
            vec!["agent-cli", "run", "--format", "json", "p", "--id", "abc"]
        );
    }

    #[test]
    fn resume_flag_skipped_without_session() {
        let mut contract = base_contract();
        contract.resume_flag = "--id {session}".into();
        let task = AgentTask::new("p", "/tmp");
        assert_eq!(
            build_command(&contract, &task),
            vec!["agent-cli", "run", "--format", "json", "p"]
        );
    }

    #[test]
    fn resume_none_marker_ignored() {
        let mut contract = base_contract();
        contract.resume_flag = "none".into();
        let mut task = AgentTask::new("p", "/tmp");
        task.session = Some("abc".into());
        assert_eq!(
            build_command(&contract, &task),
            vec!["agent-cli", "run", "--format", "json", "p"]
        );
    }

    #[test]
    fn full_pipeline_order() {
        let mut contract = base_contract();
        contract.args = vec!["{prompt}".into()];
        contract
            .mode_flags
            .insert("write".into(), vec!["--cwd-{model}".into()]);
        contract.resume_flag = "--resume --session {session}".into();
        let mut task = AgentTask::new("hello world", "/tmp");
        task.mode = "write".into();
        task.session = Some("s1".into());
        assert_eq!(
            build_command(&contract, &task),
            vec![
                "agent-cli",
                "hello world",
                "--cwd-default",
                "--resume",
                "--session",
                "s1"
            ]
        );
    }
}
