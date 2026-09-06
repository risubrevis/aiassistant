use serde_json::{Map, Value};

use crate::config::EventSchema;

use super::AgentEvent;

/// Parse one stdout line into a normalized agent event (docs/07).
/// format: "stream_json" (claude-code) | "ndjson" | "raw_text".
pub fn parse_line(format: &str, line: &str, schema: Option<&EventSchema>) -> Option<AgentEvent> {
    match format {
        "stream_json" => {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            parse_stream_json(line)
        }
        "ndjson" => {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            parse_ndjson(line, schema)
        }
        // raw_text: every stdout line is part of the answer. Keep indentation
        // and blank lines intact; the bridge concatenates the Text events.
        _ => Some(AgentEvent::Text {
            text: format!("{line}\n"),
        }),
    }
}

/// Extract a resume/session id from an output line, if present.
pub fn extract_session(format: &str, line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let obj = value.as_object()?;
    let keys: &[&str] = match format {
        "stream_json" => &["session_id"],
        "ndjson" => &["session_id", "session", "id"],
        _ => return None,
    };
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_stream_json(line: &str) -> Option<AgentEvent> {
    let obj = parse_object(line)?;
    match obj.get("type").and_then(Value::as_str)? {
        "system" => {
            if obj.get("subtype").and_then(Value::as_str) == Some("init") {
                Some(AgentEvent::Progress {
                    text: "init".into(),
                })
            } else {
                None
            }
        }
        "assistant" => parse_assistant_content(&obj),
        "result" => {
            let subtype = obj.get("subtype").and_then(Value::as_str).unwrap_or("");
            let result = obj.get("result").and_then(Value::as_str).unwrap_or("");
            match subtype {
                "success" => Some(AgentEvent::Text {
                    text: result.into(),
                }),
                s if s.starts_with("error") => Some(AgentEvent::Error {
                    text: if result.is_empty() {
                        s.into()
                    } else {
                        result.into()
                    },
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn parse_assistant_content(obj: &Map<String, Value>) -> Option<AgentEvent> {
    let blocks = obj.get("message")?.get("content")?.as_array()?;
    let tool_use = blocks.iter().find_map(|block| {
        let obj = block.as_object()?;
        if obj.get("type").and_then(Value::as_str) != Some("tool_use") {
            return None;
        }
        let name = obj.get("name").and_then(Value::as_str)?.to_string();
        let input = obj
            .get("input")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        Some((name, input))
    });
    if let Some((name, input)) = tool_use {
        let args = serde_json::to_string(&input).unwrap_or_default();
        return Some(AgentEvent::ToolAction { name, args });
    }
    join_text_blocks(blocks)
        .filter(|t| !t.is_empty())
        .map(|text| AgentEvent::Text { text })
}

fn join_text_blocks(blocks: &[Value]) -> Option<String> {
    let parts: Vec<&str> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn parse_ndjson(line: &str, schema: Option<&EventSchema>) -> Option<AgentEvent> {
    let obj = parse_object(line)?;
    match schema {
        Some(schema) => parse_with_schema(&obj, schema),
        None => parse_ndjson_default(&obj),
    }
}

fn parse_with_schema(obj: &Map<String, Value>, schema: &EventSchema) -> Option<AgentEvent> {
    if let Some(key) = &schema.text_key {
        if let Some(text) = obj.get(key).and_then(Value::as_str) {
            return Some(AgentEvent::Text { text: text.into() });
        }
    }
    if let Some(key) = &schema.diff_key {
        if let Some(patch) = obj.get(key) {
            let path = first_string(obj, &["path", "file"]).unwrap_or_default();
            return Some(AgentEvent::Diff {
                path,
                patch: json_string(patch),
            });
        }
    }
    if let Some(key) = &schema.command_key {
        if let Some(args) = obj.get(key) {
            return Some(AgentEvent::ToolAction {
                name: "command".into(),
                args: json_string(args),
            });
        }
    }
    None
}

fn parse_ndjson_default(obj: &Map<String, Value>) -> Option<AgentEvent> {
    let kind = obj.get("type").and_then(Value::as_str)?;
    match kind {
        "message" | "text" | "output" => {
            first_string(obj, &["text", "content", "message"]).map(|text| AgentEvent::Text { text })
        }
        "tool" | "tool_use" | "action" => {
            let name = obj.get("name").and_then(Value::as_str)?;
            let args = obj
                .get("input")
                .or_else(|| obj.get("args"))
                .map(json_string)
                .unwrap_or_default();
            Some(AgentEvent::ToolAction {
                name: name.into(),
                args,
            })
        }
        "diff" | "patch" => {
            let patch = ["patch", "diff"].iter().find_map(|k| obj.get(*k))?;
            let path = first_string(obj, &["path", "file"]).unwrap_or_default();
            Some(AgentEvent::Diff {
                path,
                patch: json_string(patch),
            })
        }
        "progress" | "status" => {
            let text = first_string(obj, &["text", "message", "status"])?;
            Some(AgentEvent::Progress { text })
        }
        "error" => {
            let text = first_string(obj, &["error", "message"]).unwrap_or_default();
            Some(AgentEvent::Error { text })
        }
        "session" | "init" => {
            let text = first_string(obj, &["text", "message", "status"])
                .or_else(|| first_string(obj, &["session_id", "session", "id"]))
                .unwrap_or_default();
            Some(AgentEvent::Progress { text })
        }
        _ => None,
    }
}

fn parse_object(line: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(line).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn first_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

fn json_string(value: &Value) -> String {
    match value.as_str() {
        Some(s) => s.to_string(),
        None => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(a: Option<&str>, b: Option<&str>, c: Option<&str>) -> EventSchema {
        EventSchema {
            text_key: a.map(str::to_string),
            diff_key: b.map(str::to_string),
            command_key: c.map(str::to_string),
        }
    }

    // AgentEvent derives neither PartialEq nor a public clause for it, so
    // tests destructure instead of assert_eq on the enum.

    fn assert_text(ev: Option<AgentEvent>, expected: &str) {
        match ev {
            Some(AgentEvent::Text { text }) => assert_eq!(text, expected),
            other => panic!("expected Text({expected:?}), got {other:?}"),
        }
    }

    fn assert_tool(ev: Option<AgentEvent>, name: &str, args: &str) {
        match ev {
            Some(AgentEvent::ToolAction { name: n, args: a }) => {
                assert_eq!(n, name);
                assert_eq!(a, args);
            }
            other => panic!("expected ToolAction({name:?}), got {other:?}"),
        }
    }

    fn assert_diff(ev: Option<AgentEvent>, path: &str, patch: &str) {
        match ev {
            Some(AgentEvent::Diff { path: p, patch: a }) => {
                assert_eq!(p, path);
                assert_eq!(a, patch);
            }
            other => panic!("expected Diff({path:?}), got {other:?}"),
        }
    }

    fn assert_progress(ev: Option<AgentEvent>, text: &str) {
        match ev {
            Some(AgentEvent::Progress { text: t }) => assert_eq!(t, text),
            other => panic!("expected Progress({text:?}), got {other:?}"),
        }
    }

    fn assert_error(ev: Option<AgentEvent>, text: &str) {
        match ev {
            Some(AgentEvent::Error { text: t }) => assert_eq!(t, text),
            other => panic!("expected Error({text:?}), got {other:?}"),
        }
    }

    #[test]
    fn stream_json_init_yields_progress_and_session() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/tmp","session_id":"s-123"}"#;
        assert_progress(parse_line("stream_json", line, None), "init");
        assert_eq!(extract_session("stream_json", line), Some("s-123".into()));
    }

    #[test]
    fn stream_json_assistant_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        assert_text(parse_line("stream_json", line, None), "hello");
    }

    #[test]
    fn stream_json_multiple_text_blocks_joined() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"part one"},
            {"type":"text","text":"part two"}]}}"#;
        assert_text(
            parse_line("stream_json", line, None),
            "part one\n\npart two",
        );
    }

    #[test]
    fn stream_json_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#;
        assert_tool(
            parse_line("stream_json", line, None),
            "Bash",
            r#"{"command":"ls"}"#,
        );
    }

    #[test]
    fn stream_json_result_success() {
        let line = r#"{"type":"result","subtype":"success","result":"done!",
            "num_turns":3,"total_cost_usd":0.05}"#;
        assert_text(parse_line("stream_json", line, None), "done!");
        assert_eq!(extract_session("stream_json", line), None);
    }

    #[test]
    fn stream_json_error_max_turns() {
        let line = r#"{"type":"result","subtype":"error_max_turns"}"#;
        assert_error(parse_line("stream_json", line, None), "error_max_turns");
    }

    #[test]
    fn stream_json_unknown_and_non_json_ignored() {
        assert!(parse_line("stream_json", "{\"type\":\"other\"}", None).is_none());
        assert!(parse_line("stream_json", "plain text", None).is_none());
        assert!(parse_line("stream_json", "   \n", None).is_none());
        assert_eq!(extract_session("stream_json", "not json"), None);
    }

    #[test]
    fn ndjson_schema_text() {
        let sch = schema(Some("message"), None, None);
        assert_text(
            parse_line("ndjson", "{\"id\":1,\"message\":\"hello\"}", Some(&sch)),
            "hello",
        );
    }

    #[test]
    fn ndjson_schema_diff() {
        let sch = schema(None, Some("patch"), None);
        let line = r#"{"file":"a.rs","patch":"--- a\n+++ b"}"#;
        assert_diff(
            parse_line("ndjson", line, Some(&sch)),
            "a.rs",
            "--- a\n+++ b",
        );
    }

    #[test]
    fn ndjson_schema_command() {
        let sch = schema(None, None, Some("cmd"));
        let line = r#"{"cmd":{"argv":["ls","-la"]}}"#;
        assert_tool(
            parse_line("ndjson", line, Some(&sch)),
            "command",
            r#"{"argv":["ls","-la"]}"#,
        );
    }

    #[test]
    fn ndjson_default_text_and_tool() {
        let line = r#"{"type":"text","text":"hi"}"#;
        assert_text(parse_line("ndjson", line, None), "hi");
        let line = r#"{"type":"tool","name":"Bash","args":{"command":"ls"}}"#;
        assert_tool(
            parse_line("ndjson", line, None),
            "Bash",
            r#"{"command":"ls"}"#,
        );
    }

    #[test]
    fn ndjson_default_diff_progress_error() {
        let line = r#"{"type":"diff","path":"x.rs","patch":"p"}"#;
        assert_diff(parse_line("ndjson", line, None), "x.rs", "p");
        let line = r#"{"type":"progress","message":"50%"}"#;
        assert_progress(parse_line("ndjson", line, None), "50%");
        let line = r#"{"type":"error","error":"boom"}"#;
        assert_error(parse_line("ndjson", line, None), "boom");
    }

    #[test]
    fn ndjson_default_session_extract() {
        let line = r#"{"type":"session","id":"sess-9"}"#;
        let ev = parse_line("ndjson", line, None).expect("progress event");
        assert!(matches!(ev, AgentEvent::Progress { .. }));
        assert_eq!(extract_session("ndjson", line), Some("sess-9".into()));
    }

    #[test]
    fn ndjson_non_json_ignored() {
        assert!(parse_line("ndjson", "plain stdout text", None).is_none());
        assert!(extract_session("ndjson", "[]").is_none());
        assert!(extract_session("ndjson", "{\"type\":\"tool\"}").is_none());
        assert!(parse_line("ndjson", "{\"id\":42}", None).is_none());
    }

    #[test]
    fn raw_text_passes_lines_through_as_text() {
        // Each line becomes a Text event (with a trailing newline) so the bridge
        // can reconstruct the full stdout; indentation is preserved.
        assert_text(parse_line("raw_text", "any text", None), "any text\n");
        assert_text(parse_line("raw_text", "  indented", None), "  indented\n");
        assert_text(parse_line("raw_text", "", None), "\n");
        // A line that happens to be JSON is still treated as raw text.
        assert_text(
            parse_line("raw_text", "{\"type\":\"text\",\"text\":\"x\"}", None),
            "{\"type\":\"text\",\"text\":\"x\"}\n",
        );
        // Session extraction is a no-op for raw_text / unknown formats.
        assert!(extract_session("raw_text", "{\"session_id\":\"s\"}").is_none());
        assert!(extract_session("weird_format", "{\"session_id\":\"s\"}").is_none());
    }
}
