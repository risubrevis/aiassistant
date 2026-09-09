//! OAuth 2.0 authorization for HTTP-based MCP servers: server metadata discovery
//! (RFC 8414 / RFC 9728), dynamic client registration (RFC 7591), PKCE
//! authorization-code flow with a local loopback callback, token exchange,
//! refresh and revocation.

use std::time::Duration;

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use reqwest::header::{ACCEPT, WWW_AUTHENTICATE};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

const CLIENT_NAME: &str = "AIAssistant";
const CLIENT_URI: &str = "https://github.com/risubrevis/aiassistant";

const CALLBACK_SUCCESS_HTML: &str =
    "<html><body><h2>Authentication successful</h2><p>You can close this tab and return to AIAssistant.</p></body></html>";

fn callback_error_html(message: &str) -> String {
    format!("<html><body><h2>Authentication failed</h2><p>{message}</p></body></html>")
}

/// Authorization server metadata (subset of RFC 8414 we need).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// Protected resource metadata (RFC 9728) — only the field we consume.
#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    #[serde(default)]
    authorization_servers: Vec<String>,
}

/// Client registration result from DCR (RFC 7591).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistration {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Token set returned by the token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Token expiry in milliseconds since unix epoch. 0 = unknown.
    #[serde(default)]
    pub expires_at: i64,
}

/// Holds state during an active OAuth authorization flow.
#[derive(Clone)]
pub struct OAuthSession {
    pub auth_server_metadata: AuthServerMetadata,
    pub client_registration: ClientRegistration,
    pub code_verifier: String,
    pub state: String,
    pub redirect_uri: String,
}

/// Discover the authorization server behind an MCP URL: probe the endpoint with
/// an unauthenticated initialize request, then follow the 401 → protected
/// resource metadata → authorization server metadata chain.
pub async fn discover(mcp_url: &str) -> Result<AuthServerMetadata> {
    let client = http_client()?;
    let resp = client
        .post(mcp_url)
        .header(ACCEPT, "application/json, text/event-stream")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "aiassistant",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }))
        .send()
        .await?;

    if resp.status() != StatusCode::UNAUTHORIZED {
        return Err(anyhow!(
            "MCP server did not request authentication (HTTP {})",
            resp.status()
        ));
    }

    let mut resource_urls: Vec<String> = Vec::new();
    if let Some(header) = resp
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(url) = parse_www_authenticate(header) {
            resource_urls.push(url);
        }
    }
    resource_urls.extend(well_known_resource_urls(mcp_url));

    let mut authorization_servers: Vec<String> = Vec::new();
    let mut last_err = String::from("no candidate URLs");
    for url in &resource_urls {
        match fetch_authorization_servers(&client, url).await {
            Ok(servers) => {
                authorization_servers = servers;
                break;
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    if authorization_servers.is_empty() {
        return Err(anyhow!("protected resource metadata not found: {last_err}"));
    }

    for issuer in &authorization_servers {
        match fetch_auth_server_metadata(&client, issuer).await {
            Ok(metadata) => return Ok(metadata),
            Err(e) => warn!("oauth: metadata discovery for '{issuer}' failed: {e}"),
        }
    }
    Err(anyhow!(
        "no usable authorization server metadata among {} candidate(s)",
        authorization_servers.len()
    ))
}

async fn fetch_authorization_servers(client: &reqwest::Client, url: &str) -> Result<Vec<String>> {
    let resp = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {}", resp.status()));
    }
    let meta: ProtectedResourceMetadata = resp.json().await?;
    if meta.authorization_servers.is_empty() {
        return Err(anyhow!("no authorization_servers listed"));
    }
    Ok(meta.authorization_servers)
}

async fn fetch_auth_server_metadata(
    client: &reqwest::Client,
    issuer: &str,
) -> Result<AuthServerMetadata> {
    let mut last_err = String::from("no candidate URLs");
    for url in well_known_as_urls(issuer) {
        let resp = client
            .get(url.as_str())
            .header(ACCEPT, "application/json")
            .send()
            .await;
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_err = format!("HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        let Ok(metadata) = resp.json::<AuthServerMetadata>().await else {
            last_err = "invalid metadata document".into();
            continue;
        };
        if metadata.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(anyhow!(
                "issuer mismatch: metadata '{}' vs '{issuer}'",
                metadata.issuer
            ));
        }
        if !metadata
            .code_challenge_methods_supported
            .iter()
            .any(|m| m.eq_ignore_ascii_case("S256"))
        {
            return Err(anyhow!("authorization server does not support S256 PKCE"));
        }
        return Ok(metadata);
    }
    Err(anyhow!(
        "no authorization server metadata at well-known endpoints ({last_err})"
    ))
}

