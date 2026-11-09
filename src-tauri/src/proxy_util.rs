//! Pure parsing helpers for proxy detection (no I/O, unit-tested).

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacDecision {
    Direct,
    Proxy(String),
}

/// Parse a PAC `FindProxyForURL` result such as
/// `"PROXY proxy.corp:911; DIRECT"`. Returns the first entry we can use.
pub fn parse_pac_result(s: &str) -> Option<PacDecision> {
    for entry in s.split(';') {
        let mut parts = entry.split_whitespace();
        let kind = parts.next().map(str::to_ascii_uppercase);
        let target = parts.next();
        match (kind.as_deref(), target) {
            (Some("DIRECT"), _) => return Some(PacDecision::Direct),
            (Some("PROXY") | Some("HTTP"), Some(t)) => return Some(PacDecision::Proxy(format!("http://{t}"))),
            (Some("HTTPS"), Some(t)) => return Some(PacDecision::Proxy(format!("https://{t}"))),
            (Some("SOCKS") | Some("SOCKS5"), Some(t)) => {
                return Some(PacDecision::Proxy(format!("socks5://{t}")))
            }
            _ => continue, // SOCKS4 or garbage: try the next entry
        }
    }
    None
}

/// Parse the top-level `key : value` pairs of `scutil --proxy` output.
pub fn parse_scutil(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut depth = 0i32;
    for line in text.lines() {
        let line = line.trim();
        if line.ends_with('{') {
            depth += 1;
            // "Key : <array> {" opens a nested block below the top level.
            continue;
        }
        if line == "}" {
            depth -= 1;
            continue;
        }
        if depth != 1 {
            continue;
        }
        if let Some((k, v)) = line.split_once(" : ") {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemProxy {
    /// Explicit HTTPS proxy from network settings.
    Manual(String),
    /// Proxy auto-config file URL.
    Pac(String),
    None,
}

pub fn scutil_decision(map: &HashMap<String, String>) -> SystemProxy {
    let on = |k: &str| map.get(k).map(|v| v == "1").unwrap_or(false);
    if on("HTTPSEnable") {
        if let Some(host) = map.get("HTTPSProxy") {
            let port = map.get("HTTPSPort").map(String::as_str).unwrap_or("443");
            return SystemProxy::Manual(format!("http://{host}:{port}"));
        }
    }
    if on("ProxyAutoConfigEnable") {
        if let Some(url) = map.get("ProxyAutoConfigURLString") {
            return SystemProxy::Pac(url.clone());
        }
    }
    if on("ProxyAutoDiscoveryEnable") {
        return SystemProxy::Pac("http://wpad/wpad.dat".into());
    }
    if on("HTTPEnable") {
        if let Some(host) = map.get("HTTPProxy") {
            let port = map.get("HTTPPort").map(String::as_str).unwrap_or("80");
            return SystemProxy::Manual(format!("http://{host}:{port}"));
        }
    }
    SystemProxy::None
}

pub const MARKER: &str = "__CPPX__";

/// Text between the first pair of markers (shell startup files may print
/// other things to stdout).
pub fn extract_marked(out: &str) -> Option<String> {
    let start = out.find(MARKER)? + MARKER.len();
    let rest = &out[start..];
    let end = rest.find(MARKER)?;
    let v = rest[..end].trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Add a scheme if missing: "proxy:911" → "http://proxy:911".
pub fn normalize_proxy_url(s: &str) -> String {
    let s = s.trim();
    if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// Hide credentials when showing a proxy URL in the UI.
pub fn redact(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(at)) if at > s => format!("{}***@{}", &url[..s + 3], &url[at + 1..]),
        _ => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pac_results() {
        assert_eq!(
            parse_pac_result("PROXY proxy-dmz.corp.com:912; DIRECT"),
            Some(PacDecision::Proxy("http://proxy-dmz.corp.com:912".into()))
        );
        assert_eq!(parse_pac_result("DIRECT"), Some(PacDecision::Direct));
        assert_eq!(parse_pac_result(" direct "), Some(PacDecision::Direct));
        assert_eq!(
            parse_pac_result("SOCKS4 a:1; SOCKS b:1080"),
            Some(PacDecision::Proxy("socks5://b:1080".into()))
        );
        assert_eq!(parse_pac_result("HTTPS secure:443"), Some(PacDecision::Proxy("https://secure:443".into())));
        assert_eq!(parse_pac_result(""), None);
        assert_eq!(parse_pac_result("PROXY"), None);
    }

    const SCUTIL_PAC: &str = "<dictionary> {
  BypassAllowed : 0
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
  }
  FallBackAllowed : 1
  ProxyAutoConfigEnable : 1
  ProxyAutoConfigURLString : http://wpad.corp.com/wpad.dat
  ProxyCaptiveLoginAllowed : 1
}";

    #[test]
    fn scutil_pac() {
        let m = parse_scutil(SCUTIL_PAC);
        assert_eq!(m.get("FallBackAllowed").map(String::as_str), Some("1"));
        assert!(!m.contains_key("0"), "nested array entries are ignored");
        assert_eq!(scutil_decision(&m), SystemProxy::Pac("http://wpad.corp.com/wpad.dat".into()));
    }

    #[test]
    fn scutil_manual_and_none() {
        let m = parse_scutil(
            "<dictionary> {\n  HTTPSEnable : 1\n  HTTPSPort : 8080\n  HTTPSProxy : proxy.example.com\n}",
        );
        assert_eq!(scutil_decision(&m), SystemProxy::Manual("http://proxy.example.com:8080".into()));
        let m = parse_scutil("<dictionary> {\n  HTTPEnable : 0\n}");
        assert_eq!(scutil_decision(&m), SystemProxy::None);
    }

    #[test]
    fn markers() {
        let out = format!("Welcome to zsh!\n{MARKER}http://p:3128{MARKER}\n");
        assert_eq!(extract_marked(&out).as_deref(), Some("http://p:3128"));
        assert_eq!(extract_marked(&format!("{MARKER}{MARKER}")), None);
        assert_eq!(extract_marked("nothing"), None);
    }

    #[test]
    fn urls() {
        assert_eq!(normalize_proxy_url(" proxy:911 "), "http://proxy:911");
        assert_eq!(normalize_proxy_url("socks5://h:1"), "socks5://h:1");
        assert_eq!(redact("http://user:pw@proxy:911"), "http://***@proxy:911");
        assert_eq!(redact("http://proxy:911"), "http://proxy:911");
    }
}
