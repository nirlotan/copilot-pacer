//! Client for GitHub's Copilot entitlement endpoint.
//!
//! `GET https://api.github.com/copilot_internal/user` is what the Copilot
//! editor extensions call to draw their usage meter. It is undocumented, so
//! parsing is deliberately defensive: every field is optional and we look in a
//! few places for the "credits / premium requests" quota.

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

pub const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

// Headers mirror what the Copilot editor integrations send; the endpoint is
// known to be picky about the editor headers.
const EDITOR_VERSION: &str = "vscode/1.99.0";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const API_VERSION: &str = "2025-04-01";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub used_pct: f64,
    pub unlimited: bool,
    pub entitlement: Option<f64>,
    pub remaining: Option<f64>,
    pub overage_count: Option<f64>,
    pub quota_name: String,
    pub plan: Option<String>,
    pub login: Option<String>,
    /// As reported by GitHub, e.g. "2026-12-01".
    pub github_reset_date: Option<String>,
    pub fetched_at: i64,
}

#[derive(Debug)]
pub enum FetchError {
    /// 401/403/404: the token isn't accepted for this endpoint.
    Unauthorized(u16, String),
    Http(u16, String),
    Network(String),
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Unauthorized(code, body) => {
                write!(f, "GitHub rejected the token (HTTP {code}). {body}")
            }
            FetchError::Http(code, body) => write!(f, "GitHub returned HTTP {code}. {body}"),
            FetchError::Network(e) => write!(f, "Network error: {e}. {PROXY_HINT}"),
            FetchError::Parse(e) => write!(f, "Unexpected response: {e}"),
        }
    }
}

pub const PROXY_HINT: &str =
    "On a corporate network or VPN? Check Settings → Network (try “Detect again” or set the proxy manually).";

/// Full error text including underlying causes (DNS, TLS, connect…), which
/// reqwest's top-level message ("error sending request") hides.
pub fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(cause) = src {
        let msg = cause.to_string();
        if !out.contains(&msg) {
            out.push_str(": ");
            out.push_str(&msg);
        }
        src = cause.source();
    }
    out
}

pub enum ProxyChoice {
    /// reqwest's own detection (environment variables, macOS manual proxy).
    System,
    NoProxy,
    Url(String),
}

pub fn client(choice: &ProxyChoice) -> Result<reqwest::Client, String> {
    let mut b = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10));
    match choice {
        ProxyChoice::System => {}
        ProxyChoice::NoProxy => b = b.no_proxy(),
        ProxyChoice::Url(url) => {
            let p = reqwest::Proxy::all(url.as_str()).map_err(|e| format!("Invalid proxy URL “{url}”: {e}"))?;
            b = b.proxy(p);
        }
    }
    b.build().map_err(|e| error_chain(&e))
}

pub async fn fetch_usage(http: &reqwest::Client, token: &str, now_unix: i64) -> Result<Usage, FetchError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .header("Editor-Version", EDITOR_VERSION)
        .header("Editor-Plugin-Version", EDITOR_PLUGIN_VERSION)
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .map_err(|e| FetchError::Network(error_chain(&e)))?;

    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| FetchError::Network(error_chain(&e)))?;
    if matches!(status, 401 | 403 | 404) {
        return Err(FetchError::Unauthorized(status, short_message(&body)));
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Http(status, short_message(&body)));
    }
    let v: Value = serde_json::from_str(&body).map_err(|e| FetchError::Parse(e.to_string()))?;
    parse_usage(&v, now_unix).map_err(FetchError::Parse)
}

fn short_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(m) = v.get("message").and_then(Value::as_str) {
            return m.to_string();
        }
    }
    body.chars().take(160).collect()
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse().ok()))
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Pick the quota that the editor shows as "Credits" / "Premium requests".
fn pick_quota(snapshots: &serde_json::Map<String, Value>) -> Option<(String, &Value)> {
    const PREFERRED: [&str; 4] = ["premium_interactions", "credits", "ai_credits", "premium_requests"];
    for k in PREFERRED {
        if let Some(q) = snapshots.get(k) {
            return Some((k.to_string(), q));
        }
    }
    if let Some((k, q)) = snapshots
        .iter()
        .find(|(k, _)| k.contains("credit") || k.contains("premium"))
    {
        return Some((k.clone(), q));
    }
    // Last resort: first quota that isn't unlimited.
    snapshots
        .iter()
        .find(|(_, q)| q.get("unlimited").and_then(Value::as_bool) != Some(true))
        .map(|(k, q)| (k.clone(), q))
}

pub fn parse_usage(v: &Value, now_unix: i64) -> Result<Usage, String> {
    let snapshots = v
        .get("quota_snapshots")
        .or_else(|| v.get("quotaSnapshots"))
        .and_then(Value::as_object)
        .ok_or("no quota_snapshots in response (is Copilot enabled for this account?)")?;
    let (name, q) = pick_quota(snapshots).ok_or("no usable quota in quota_snapshots")?;

    let unlimited = q.get("unlimited").and_then(Value::as_bool).unwrap_or(false);
    let entitlement = num(q, "entitlement");
    let remaining = num(q, "remaining").or_else(|| num(q, "quota_remaining"));
    let pct_remaining = num(q, "percent_remaining").or_else(|| num(q, "percentRemaining"));

    let used_pct = if unlimited {
        0.0
    } else if let Some(p) = pct_remaining {
        100.0 - p
    } else if let (Some(e), Some(r)) = (entitlement, remaining) {
        if e > 0.0 {
            (e - r) / e * 100.0
        } else {
            0.0
        }
    } else {
        return Err(format!("quota '{name}' has no percent_remaining"));
    };

    Ok(Usage {
        used_pct: (used_pct * 100.0).round() / 100.0,
        unlimited,
        entitlement,
        remaining,
        overage_count: num(q, "overage_count"),
        quota_name: name,
        plan: s(v, "copilot_plan").or_else(|| s(v, "access_type_sku")),
        login: s(v, "login"),
        github_reset_date: s(v, "quota_reset_date_utc")
            .or_else(|| s(v, "quota_reset_date"))
            .or_else(|| s(v, "limited_user_reset_date")),
        fetched_at: now_unix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_business_response() {
        let v = json!({
            "login": "octo",
            "copilot_plan": "business",
            "quota_reset_date": "2026-12-01",
            "quota_snapshots": {
                "chat": { "unlimited": true, "percent_remaining": 100.0 },
                "completions": { "unlimited": true, "percent_remaining": 100.0 },
                "premium_interactions": {
                    "entitlement": 300, "remaining": 270, "percent_remaining": 90.0,
                    "unlimited": false, "overage_count": 0, "overage_permitted": false
                }
            }
        });
        let u = parse_usage(&v, 1).unwrap();
        assert_eq!(u.used_pct, 10.0);
        assert_eq!(u.quota_name, "premium_interactions");
        assert_eq!(u.plan.as_deref(), Some("business"));
        assert_eq!(u.github_reset_date.as_deref(), Some("2026-12-01"));
    }

    #[test]
    fn derives_percent_from_counts() {
        let v = json!({"quota_snapshots": {"credits": {"entitlement": 200, "remaining": 150}}});
        let u = parse_usage(&v, 1).unwrap();
        assert_eq!(u.used_pct, 25.0);
        assert_eq!(u.quota_name, "credits");
    }

    #[test]
    fn errors_without_snapshots() {
        assert!(parse_usage(&json!({"login": "x"}), 1).is_err());
    }
}