/// Dynamic Client Registration (RFC 7591) as a public native app.
pub async fn register_client(
    registration_endpoint: &str,
    redirect_uris: &[String],
) -> Result<ClientRegistration> {
    let client = http_client()?;
    let resp = client
        .post(registration_endpoint)
        .header(ACCEPT, "application/json")
        .json(&serde_json::json!({
            "client_name": CLIENT_NAME,
            "client_uri": CLIENT_URI,
            "application_type": "native",
            "grant_types": ["authorization_code"],
            "response_types": ["code"],
            "redirect_uris": redirect_uris,
            "token_endpoint_auth_method": "none"
        }))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "client registration failed: {}",
            error_detail(resp).await
        ));
    }
    Ok(resp.json().await?)
}

/// Build the session state for a new authorization flow.
pub fn create_session(
    metadata: AuthServerMetadata,
    registration: ClientRegistration,
    redirect_port: u16,
) -> OAuthSession {
    OAuthSession {
        auth_server_metadata: metadata,
        client_registration: registration,
        code_verifier: generate_code_verifier(),
        state: generate_state(),
        redirect_uri: format!("http://127.0.0.1:{redirect_port}/callback"),
    }
}

/// Build the authorization URL the user must open in a browser.
pub fn build_auth_url(session: &OAuthSession, resource: &str, scopes: &str) -> String {
    let challenge = generate_pkce_challenge(&session.code_verifier);
    let mut pairs: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", session.client_registration.client_id.as_str()),
        ("redirect_uri", session.redirect_uri.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", session.state.as_str()),
    ];
    if !resource.is_empty() {
        pairs.push(("resource", resource));
    }
    if !scopes.is_empty() {
        pairs.push(("scope", scopes));
    }
    let base = &session.auth_server_metadata.authorization_endpoint;
    match reqwest::Url::parse_with_params(base, pairs) {
        Ok(url) => url.to_string(),
        Err(_) => base.clone(),
    }
}

/// Wait (up to `timeout`) for the browser redirect carrying the authorization
/// code. Returns `(code, state)`.
pub async fn wait_for_callback(
    port: u16,
    expected_state: &str,
    timeout: Duration,
) -> Result<(String, String)> {
    tokio::time::timeout(timeout, listen_for_callback(port, expected_state))
        .await
        .map_err(|_| anyhow!("timed out waiting for the OAuth callback"))?
}

async fn listen_for_callback(port: u16, expected_state: &str) -> Result<(String, String)> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let request = read_request(&mut stream).await;
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default();
        let url = reqwest::Url::parse(&absolute_target(target, port));
        let param = |name: &str| -> Option<String> {
            url.as_ref()
                .ok()?
                .query_pairs()
                .into_iter()
                .find(|(k, _)| k.as_ref() == name)
                .map(|(_, v)| v.into_owned())
        };

        if let Some(err) = param("error").or_else(|| param("error_description")) {
            let _ = write_response(&mut stream, 400, &callback_error_html(&err)).await;
            return Err(anyhow!("authorization server error: {err}"));
        }
        let Some(code) = param("code") else {
            // Stray request (favicon, probing) — keep waiting for the redirect.
            let _ = write_response(&mut stream, 404, "Not an OAuth callback").await;
            continue;
        };
        if param("state").as_deref() != Some(expected_state) {
            let _ = write_response(&mut stream, 400, &callback_error_html("state mismatch")).await;
            return Err(anyhow!("OAuth callback state mismatch"));
        }
        let _ = write_response(&mut stream, 200, CALLBACK_SUCCESS_HTML).await;
        return Ok((code, expected_state.to_string()));
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

