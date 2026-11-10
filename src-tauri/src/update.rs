//! Checking GitHub Releases for a newer version and downloading its installer.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::version::{current_platform, pick_asset};

/// Repository whose releases are checked.
pub const REPO: &str = "nirlotan/copilot-pacer";

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub page_url: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: u64,
}

pub const UNREACHABLE: &str = "Couldn't reach GitHub";

pub async fn latest(http: &reqwest::Client) -> Result<Release, String> {
    let resp = http
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| format!("{UNREACHABLE}: {}", crate::api::error_chain(&e)))?;
    let status = resp.status().as_u16();
    match status {
        200 => {}
        404 => {
            return Err(format!(
                "No published release found on github.com/{REPO}. The repository must be public and have at least one release."
            ))
        }
        403 | 429 => return Err("GitHub's rate limit was reached. Try again in a while.".into()),
        _ => return Err(format!("GitHub returned HTTP {status} when checking for updates.")),
    }
    let r: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("Unexpected response from GitHub: {e}"))?;
    let names: Vec<&str> = r.assets.iter().map(|a| a.name.as_str()).collect();
    let asset = pick_asset(&names, current_platform()).map(|i| &r.assets[i]);
    Ok(Release {
        version: r.tag_name.trim().trim_start_matches(['v', 'V']).to_string(),
        page_url: r.html_url,
        asset_name: asset.map(|a| a.name.clone()),
        asset_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size).unwrap_or(0),
    })
}

/// Stream `url` to `dest` (via a temporary ".part" file), reporting
/// (bytes so far, total if known).
pub async fn download(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let mut resp = http
        .get(url)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", crate::api::error_chain(&e)))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length();
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let mut file = std::fs::File::create(&part).map_err(|e| format!("Can't write {}: {e}", part.display()))?;
    let mut done: u64 = 0;
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(format!("Download interrupted: {}", crate::api::error_chain(&e)));
            }
        };
        if let Err(e) = file.write_all(&chunk) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("Can't write {}: {e}", part.display()));
        }
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if let Some(t) = total {
        if done != t {
            let _ = std::fs::remove_file(&part);
            return Err(format!("Download incomplete ({done} of {t} bytes)."));
        }
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Can't save {}: {e}", dest.display()))
}
