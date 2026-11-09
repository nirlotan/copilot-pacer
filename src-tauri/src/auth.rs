//! Getting a GitHub OAuth token:
//!  1. a token from our own device-flow sign-in, kept in the OS keychain;
//!  2. otherwise, the GitHub CLI's token (`gh auth token`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

/// Client ID of GitHub's Copilot editor integrations (Copilot.vim / JetBrains).
/// Tokens minted for it are accepted by the Copilot entitlement endpoint.
pub const COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

const KEYCHAIN_SERVICE: &str = "copilot-pacer";
const KEYCHAIN_USER: &str = "github-oauth-token";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Device,
    GhCli,
}

// ---------------------------------------------------------------------------
// Keychain
// ---------------------------------------------------------------------------

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|e| e.to_string())
}

pub fn keychain_get() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|t| !t.is_empty())
}

pub fn keychain_set(token: &str) -> Result<(), String> {
    entry()?.set_password(token).map_err(|e| e.to_string())
}

pub fn keychain_delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// GitHub CLI
// ---------------------------------------------------------------------------

/// GUI apps on macOS don't inherit the shell's PATH, so look in the usual
/// install locations as well.
fn gh_candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![PathBuf::from(if cfg!(windows) { "gh.exe" } else { "gh" })];
    if cfg!(target_os = "macos") {
        v.push("/opt/homebrew/bin/gh".into());
        v.push("/usr/local/bin/gh".into());
        v.push("/opt/local/bin/gh".into());
        if let Ok(home) = std::env::var("HOME") {
            v.push(PathBuf::from(home).join(".local/bin/gh"));
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Ok(base) = std::env::var(var) {
                v.push(PathBuf::from(&base).join("GitHub CLI").join("gh.exe"));
                v.push(PathBuf::from(&base).join("Programs").join("GitHub CLI").join("gh.exe"));
            }
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            v.push(PathBuf::from(profile).join("scoop").join("shims").join("gh.exe"));
        }
    }
    v
}

/// Blocking: run from a blocking task.
pub fn gh_token() -> Option<String> {
    for bin in gh_candidates() {
        let mut cmd = Command::new(&bin);
        cmd.args(["auth", "token", "--hostname", "github.com"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(out) = cmd.output() {
            if out.status.success() {
                let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Device flow
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    #[serde(alias = "device_code")]
    pub device_code: String,
    #[serde(alias = "user_code")]
    pub user_code: String,
    #[serde(alias = "verification_uri")]
    pub verification_uri: String,
    #[serde(alias = "expires_in")]
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

pub async fn request_device_code(http: &reqwest::Client) -> Result<DeviceCode, String> {
    let resp = http
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", COPILOT_CLIENT_ID), ("scope", "read:user")])
        .send()
        .await
        .map_err(|e| {
            format!(
                "Could not reach GitHub: {}. {}",
                crate::api::error_chain(&e),
                crate::api::PROXY_HINT
            )
        })?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("GitHub returned HTTP {}: {}", status.as_u16(), text.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| format!("Unexpected device-code response: {e}"))
}

pub enum PollResult {
    Token(String),
    Pending,
    SlowDown,
    Failed(String),
}

pub async fn poll_device_token(http: &reqwest::Client, device_code: &str) -> PollResult {
    let resp = http
        .post(ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", COPILOT_CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await;
    let v: serde_json::Value = match resp {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => return PollResult::Failed(e.to_string()),
        },
        // Transient network hiccup: keep polling.
        Err(_) => return PollResult::Pending,
    };
    if let Some(t) = v.get("access_token").and_then(|t| t.as_str()) {
        return PollResult::Token(t.to_string());
    }
    match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") => PollResult::Pending,
        Some("slow_down") => PollResult::SlowDown,
        Some("expired_token") => PollResult::Failed("The code expired. Please try again.".into()),
        Some("access_denied") => PollResult::Failed("Sign-in was cancelled on GitHub.".into()),
        Some(other) => PollResult::Failed(
            v.get("error_description")
                .and_then(|d| d.as_str())
                .unwrap_or(other)
                .to_string(),
        ),
        None => PollResult::Failed("Unexpected response from GitHub.".into()),
    }
}