async fn write_response(stream: &mut tokio::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn absolute_target(target: &str, port: u16) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("http://127.0.0.1:{port}{target}")
    }
}

/// Authorization code + PKCE exchange (RFC 6749 §4.1.3 / RFC 7636 §4.6).
pub async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenSet> {
    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ];
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }
    token_request(token_endpoint, &params).await
}

pub async fn refresh_access_token(
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> Result<TokenSet> {
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];
    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }
    token_request(token_endpoint, &params).await
}

async fn token_request(token_endpoint: &str, params: &[(&str, &str)]) -> Result<TokenSet> {
    let client = http_client()?;
    let resp = client.post(token_endpoint).form(params).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "token request failed: {}",
            error_detail(resp).await
        ));
    }
    let raw: serde_json::Value = resp.json().await?;
    let access_token = raw
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("token response missing access_token"))?
        .to_string();
    let refresh_token = raw
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let expires_at = raw
        .get("expires_in")
        .and_then(|v| v.as_f64())
        .map(|secs| now_ms() + (secs * 1000.0) as i64)
        .unwrap_or(0);
    Ok(TokenSet {
        access_token,
        refresh_token,
        expires_at,
    })
}

pub async fn find_free_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// RFC 7009 token revocation. Callers treat failures as non-fatal.
pub async fn revoke_token(
    revocation_endpoint: &str,
    token: &str,
    token_type_hint: &str,
) -> Result<()> {
    let client = http_client()?;
    let mut params = vec![("token", token)];
    if !token_type_hint.is_empty() {
        params.push(("token_type_hint", token_type_hint));
    }
    let resp = client
        .post(revocation_endpoint)
        .form(&params)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "token revocation failed: {}",
            error_detail(resp).await
        ));
    }
    Ok(())
}

/// RFC 6749 §5.2 error body if present, else a truncated raw body.
async fn error_detail(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        let error = v.get("error").and_then(|v| v.as_str()).unwrap_or_default();
        if !error.is_empty() {
            let description = v
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            return if description.is_empty() {
                format!("HTTP {status}: {error}")
            } else {
                format!("HTTP {status}: {error} ({description})")
            };
        }
    }
    let truncated: String = body.chars().take(200).collect();
    format!("HTTP {status}: {truncated}")
}

fn http_client() -> Result<reqwest::Client> {
    Ok(crate::net::apply(reqwest::Client::builder().timeout(HTTP_TIMEOUT)).build()?)
}

