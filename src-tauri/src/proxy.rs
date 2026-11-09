//! Automatic proxy detection.
//!
//! GUI apps don't inherit the terminal's `HTTPS_PROXY`, and reqwest can't
//! evaluate PAC files, so we work out the proxy for GitHub ourselves:
//!
//! 1. this process's proxy environment variables;
//! 2. macOS: `scutil --proxy` → manual HTTPS proxy, or the PAC file evaluated
//!    with the system JavaScript engine (`osascript -l JavaScript`);
//!    Windows: the OS resolver (`WebRequest.GetSystemWebProxy`), which handles
//!    PAC/WPAD;
//! 3. the proxy variables your login shell sets (macOS/Linux);
//! 4. otherwise connect directly.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::proxy_util::{extract_marked, normalize_proxy_url, parse_pac_result, PacDecision, MARKER};

/// URL used to ask "which proxy for GitHub?".
const TARGET_URL: &str = "https://api.github.com/";
const TARGET_HOST: &str = "api.github.com";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    /// None = connect directly.
    pub url: Option<String>,
    /// Human-readable origin of the decision.
    pub source: String,
}

pub async fn detect() -> Resolved {
    if let Some(url) = env_proxy() {
        return Resolved { url: Some(url), source: "environment variables".into() };
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(r) = macos_detect().await {
            return r;
        }
    }

    #[cfg(windows)]
    {
        if let Some(r) = windows_detect().await {
            return r;
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(url) = blocking(login_shell_proxy).await {
            return Resolved { url: Some(url), source: "your login shell's proxy variables".into() };
        }
    }

    Resolved { url: None, source: "no proxy found, connecting directly".into() }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T> {
    tauri::async_runtime::spawn_blocking(f).await.ok().flatten()
}

fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(|v| v.trim().to_string())
        .find(|v| !v.is_empty())
        .map(|v| normalize_proxy_url(&v))
}

/// Run a command, returning its stdout (whatever the exit status) unless it
/// exceeds `timeout`.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                // Background processes started by shell rc files may keep the
                // pipe open, so don't wait forever for EOF.
                return rx.recv_timeout(Duration::from_secs(2)).ok();
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
}

#[cfg(not(windows))]
fn login_shell_proxy() -> Option<String> {
    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());
    let script = format!(
        "printf '{MARKER}%s{MARKER}' \"${{HTTPS_PROXY:-${{https_proxy:-${{ALL_PROXY:-${{all_proxy:-}}}}}}}}\""
    );
    let mut cmd = Command::new(shell);
    cmd.args(["-ilc", &script]);
    let out = run_with_timeout(cmd, Duration::from_secs(8))?;
    extract_marked(&out).map(|v| normalize_proxy_url(&v))
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
async fn macos_detect() -> Option<Resolved> {
    use crate::proxy_util::{parse_scutil, scutil_decision, SystemProxy};

    let out = blocking(|| {
        let mut cmd = Command::new("/usr/sbin/scutil");
        cmd.arg("--proxy");
        run_with_timeout(cmd, Duration::from_secs(5))
    })
    .await?;

    match scutil_decision(&parse_scutil(&out)) {
        SystemProxy::None => None,
        SystemProxy::Manual(url) => Some(Resolved { url: Some(url), source: "macOS network settings".into() }),
        SystemProxy::Pac(pac_url) => {
            let pac = fetch_pac(&pac_url).await?;
            let path = std::env::temp_dir().join(format!("copilot-pacer-{}.pac", std::process::id()));
            std::fs::write(&path, pac).ok()?;
            let p = path.clone();
            let result = blocking(move || {
                let mut cmd = Command::new("/usr/bin/osascript");
                cmd.args(["-l", "JavaScript", "-e", include_str!("pac_eval.js")])
                    .arg(&p)
                    .args([TARGET_URL, TARGET_HOST]);
                run_with_timeout(cmd, Duration::from_secs(15))
            })
            .await;
            let _ = std::fs::remove_file(&path);
            match parse_pac_result(&result?)? {
                PacDecision::Direct => Some(Resolved { url: None, source: format!("PAC file {pac_url} says direct") }),
                PacDecision::Proxy(url) => Some(Resolved { url: Some(url), source: format!("PAC file {pac_url}") }),
            }
        }
    }
}

#[cfg(target_os = "macos")]
async fn fetch_pac(url: &str) -> Option<String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read_to_string(path).ok();
    }
    // PAC servers are on the local network: always fetch them directly.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok().filter(|t| t.contains("FindProxyForURL"))
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
async fn windows_detect() -> Option<Resolved> {
    let script = format!(
        "$u=[Uri]'{TARGET_URL}'; $p=[System.Net.WebRequest]::GetSystemWebProxy(); \
         $r=$p.GetProxy($u); \
         if ($r -and $r.AbsoluteUri -ne $u.AbsoluteUri) {{ '{MARKER}' + $r.AbsoluteUri + '{MARKER}' }} \
         else {{ '{MARKER}DIRECT{MARKER}' }}"
    );
    let out = blocking(move || {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        run_with_timeout(cmd, Duration::from_secs(15))
    })
    .await?;
    let v = extract_marked(&out)?;
    if v == "DIRECT" {
        return None; // let the login-shell / direct fallbacks apply
    }
    Some(Resolved {
        url: Some(v.trim_end_matches('/').to_string()),
        source: "Windows proxy settings".into(),
    })
}
