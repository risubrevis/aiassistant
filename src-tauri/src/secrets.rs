use anyhow::Result;
use tracing::warn;

/// OS keychain entry name for a provider API key (docs/01, docs/09).
fn entry(ref_id: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("aiassistant", &format!("provider:{ref_id}")).ok()
}

/// Read a provider API key from the OS keychain. Returns `None` if no key is
/// stored or the keychain is unavailable (e.g. headless / no daemon). For
/// keyless endpoints (Ollama) this is the expected path.
pub fn get_api_key(ref_id: &str) -> Option<String> {
    let entry = match entry(ref_id) {
        Some(e) => e,
        None => {
            warn!("keychain unavailable for provider:{ref_id}");
            return None;
        }
    };
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read keyring for provider:{ref_id}: {e}");
            None
        }
    }
}

pub fn set_api_key(ref_id: &str, key: &str) -> Result<()> {
    let entry = entry(ref_id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    entry.set_password(key)?;
    Ok(())
}

pub fn delete_api_key(ref_id: &str) -> Result<()> {
    let entry = entry(ref_id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// OS keychain entry for the outbound proxy password (Settings → Network).
fn proxy_entry() -> Option<keyring::Entry> {
    keyring::Entry::new("aiassistant", "network:proxy").ok()
}

pub fn get_proxy_password() -> Option<String> {
    let entry = match proxy_entry() {
        Some(e) => e,
        None => {
            warn!("keychain unavailable for network:proxy");
            return None;
        }
    };
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read keyring for network:proxy: {e}");
            None
        }
    }
}

pub fn set_proxy_password(pw: &str) -> Result<()> {
    let entry = proxy_entry().ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    entry.set_password(pw)?;
    Ok(())
}

pub fn delete_proxy_password() -> Result<()> {
    let entry = proxy_entry().ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// OS keychain entry for a web hook secret (Settings → Web Hooks).
fn webhook_entry(id: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("aiassistant", &format!("webhook:{id}")).ok()
}

pub fn get_webhook_secret(id: &str) -> Option<String> {
    let entry = match webhook_entry(id) {
        Some(e) => e,
        None => {
            warn!("keychain unavailable for webhook:{id}");
            return None;
        }
    };
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read keyring for webhook:{id}: {e}");
            None
        }
    }
}

pub fn set_webhook_secret(id: &str, secret: &str) -> Result<()> {
    let entry = webhook_entry(id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    entry.set_password(secret)?;
    Ok(())
}

pub fn delete_webhook_secret(id: &str) -> Result<()> {
    let entry = webhook_entry(id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// OS keychain entry for a web search provider API key (Settings → Web Search).
fn web_search_entry(id: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("aiassistant", &format!("websearch:{id}")).ok()
}

pub fn get_web_search_api_key(id: &str) -> Option<String> {
    let entry = match web_search_entry(id) {
        Some(e) => e,
        None => {
            warn!("keychain unavailable for websearch:{id}");
            return None;
        }
    };
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read keyring for websearch:{id}: {e}");
            None
        }
    }
}

pub fn set_web_search_api_key(id: &str, key: &str) -> Result<()> {
    let entry = web_search_entry(id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    entry.set_password(key)?;
    Ok(())
}

pub fn delete_web_search_api_key(id: &str) -> Result<()> {
    let entry = web_search_entry(id).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// OS keychain entry for an MCP OAuth token (`kind` is "access" or "refresh").
fn mcp_oauth_entry(server_id: &str, kind: &str) -> Option<keyring::Entry> {
    keyring::Entry::new("aiassistant", &format!("mcp-oauth:{server_id}:{kind}")).ok()
}

pub fn get_mcp_oauth_token(server_id: &str, kind: &str) -> Option<String> {
    let entry = match mcp_oauth_entry(server_id, kind) {
        Some(e) => e,
        None => {
            warn!("keychain unavailable for mcp-oauth:{server_id}:{kind}");
            return None;
        }
    };
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read keyring for mcp-oauth:{server_id}:{kind}: {e}");
            None
        }
    }
}

pub fn set_mcp_oauth_token(server_id: &str, kind: &str, token: &str) -> Result<()> {
    let entry =
        mcp_oauth_entry(server_id, kind).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    entry.set_password(token)?;
    Ok(())
}

pub fn delete_mcp_oauth_token(server_id: &str, kind: &str) -> Result<()> {
    let entry =
        mcp_oauth_entry(server_id, kind).ok_or_else(|| anyhow::anyhow!("keychain unavailable"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}