/// 48 random bytes (3× UUIDv4) base64url-encoded — 64 chars, within the
/// RFC 7636 43-128 range.
fn generate_code_verifier() -> String {
    let mut bytes = Vec::with_capacity(48);
    for _ in 0..3 {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn generate_state() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Extract `resource_metadata="URL"` from a WWW-Authenticate header value.
fn parse_www_authenticate(header: &str) -> Option<String> {
    const KEY: &str = "resource_metadata=";
    let idx = header.to_ascii_lowercase().find(KEY)?;
    let rest = header[idx + KEY.len()..].trim_start();
    if let Some(unquoted) = rest.strip_prefix('"') {
        let end = unquoted.find('"')?;
        Some(unquoted[..end].to_string())
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn well_known_resource_urls(mcp_url: &str) -> Vec<String> {
    let mut urls = Vec::new();
    if let Ok(url) = reqwest::Url::parse(mcp_url) {
        let origin = origin_of(&url);
        let path = path_of(&url);
        if !path.is_empty() {
            urls.push(format!(
                "{origin}/.well-known/oauth-protected-resource{path}"
            ));
        }
        urls.push(format!("{origin}/.well-known/oauth-protected-resource"));
    }
    urls
}

fn well_known_as_urls(issuer: &str) -> Vec<String> {
    let mut urls = Vec::new();
    if let Ok(url) = reqwest::Url::parse(issuer) {
        let origin = origin_of(&url);
        let path = path_of(&url);
        if !path.is_empty() {
            urls.push(format!(
                "{origin}/.well-known/oauth-authorization-server{path}"
            ));
            urls.push(format!("{origin}/.well-known/openid-configuration{path}"));
            urls.push(format!("{origin}{path}/.well-known/openid-configuration"));
        }
        urls.push(format!("{origin}/.well-known/oauth-authorization-server"));
        urls.push(format!("{origin}/.well-known/openid-configuration"));
    }
    urls
}

fn origin_of(url: &reqwest::Url) -> String {
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!(
        "{}://{}{port}",
        url.scheme(),
        url.host_str().unwrap_or_default()
    )
}

/// URL path without the trailing slash ("" for the root).
fn path_of(url: &reqwest::Url) -> String {
    let path = url.path();
    if path == "/" || path.is_empty() {
        String::new()
    } else {
        path.trim_end_matches('/').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_verifier_length() {
        let verifier = generate_code_verifier();
        assert!(
            (43..=128).contains(&verifier.len()),
            "unexpected verifier length: {}",
            verifier.len()
        );
        assert!(!verifier.contains('+') && !verifier.contains('/') && !verifier.contains('='));
    }

    #[test]
    fn pkce_challenge_rfc7636_vector() {
        assert_eq!(
            generate_pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn www_authenticate_parsing() {
        assert_eq!(
            parse_www_authenticate(
                "Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\""
            )
            .as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource")
        );
        assert_eq!(parse_www_authenticate("Bearer realm=\"x\""), None);
    }

    #[test]
    fn well_known_urls() {
        assert_eq!(
            well_known_as_urls("https://auth.example.com"),
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            well_known_as_urls("https://auth.example.com/tenant1"),
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            well_known_resource_urls("https://mcp.example.com/mcp"),
            vec![
                "https://mcp.example.com/.well-known/oauth-protected-resource/mcp",
                "https://mcp.example.com/.well-known/oauth-protected-resource",
            ]
        );
    }

    #[test]
    fn auth_url_params() {
        let metadata = AuthServerMetadata {
            issuer: "https://as.example.com".into(),
            authorization_endpoint: "https://as.example.com/authorize".into(),
            token_endpoint: "https://as.example.com/token".into(),
            registration_endpoint: None,
            revocation_endpoint: None,
            code_challenge_methods_supported: vec!["S256".into()],
            scopes_supported: vec![],
        };
        let registration = ClientRegistration {
            client_id: "client-123".into(),
            client_secret: None,
        };
        let session = create_session(metadata, registration, 8765);
        let url = reqwest::Url::parse(&build_auth_url(
            &session,
            "https://mcp.example.com",
            "read write",
        ))
        .unwrap();
        let param = |name: &str| -> Option<String> {
            url.query_pairs()
                .into_iter()
                .find(|(k, _)| k.as_ref() == name)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(param("response_type").as_deref(), Some("code"));
        assert_eq!(param("client_id").as_deref(), Some("client-123"));
        assert_eq!(
            param("redirect_uri").as_deref(),
            Some("http://127.0.0.1:8765/callback")
        );
        assert_eq!(param("code_challenge_method").as_deref(), Some("S256"));
        assert_eq!(
            param("code_challenge").as_deref(),
            Some(generate_pkce_challenge(&session.code_verifier).as_str())
        );
        assert_eq!(param("state").as_deref(), Some(session.state.as_str()));
        assert_eq!(
            param("resource").as_deref(),
            Some("https://mcp.example.com")
        );
        assert_eq!(param("scope").as_deref(), Some("read write"));
    }
}
