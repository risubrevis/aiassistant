use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::config_path;
use super::model::ModelRef;

/// Atomically write `contents` to `path` (temp file + rename in the same dir).
fn write_file_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir {}", parent.display()))?;
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, contents)
        .with_context(|| format!("failed to write to {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Atomically write a TOML document back to config.toml (temp file + rename),
/// preserving the user's comments and formatting outside the edited sections.
fn write_doc(doc: toml_edit::DocumentMut) -> Result<()> {
    write_file_atomic(&config_path(), &doc.to_string())
}

fn load_doc() -> Result<toml_edit::DocumentMut> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    raw.parse()
        .with_context(|| format!("failed to parse config at {}", path.display()))
}

/// Serialize `items` as `[[field]]` and return the parsed array item.
#[allow(dead_code)]
fn array_item<T: Serialize>(field: &str, items: &[T]) -> Result<toml_edit::Item> {
    let mut map: BTreeMap<&str, &[T]> = BTreeMap::new();
    map.insert(field, items);
    let s = toml::to_string(&map).context("failed to serialize config section")?;
    let sub: toml_edit::DocumentMut = s.parse().context("failed to parse serialized section")?;
    Ok(sub[field].clone())
}

/// Set or remove an `Option<ModelRef>` field under `[defaults]`
/// (main_model / secondary_model / embedding_model). `None` removes the key.
pub fn write_defaults_model_ref(field: &str, mr: Option<&ModelRef>) -> Result<()> {
    let mut doc = load_doc()?;
    let defaults = doc
        .entry("defaults")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .context("[defaults] is not a table")?;
    match mr {
        Some(mr) => {
            let mut map: BTreeMap<&str, &ModelRef> = BTreeMap::new();
            map.insert(field, mr);
            let s = toml::to_string(&map)?;
            let sub: toml_edit::DocumentMut = s.parse()?;
            defaults.insert(field, sub[field].clone());
        }
        None => {
            defaults.remove(field);
        }
    }
    write_doc(doc)
}

/// Set a string field under `[defaults]` (mode / command_toggle / edit_toggle).
pub fn write_defaults_field(field: &str, value: &str) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"][field] = toml_edit::value(value);
    write_doc(doc)
}

/// Write the system prompt to `system_prompt.md` next to config.toml.
pub fn write_system_prompt(value: &str) -> Result<()> {
    write_file_atomic(&super::system_prompt_path(), value)
}

/// Write the user-editable environment info to `environment.md` next to config.toml.
pub fn write_environment_info(value: &str) -> Result<()> {
    write_file_atomic(&super::environment_path(), value)
}

/// Set `[defaults].mode` (minimal | plan | write).
pub fn write_mode(mode: &str) -> Result<()> {
    write_defaults_field("mode", mode)
}

/// Set `[defaults].auto_collapse_context_pct` (context compaction threshold; 0 = disabled).
pub fn write_auto_collapse_context_pct(value: u32) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["auto_collapse_context_pct"] = toml_edit::value(value as i64);
    write_doc(doc)
}

/// Set `[defaults].max_turns` (max LLM tool-call round-trips per turn).
pub fn write_max_turns(value: u32) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["max_turns"] = toml_edit::value(value as i64);
    write_doc(doc)
}

/// Set `[defaults].auto_pull_changes` (inject changed project files at turn start).
pub fn write_auto_pull_changes(value: bool) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["auto_pull_changes"] = toml_edit::value(value);
    write_doc(doc)
}

/// Set `[defaults].delete_to_trash` (delete_path: OS trash vs permanent removal).
pub fn write_delete_to_trash(value: bool) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["delete_to_trash"] = toml_edit::value(value);
    write_doc(doc)
}

/// Set `[defaults].add_environment_info` (inject env info into the system prompt).
pub fn write_add_environment_info(value: bool) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["add_environment_info"] = toml_edit::value(value);
    write_doc(doc)
}

/// Set `[defaults].rag_enabled` (master switch for RAG / embedding retrieval).
pub fn write_rag_enabled(value: bool) -> Result<()> {
    let mut doc = load_doc()?;
    doc["defaults"]["rag_enabled"] = toml_edit::value(value);
    write_doc(doc)
}

/// Set a string field under `[logging]` (level / file_level).
pub fn write_logging_field(field: &str, value: &str) -> Result<()> {
    let mut doc = load_doc()?;
    doc["logging"][field] = toml_edit::value(value);
    write_doc(doc)
}

