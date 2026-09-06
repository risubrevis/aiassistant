use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;
use regex::Regex;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{image::Image, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::warn;

#[derive(Debug, Clone, Serialize)]
pub struct McpWebUiEntry {
    pub server_id: String,
    pub title: String,
    pub url: String,
    pub icon_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectResult {
    pub ok: bool,
    pub url: Option<String>,
}

fn base_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("mcpwebui")
}

fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn server_dir(app: &AppHandle, id: &str) -> PathBuf {
    base_dir(app).join(safe_id(id))
}

fn icon_file(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let dir = server_dir(app, id);
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        if e.file_name().to_string_lossy().starts_with("favicon.") {
            return Some(e.path());
        }
    }
    None
}

fn ext_to_mime(ext: &str) -> Option<String> {
    Some(
        match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "ico" => "image/x-icon",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            _ => return None,
        }
        .to_string(),
    )
}

fn ctype_to_ext(ct: &str) -> Option<&'static str> {
    let ct = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match ct.as_str() {
        "image/png" => "png",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => return None,
    })
}

fn url_path_ext(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let file = path.rsplit('/').next()?;
    let (_name, ext) = file.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    if ext.is_empty() || ext.len() > 6 {
        None
    } else {
        Some(ext)
    }
}

/// Download (URL) or copy (local path) the favicon into the cache. Best-effort.
pub async fn cache_favicon(app: &AppHandle, id: &str, source: &str) -> Option<String> {
    if source.trim().is_empty() {
        return None;
    }
    let dir = server_dir(app, id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("mcpwebui: cache dir: {e}");
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("favicon.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    let (bytes, ext): (Vec<u8>, String) = if source.starts_with("http://")
        || source.starts_with("https://")
    {
        let client =
            match crate::net::apply(reqwest::Client::builder().timeout(Duration::from_secs(15)))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    warn!("mcpwebui: http client: {e}");
                    return None;
                }
            };
        let resp = match client.get(source).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("mcpwebui: fetch favicon: {e}");
                return None;
            }
        };
        if !resp.status().is_success() {
            warn!("mcpwebui: favicon status {}", resp.status());
            return None;
        }
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let ext = ctype_to_ext(&ct)
            .map(|s| s.to_string())
            .or_else(|| url_path_ext(source))
            .unwrap_or_else(|| "bin".to_string());
        let bytes = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => {
                warn!("mcpwebui: read bytes: {e}");
                return None;
            }
        };
        (bytes, ext)
    } else {
        let bytes = match std::fs::read(source) {
            Ok(b) => b,
            Err(e) => {
                warn!("mcpwebui: read local favicon: {e}");
                return None;
            }
        };
        let ext = Path::new(source)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| "bin".to_string());
        (bytes, ext)
    };

    let mime = match ext_to_mime(&ext) {
        Some(m) => m,
        None => {
            warn!("mcpwebui: unknown favicon ext {ext}");
            return None;
        }
    };
    let path = dir.join(format!("favicon.{ext}"));
    if let Err(e) = std::fs::write(&path, &bytes) {
        warn!("mcpwebui: write favicon: {e}");
        return None;
    }
    Some(mime)
}

/// Remove only the cached favicon file(s) for a server.
pub fn remove_icon(app: &AppHandle, id: &str) {
    let dir = server_dir(app, id);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("favicon.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Remove the entire per-server cache (icon + persistent web data store).
pub fn remove_server_cache(app: &AppHandle, id: &str) -> Result<()> {
    let dir = server_dir(app, id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("remove {}", dir.display()))?;
    }
    Ok(())
}

pub async fn list_entries(app: &AppHandle, pool: &SqlitePool) -> Result<Vec<McpWebUiEntry>> {
    let rows = crate::db::mcp_servers::list_with_webui(pool).await?;
    let mut out = Vec::new();
    for row in rows {
        let icon = icon_file(app, &row.id).and_then(|p| {
            let ext = p.extension()?.to_str()?.to_string();
            let mime = ext_to_mime(&ext).unwrap_or_else(|| "application/octet-stream".into());
            let bytes = std::fs::read(&p).ok()?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Some(format!("data:{mime};base64,{b64}"))
        });
        out.push(McpWebUiEntry {
            server_id: row.id,
            title: row.title,
            url: row.webui_url,
            icon_data_url: icon,
        });
    }
    Ok(out)
}

fn data_store_id(id: &str) -> [u8; 16] {
    *uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, safe_id(id).as_bytes()).as_bytes()
}

fn decode_icon(path: &Path) -> Option<Image<'static>> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    Some(Image::new_owned(img.into_raw(), w, h))
}

