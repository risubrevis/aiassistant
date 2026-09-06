use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::config::{Config, Provider};
use crate::providers::http_client;

/// Embed a batch of texts with the configured embedding model. Returns one
/// vector per input, in order. Errors when no embedding model is configured.
pub async fn embed(pool: &SqlitePool, cfg: &Config, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    let mr = cfg
        .defaults
        .embedding_model
        .as_ref()
        .filter(|m| !m.provider.is_empty() && !m.model.is_empty())
        .context("no embedding model configured")?;
    let pcfg = crate::db::providers::get_provider(pool, &mr.provider)
        .await?
        .context("no provider for embedding model")?
        .to_config_provider();
    // `mr.model` is a provider_models UUID; fall back to the raw value for
    // legacy configs that still store an API model name.
    let model = match crate::db::providers::get_model(pool, &mr.model).await? {
        Some(m) => m.name,
        None => mr.model.clone(),
    };
    let client = build_client(&pcfg);
    match pcfg.kind.as_str() {
        "ollama" => embed_ollama(&client, &pcfg.base_url, &model, texts).await,
        "openai" | "custom" => embed_openai(&client, &pcfg.base_url, &model, texts).await,
        other => anyhow::bail!("embedding not supported for provider kind '{other}'"),
    }
}

fn build_client(pcfg: &Provider) -> reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    if !pcfg.api_key_ref.is_empty() {
        if let Some(key) = crate::secrets::get_api_key(&pcfg.api_key_ref) {
            if !key.is_empty() {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {key}")) {
                    headers.insert(AUTHORIZATION, v);
                }
            }
        }
    }
    for (k, v) in &pcfg.extra_headers {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, val);
        }
    }
    http_client(Duration::from_millis(pcfg.timeout_ms), headers)
}

async fn embed_ollama(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    #[derive(Deserialize)]
    struct Resp {
        embeddings: Vec<Vec<f32>>,
    }
    let origin = crate::providers::ollama_origin(base_url).unwrap_or_else(|| {
        base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string()
    });
    let body = serde_json::json!({ "model": model, "input": texts });
    let resp = client
        .post(format!("{origin}/api/embed"))
        .json(&body)
        .send()
        .await
        .context("embedding request failed")?;
    let status = resp.status();
    let parsed: Resp = resp
        .json()
        .await
        .with_context(|| format!("embedding parse failed (status {status})"))?;
    if parsed.embeddings.len() != texts.len() {
        anyhow::bail!(
            "embedding count mismatch: got {} for {} inputs",
            parsed.embeddings.len(),
            texts.len()
        );
    }
    Ok(parsed.embeddings)
}

async fn embed_openai(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    #[derive(Deserialize)]
    struct Data {
        embedding: Vec<f32>,
    }
    #[derive(Deserialize)]
    struct Resp {
        data: Vec<Data>,
    }
    let body = serde_json::json!({ "model": model, "input": texts });
    let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("embedding request failed")?;
    let status = resp.status();
    let parsed: Resp = resp
        .json()
        .await
        .with_context(|| format!("embedding parse failed (status {status})"))?;
    if parsed.data.len() != texts.len() {
        anyhow::bail!(
            "embedding count mismatch: got {} for {} inputs",
            parsed.data.len(),
            texts.len()
        );
    }
    Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
}

/// Chunk plain text into ~`CHUNK_CHARS`-char pieces with `OVERLAP` overlap,
/// snapped to UTF-8 char boundaries.
pub fn chunk_text(text: &str) -> Vec<String> {
    const CHUNK_CHARS: usize = 1200;
    const OVERLAP: usize = 200;
    let s = text.trim();
    if s.is_empty() {
        return vec![];
    }
    if s.len() <= CHUNK_CHARS {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < s.len() {
        let mut end = (start + CHUNK_CHARS).min(s.len());
        while end < s.len() && !s.is_char_boundary(end) {
            end += 1;
        }
        out.push(s[start..end].to_string());
        if end >= s.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP);
        while start < s.len() && !s.is_char_boundary(start) {
            start += 1;
        }
        if start >= end {
            start = end;
        }
    }
    out
}

/// Whether a file extension is text-like and worth indexing.
pub fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rst"
            | "rs"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "pyi"
            | "go"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cxx"
            | "java"
            | "kt"
            | "rb"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "ps1"
            | "yml"
            | "yaml"
            | "toml"
            | "ini"
            | "cfg"
            | "conf"
            | "json"
            | "json5"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "less"
            | "svelte"
            | "vue"
            | "php"
            | "pl"
            | "lua"
            | "r"
            | "swift"
            | "dart"
            | "sql"
            | "graphql"
            | "gql"
            | "proto"
            | "csv"
            | "tsv"
            | "log"
            | "env"
    )
}

/// Whether a MIME type is text-like.
pub fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/x-yaml"
                | "application/yaml"
                | "application/toml"
                | "application/javascript"
                | "application/x-sh"
        )
}
