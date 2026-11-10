//! Version comparison and release-asset selection for update checks
//! (pure, unit-tested).

use std::cmp::Ordering;

/// Parse "v1.2.3", "1.2", "1.2.3-beta.1" into (numbers, is_prerelease).
pub fn parse(v: &str) -> Option<(Vec<u64>, bool)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((c, rest)) => (c, v[c.len()..].starts_with('-') && !rest.is_empty()),
        None => (v, false),
    };
    let nums: Option<Vec<u64>> = core.split('.').map(|p| p.parse().ok()).collect();
    let nums = nums?;
    (!nums.is_empty()).then_some((nums, pre))
}

pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (mut na, pa) = parse(a)?;
    let (mut nb, pb) = parse(b)?;
    let len = na.len().max(nb.len());
    na.resize(len, 0);
    nb.resize(len, 0);
    Some(match na.cmp(&nb) {
        // Same numbers: a release beats its pre-releases.
        Ordering::Equal => pb.cmp(&pa),
        o => o,
    })
}

/// True when `latest` is strictly newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    compare(latest, current) == Some(Ordering::Greater)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Other,
}

pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Other
    }
}

/// Pick the installer for this platform from a release's asset names.
/// Returns the index into `names`.
pub fn pick_asset(names: &[&str], platform: Platform) -> Option<usize> {
    let lower: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
    let find = |pred: &dyn Fn(&str) -> bool| lower.iter().position(|n| pred(n));
    match platform {
        Platform::MacOs => find(&|n| n.ends_with(".dmg") && n.contains("macos"))
            .or_else(|| find(&|n| n.ends_with(".dmg") && n.contains("universal")))
            .or_else(|| find(&|n| n.ends_with(".dmg"))),
        Platform::Windows => find(&|n| n.ends_with("setup.exe"))
            .or_else(|| find(&|n| n.ends_with(".exe")))
            .or_else(|| find(&|n| n.ends_with(".msi"))),
        Platform::Other => None,
    }
}

/// "Copilot-Pacer-macOS.dmg" + "0.2.0" → "Copilot-Pacer-macOS-0.2.0.dmg", so a
/// download of an older version is never mistaken for the new one.
pub fn versioned_file_name(name: &str, version: &str) -> String {
    if name.contains(version) {
        return name.to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{version}.{ext}"),
        _ => format!("{name}-{version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse("v1.2.3"), Some((vec![1, 2, 3], false)));
        assert_eq!(parse("0.1"), Some((vec![0, 1], false)));
        assert_eq!(parse("1.0.0-beta.2"), Some((vec![1, 0, 0], true)));
        assert_eq!(parse("1.0.0+build5"), Some((vec![1, 0, 0], false)));
        assert_eq!(parse("latest"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn newer() {
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn assets() {
        let names = [
            "Copilot-Pacer-Windows.msi",
            "Copilot-Pacer-macOS.dmg",
            "Copilot-Pacer-Windows-Setup.exe",
        ];
        assert_eq!(pick_asset(&names, Platform::MacOs), Some(1));
        assert_eq!(pick_asset(&names, Platform::Windows), Some(2));
        assert_eq!(pick_asset(&["a.msi"], Platform::Windows), Some(0));
        assert_eq!(pick_asset(&["Copilot Pacer_0.1.0_universal.dmg"], Platform::MacOs), Some(0));
        assert_eq!(pick_asset(&["notes.txt"], Platform::MacOs), None);
        assert_eq!(pick_asset(&names, Platform::Other), None);
    }

    #[test]
    fn file_names() {
        assert_eq!(versioned_file_name("Copilot-Pacer-macOS.dmg", "0.2.0"), "Copilot-Pacer-macOS-0.2.0.dmg");
        assert_eq!(versioned_file_name("Copilot Pacer_0.2.0_universal.dmg", "0.2.0"), "Copilot Pacer_0.2.0_universal.dmg");
        assert_eq!(versioned_file_name("installer", "1.0"), "installer-1.0");
    }
}