fn loader_script(bg: (u8, u8, u8)) -> String {
    const TPL: &str = r#"(function(){try{if(window.__mcpwebui_loader)return;window.__mcpwebui_loader=true;var s=document.createElement('style');s.textContent='#__mcpwebui_loader{position:fixed;inset:0;z-index:2147483647;display:flex;align-items:center;justify-content:center;background:rgb(__BG__);}#__mcpwebui_loader .sp{width:36px;height:36px;border:3px solid rgba(255,255,255,.18);border-top-color:#6aa9ff;border-radius:50%;animation:__mwu_rot .8s linear infinite}@keyframes __mwu_rot{to{transform:rotate(360deg)}}';(document.head||document.documentElement).appendChild(s);var d=document.createElement('div');d.id='__mcpwebui_loader';d.innerHTML='<div class="sp"></div>';(document.body||document.documentElement).appendChild(d);var rm=function(){try{d.remove();s.remove();}catch(e){}};if(document.readyState==='complete')rm();else window.addEventListener('load',rm,{once:true});setTimeout(rm,12000);}catch(e){}})();"#;
    TPL.replace("__BG__", &format!("{},{},{}", bg.0, bg.1, bg.2))
}

pub fn open_window(
    app: &AppHandle,
    id: &str,
    title: &str,
    url: &str,
    theme: Option<&str>,
) -> Result<()> {
    let label = format!("mcpwebui-{}", safe_id(id));
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let url: reqwest::Url = url
        .parse()
        .with_context(|| format!("invalid webui url: {url}"))?;
    let bg = match theme {
        Some("light") => (255, 255, 255),
        _ => (30, 30, 30),
    };
    let mut builder = WebviewWindowBuilder::new(app, label.clone(), WebviewUrl::External(url))
        .title(title)
        .inner_size(1000.0, 700.0)
        .min_inner_size(480.0, 320.0)
        .resizable(true)
        .decorations(true)
        .center()
        .data_directory(server_dir(app, id).join("webdata"))
        .data_store_identifier(data_store_id(id))
        .initialization_script(loader_script(bg));
    if let Some(p) = icon_file(app, id) {
        if let Some(img) = decode_icon(&p) {
            builder = builder.icon(img).context("set mcpwebui window icon")?;
        }
    }
    builder.build().context("build mcpwebui window")?;
    Ok(())
}

fn link_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)<link\b[^>]*>").unwrap())
}
fn rel_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\brel\s*=\s*["']([^"']*)["']"#).unwrap())
}
fn href_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\bhref\s*=\s*["']([^"']*)["']"#).unwrap())
}

fn origin_of(url: &str) -> Option<String> {
    let scheme = if url.starts_with("https://") {
        "https"
    } else if url.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let rest = &url[scheme.len() + 3..];
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

fn resolve(base: &str, origin: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}:{href}");
    }
    if href.starts_with('/') {
        return format!("{origin}{href}");
    }
    let noq = base.split(['?', '#']).next().unwrap_or(base);
    let dir = match noq.rfind('/') {
        Some(i) => &noq[..=i],
        None => base,
    };
    format!("{dir}{href}")
}

async fn is_image(client: &reqwest::Client, url: &str) -> bool {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !resp.status().is_success() {
        return false;
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    ct.starts_with("image/") || url_path_ext(url).and_then(|e| ext_to_mime(&e)).is_some()
}

async fn discover(client: &reqwest::Client, site_url: &str, origin: &str) -> Option<String> {
    if let Ok(resp) = client.get(site_url).send().await {
        if resp.status().is_success() {
            if let Ok(html) = resp.text().await {
                let rel_re = rel_regex();
                let href_re = href_regex();
                for m in link_regex().find_iter(&html) {
                    let tag = m.as_str();
                    let rel = rel_re
                        .captures(tag)
                        .and_then(|c| c.get(1))
                        .map(|x| x.as_str().to_ascii_lowercase());
                    let href = href_re
                        .captures(tag)
                        .and_then(|c| c.get(1))
                        .map(|x| x.as_str());
                    if let (Some(rel), Some(href)) = (rel, href) {
                        let is_icon = rel
                            .split_whitespace()
                            .any(|t| t == "icon" || t == "apple-touch-icon");
                        if is_icon {
                            let abs = resolve(site_url, origin, href);
                            if is_image(client, &abs).await {
                                return Some(abs);
                            }
                        }
                    }
                }
            }
        }
    }
    let ico = format!("{origin}/favicon.ico");
    if is_image(client, &ico).await {
        return Some(ico);
    }
    None
}

pub async fn detect_favicon(site_url: String, current: Option<String>) -> DetectResult {
    let client = match crate::net::apply(
        reqwest::Client::builder().timeout(Duration::from_secs(10)),
    )
    .build()
    {
        Ok(c) => c,
        Err(_) => {
            return DetectResult {
                ok: false,
                url: None,
            }
        }
    };
    if let Some(cur) = current.as_ref().filter(|c| !c.trim().is_empty()) {
        if is_image(&client, cur).await {
            return DetectResult {
                ok: true,
                url: Some(cur.clone()),
            };
        }
    }
    if let Some(origin) = origin_of(&site_url) {
        if let Some(u) = discover(&client, &site_url, &origin).await {
            return DetectResult {
                ok: true,
                url: Some(u),
            };
        }
    }
    DetectResult {
        ok: false,
        url: None,
    }
}