/// Replace the whole `[network]` section in config.toml (proxy password is
/// stored in the OS keychain, never written here).
pub fn write_network(cfg: &super::Network) -> Result<()> {
    let mut doc = load_doc()?;
    let map: BTreeMap<&str, &super::Network> = BTreeMap::from([("network", cfg)]);
    let s = toml::to_string(&map).context("failed to serialize [network]")?;
    let sub: toml_edit::DocumentMut = s.parse().context("failed to parse serialized [network]")?;
    doc["network"] = sub["network"].clone();
    write_doc(doc)
}

/// Replace the whole `[web_search]` section in config.toml.
pub fn write_web_search(cfg: &super::WebSearch) -> Result<()> {
    let mut doc = load_doc()?;
    let map: BTreeMap<&str, &super::WebSearch> = BTreeMap::from([("web_search", cfg)]);
    let s = toml::to_string(&map).context("failed to serialize [web_search]")?;
    let sub: toml_edit::DocumentMut = s
        .parse()
        .context("failed to parse serialized [web_search]")?;
    doc["web_search"] = sub["web_search"].clone();
    write_doc(doc)
}

/// Replace the `[defaults].disabled_tools` array in config.toml.
pub fn write_disabled_tools(tools: &[String]) -> Result<()> {
    let mut doc = load_doc()?;
    let mut arr = toml_edit::Array::new();
    for t in tools {
        arr.push(t.as_str());
    }
    doc["defaults"]["disabled_tools"] = toml_edit::Item::Value(arr.into());
    write_doc(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{config_path, load};

    fn backup() -> Option<std::path::PathBuf> {
        let p = config_path();
        if p.exists() {
            let bak = p.with_extension("toml.bak");
            std::fs::copy(&p, &bak).ok()?;
            Some(bak)
        } else {
            None
        }
    }
    fn restore(bak: Option<std::path::PathBuf>) {
        let p = config_path();
        if let Some(b) = bak {
            let _ = std::fs::copy(&b, &p);
            let _ = std::fs::remove_file(&b);
        } else {
            let _ = std::fs::remove_file(&p);
        }
    }

    #[test]
    #[ignore = "writes to the real config dir (backed up)"]
    fn roundtrip_multiline_system_prompt() {
        let bak = backup();
        let _ = crate::config::write_default_config(&config_path());
        let multiline = "line one\nline two with \"quotes\"\n\nline four";
        super::write_system_prompt(multiline).unwrap();
        let cfg = load().unwrap();
        assert_eq!(cfg.defaults.system_prompt, multiline);
        let md = std::fs::read_to_string(crate::config::system_prompt_path()).unwrap();
        assert!(md.contains(multiline));
        // also ensure re-reading the raw file parses (no corruption)
        let raw = std::fs::read_to_string(config_path()).unwrap();
        let _reparsed: crate::config::Config = toml::from_str(&raw).unwrap();
        restore(bak);
    }

    #[test]
    #[ignore = "writes to the real config dir (backed up)"]
    fn roundtrip_defaults_model_ref() {
        let bak = backup();
        // start from a clean default so the file exists
        let _ = crate::config::write_default_config(&config_path());
        write_defaults_model_ref(
            "main_model",
            Some(&ModelRef {
                provider: "prov-uuid".into(),
                model: "model-uuid".into(),
            }),
        )
        .unwrap();
        let cfg = load().unwrap();
        assert_eq!(
            cfg.defaults.main_model.as_ref().unwrap().model,
            "model-uuid"
        );
        restore(bak);
    }

    #[test]
    #[ignore = "writes to the real config dir (backed up)"]
    fn roundtrip_web_search() {
        let bak = backup();
        let _ = crate::config::write_default_config(&config_path());
        let ws = crate::config::WebSearch {
            user_agent: "TestUA/1.0".into(),
            accept_language: "de-DE,de;q=0.9".into(),
            extra_headers: "X-Test: yes\n# comment\nBadLine\n\nX-Other: no".into(),
            timeout_ms: 12345,
        };
        super::write_web_search(&ws).unwrap();
        let cfg = load().unwrap();
        assert_eq!(cfg.web_search.user_agent, "TestUA/1.0");
        assert_eq!(cfg.web_search.accept_language, "de-DE,de;q=0.9");
        assert_eq!(
            cfg.web_search.extra_headers,
            "X-Test: yes\n# comment\nBadLine\n\nX-Other: no"
        );
        assert_eq!(cfg.web_search.timeout_ms, 12345);
        // re-reading the raw file parses (no corruption)
        let raw = std::fs::read_to_string(config_path()).unwrap();
        let _reparsed: crate::config::Config = toml::from_str(&raw).unwrap();
        restore(bak);
    }
}
