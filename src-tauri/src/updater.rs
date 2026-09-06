use serde::Serialize;

const OWNER: &str = "risubrevis";
const REPO: &str = "aiassistant";
const GH_API: &str = "https://api.github.com";

#[derive(Serialize)]
pub struct CommitEntry {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

/// Whether in-app download+install is supported on this runtime.
/// On Linux it's only supported when the binary was patched with a known bundle
/// type at build time (AppImage/Deb/Rpm). Arch pacman installs and `cargo run`
/// have bundle_type() == None → not supported (user updates via package manager).
/// Windows and macOS always support it.
pub fn install_supported() -> bool {
    if cfg!(target_os = "linux") {
        tauri::utils::platform::bundle_type().is_some()
    } else {
        true
    }
}

/// Fetch the list of commits between two versions from the GitHub compare API.
/// `from`/`to` are version strings like "1.0.0". Tries `v{from}...v{to}` first,
/// then bare `{from}...{to}`. Returns an empty vec when the compare is not found
/// (e.g. a tag is missing); returns Err only on network/parse failures.
pub async fn fetch_changelog(from: &str, to: &str) -> anyhow::Result<Vec<CommitEntry>> {
    let client = crate::net::apply(reqwest::Client::builder())
        .user_agent(concat!("aiassistant/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    for (l, r) in [
        (format!("v{from}"), format!("v{to}")),
        (from.to_string(), to.to_string()),
    ] {
        let url = format!("{GH_API}/repos/{OWNER}/{REPO}/compare/{l}...{r}");
        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        if !resp.status().is_success() {
            anyhow::bail!("github compare returned HTTP {}", resp.status());
        }
        let body: serde_json::Value = resp.json().await?;
        let commits = body["commits"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let sha = c["sha"].as_str().unwrap_or("").to_string();
                        let msg = c["commit"]["message"].as_str().unwrap_or("").to_string();
                        let message = msg.lines().next().unwrap_or("").trim().to_string();
                        let author = c["commit"]["author"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let date = c["commit"]["author"]["date"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        if sha.is_empty() && message.is_empty() {
                            None
                        } else {
                            Some(CommitEntry {
                                sha,
                                message,
                                author,
                                date,
                            })
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(commits);
    }
    Ok(Vec::new())
}
