use std::sync::RwLock;
use std::time::Duration;

use serde::Serialize;

use crate::config::Network;
use crate::secrets;

/// Process-global snapshot of the network config, read at client-build time.
/// Set at startup and on config hot-reload. reqwest captures proxy settings at
/// build() time, so callers must rebuild clients to pick up changes — which all
/// current call sites already do (per-turn / per-connect / per-call).
static NET: RwLock<Option<Network>> = RwLock::new(None);

pub fn init(cfg: &Network) {
    if let Ok(mut g) = NET.write() {
        *g = Some(cfg.clone());
    }
}

fn snapshot() -> Network {
    NET.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkTestResult {
    pub ok: bool,
    pub detail: String,
    pub elapsed_ms: u64,
    pub status: Option<u16>,
}

/// Apply the global network/proxy settings to a client builder. Chain before
/// `.build()`. When the proxy is disabled the builder is returned unchanged so
/// reqwest's `system-proxy` feature (env vars) is respected as a fallback.
pub fn apply(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    apply_with(builder, &snapshot(), None)
}

/// Apply a specific network config (used by the Test button with unsaved form
/// values). `password` overrides the keychain lookup when `Some`.
pub fn apply_with(
    mut builder: reqwest::ClientBuilder,
    net: &Network,
    password: Option<&str>,
) -> reqwest::ClientBuilder {
    if !net.verify_tls {
        builder = builder.danger_accept_invalid_certs(true);
    }
    let ca = net.ca_cert_path.trim();
    if !ca.is_empty() {
        if let Some(cert) = load_ca_cert(ca) {
            builder = builder.add_root_certificate(cert);
        } else {
            tracing::warn!("net: failed to load CA cert at {ca}");
        }
    }
    if net.proxy_enabled && !net.proxy_host.trim().is_empty() && net.proxy_port > 0 {
        let url = format!(
            "{}://{}:{}",
            net.proxy_type,
            net.proxy_host.trim(),
            net.proxy_port
        );
        match reqwest::Proxy::all(&url) {
            Ok(mut proxy) => {
                let user = net.proxy_username.trim();
                if !user.is_empty() {
                    let pass = match password {
                        Some(p) => p.to_string(),
                        None => secrets::get_proxy_password().unwrap_or_default(),
                    };
                    proxy = proxy.basic_auth(user, &pass);
                }
                let np = net.no_proxy.trim();
                if !np.is_empty() {
                    if let Some(no_proxy) = reqwest::NoProxy::from_string(np) {
                        proxy = proxy.no_proxy(Some(no_proxy));
                    }
                }
                // Disable the automatic system (env-var) proxy so our explicit
                // proxy + bypass list is the sole routing policy.
                builder = builder.no_proxy().proxy(proxy);
            }
            Err(e) => tracing::warn!("net: invalid proxy URL {url}: {e}"),
        }
    }
    builder
}

fn load_ca_cert(path: &str) -> Option<reqwest::Certificate> {
    let bytes = std::fs::read(path).ok()?;
    reqwest::Certificate::from_pem(&bytes)
        .or_else(|_| reqwest::Certificate::from_der(&bytes))
        .ok()
}

/// Probe `net.test_url` through a client built from `net`. Used by the Test
/// button with the current (possibly unsaved) form values.
pub async fn test_connection(net: &Network, password: Option<&str>) -> NetworkTestResult {
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(net.connect_timeout_ms.max(1000));
    let builder = apply_with(reqwest::Client::builder().timeout(timeout), net, password);
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => {
            return NetworkTestResult {
                ok: false,
                detail: format!("client build failed: {e}"),
                elapsed_ms: start.elapsed().as_millis() as u64,
                status: None,
            }
        }
    };
    let url = {
        let u = net.test_url.trim();
        if u.is_empty() {
            "https://www.google.com"
        } else {
            u
        }
    };
    match client.get(url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let ok = resp.status().is_success() || resp.status().is_redirection();
            NetworkTestResult {
                ok,
                detail: format!("HTTP {status}"),
                elapsed_ms: start.elapsed().as_millis() as u64,
                status: Some(status),
            }
        }
        Err(e) => NetworkTestResult {
            ok: false,
            detail: crate::providers::reqwest_error_detail(&e),
            elapsed_ms: start.elapsed().as_millis() as u64,
            status: None,
        },
    }
}
