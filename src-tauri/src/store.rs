//! Settings and usage-history persistence (JSON files in the app config dir).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pace::{parse_hhmm, parse_ymd, PaceConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// False until the UI has written first-run defaults.
    pub initialized: bool,
    /// Day of month the cycle resets (1–31).
    pub reset_day: u32,
    /// 0 = Sunday … 6 = Saturday.
    pub work_days: [bool; 7],
    /// "HH:MM"
    pub work_start: String,
    /// "HH:MM"
    pub work_end: String,
    /// Holiday calendars, e.g. "IL", "GB-ENG", "US-CA".
    pub holiday_regions: Vec<String>,
    /// date-holidays types to count as days off.
    pub holiday_types: Vec<String>,
    /// Personal days off, "YYYY-MM-DD".
    pub custom_days_off: Vec<String>,
    /// Derived by the UI from regions + custom days; cached so the tray is
    /// correct before the webview has loaded.
    pub excluded_days: Vec<String>,
    pub refresh_minutes: u32,
    pub notify_over_pace: bool,
    /// Percentage points above the healthy line before we call it "over".
    pub over_margin: f64,
    /// "auto" (detect from the system), "manual" (use `proxy_url`) or "none".
    pub proxy_mode: String,
    /// Proxy for manual mode, e.g. "http://proxy.corp:8080".
    pub proxy_url: String,
    /// Show "10%" next to the menu-bar icon (macOS only).
    pub show_percent_in_menu_bar: bool,
    /// macOS: show a Dock icon. Windows: show a taskbar button while open.
    pub show_in_dock: bool,
    /// The one-time "launch at login?" prompt has been answered.
    pub launch_prompt_done: bool,
    /// Check GitHub for new versions daily and download them.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            initialized: false,
            reset_day: 1,
            work_days: [false, true, true, true, true, true, false],
            work_start: "09:00".into(),
            work_end: "18:00".into(),
            holiday_regions: vec![],
            holiday_types: vec!["public".into(), "bank".into()],
            custom_days_off: vec![],
            excluded_days: vec![],
            refresh_minutes: 10,
            notify_over_pace: true,
            over_margin: 5.0,
            proxy_mode: "auto".into(),
            proxy_url: String::new(),
            show_percent_in_menu_bar: true,
            show_in_dock: false,
            launch_prompt_done: false,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn validate(&mut self) -> Result<(), String> {
        if !(1..=31).contains(&self.reset_day) {
            return Err("Reset day must be between 1 and 31".into());
        }
        let s = parse_hhmm(&self.work_start).ok_or("Work start must look like 09:00")?;
        let e = parse_hhmm(&self.work_end).ok_or("Work end must look like 18:00")?;
        if e <= s {
            return Err("Work end must be after work start".into());
        }
        self.refresh_minutes = self.refresh_minutes.clamp(1, 24 * 60);
        if !self.over_margin.is_finite() {
            self.over_margin = 5.0;
        }
        self.over_margin = self.over_margin.clamp(0.0, 100.0);
        self.proxy_url = self.proxy_url.trim().to_string();
        if !matches!(self.proxy_mode.as_str(), "auto" | "manual" | "none") {
            self.proxy_mode = "auto".into();
        }
        if self.proxy_mode == "manual" {
            if self.proxy_url.is_empty() {
                return Err("Enter a proxy URL, or choose Automatic".into());
            }
            self.proxy_url = crate::proxy_util::normalize_proxy_url(&self.proxy_url);
        }
        Ok(())
    }

    pub fn pace_config(&self) -> PaceConfig {
        let d = PaceConfig::default();
        PaceConfig {
            reset_day: self.reset_day.clamp(1, 31),
            work_days: self.work_days,
            work_start_min: parse_hhmm(&self.work_start).unwrap_or(d.work_start_min),
            work_end_min: parse_hhmm(&self.work_end).unwrap_or(d.work_end_min),
            excluded: self
                .excluded_days
                .iter()
                .chain(self.custom_days_off.iter())
                .filter_map(|s| parse_ymd(s))
                .collect(),
        }
    }
}

/// One usage observation: (unix seconds, percent used).
pub type Snapshot = (i64, f64);

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        Store { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
        let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        // Write-then-rename so a crash never leaves a half-written file.
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn load_settings(&self) -> Settings {
        Self::read_json(&self.path("settings.json")).unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), String> {
        Self::write_json(&self.path("settings.json"), s)
    }

    pub fn load_history(&self) -> Vec<Snapshot> {
        Self::read_json(&self.path("history.json")).unwrap_or_default()
    }

    pub fn save_history(&self, h: &[Snapshot]) -> Result<(), String> {
        Self::write_json(&self.path("history.json"), &h)
    }
}

/// Append a snapshot, compressing runs of identical values and pruning old
/// data. Keeps roughly the last 120 days.
pub fn push_snapshot(history: &mut Vec<Snapshot>, ts: i64, used: f64) {
    const KEEP_SECS: i64 = 120 * 24 * 3600;
    history.retain(|(t, _)| ts - *t <= KEEP_SECS);
    let n = history.len();
    // If the last two points share this value, slide the last one forward
    // instead of adding a new point (the line stays flat either way).
    if n >= 2 && history[n - 1].1 == used && history[n - 2].1 == used {
        history[n - 1].0 = ts;
        return;
    }
    // A reset (drop in usage) or new value: always record.
    history.push((ts, used));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_compress_flat_runs() {
        let mut h = vec![];
        push_snapshot(&mut h, 0, 1.0);
        push_snapshot(&mut h, 60, 1.0);
        push_snapshot(&mut h, 120, 1.0);
        push_snapshot(&mut h, 180, 1.0);
        assert_eq!(h, vec![(0, 1.0), (180, 1.0)]);
        push_snapshot(&mut h, 240, 2.0);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn settings_validation() {
        let mut s = Settings::default();
        assert!(s.validate().is_ok());
        s.work_end = "08:00".into();
        assert!(s.validate().is_err());
        let mut s = Settings { reset_day: 0, ..Settings::default() };
        assert!(s.validate().is_err());
    }

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"resetDay": 15}"#).unwrap();
        assert_eq!(s.reset_day, 15);
        assert_eq!(s.work_start, "09:00");
    }
}
