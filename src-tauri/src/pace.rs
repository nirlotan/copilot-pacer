//! Pacing math. Pure std, no I/O, no time zones: callers pass the local
//! calendar day and minute-of-day, so everything here is deterministic and
//! unit-testable.
//!
//! Days are represented as "days since 1970-01-01" in the *local* calendar.

use std::collections::HashSet;

pub const MINUTES_PER_DAY: i64 = 1440;

// ---------------------------------------------------------------------------
// Calendar helpers (Howard Hinnant's civil-date algorithms)
// ---------------------------------------------------------------------------

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Sunday … 6 = Saturday (same convention as JavaScript's `Date#getDay`).
pub fn weekday(day: i64) -> usize {
    // 1970-01-01 was a Thursday (4).
    (day + 4).rem_euclid(7) as usize
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        _ => 28,
    }
}

pub fn parse_ymd(s: &str) -> Option<i64> {
    let s = s.get(..10)?;
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

pub fn fmt_ymd(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse "HH:MM" into minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h > 24 || m > 59 || (h == 24 && m != 0) {
        return None;
    }
    Some(h * 60 + m)
}

// ---------------------------------------------------------------------------
// Pacing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// Local calendar day (days since 1970-01-01).
    pub day: i64,
    /// Minutes since local midnight, 0..1440.
    pub minute: u32,
}

#[derive(Debug, Clone)]
pub struct PaceConfig {
    /// Day of month the quota resets (1–31; clamped to the month's length).
    pub reset_day: u32,
    /// Indexed 0 = Sunday … 6 = Saturday.
    pub work_days: [bool; 7],
    pub work_start_min: u32,
    pub work_end_min: u32,
    /// Holidays and personal days off.
    pub excluded: HashSet<i64>,
}

impl Default for PaceConfig {
    fn default() -> Self {
        PaceConfig {
            reset_day: 1,
            work_days: [false, true, true, true, true, true, false],
            work_start_min: 9 * 60,
            work_end_min: 18 * 60,
            excluded: HashSet::new(),
        }
    }
}

impl PaceConfig {
    pub fn is_working_day(&self, day: i64) -> bool {
        self.work_days[weekday(day)] && !self.excluded.contains(&day)
    }

    /// Share of a working day that has elapsed at `minute` (0.0–1.0).
    pub fn day_fraction(&self, minute: u32) -> f64 {
        let (s, e) = (self.work_start_min as f64, self.work_end_min as f64);
        let m = minute as f64;
        if e <= s {
            // Bad/empty hours: treat the whole calendar day as working time.
            return (m / MINUTES_PER_DAY as f64).clamp(0.0, 1.0);
        }
        ((m - s) / (e - s)).clamp(0.0, 1.0)
    }
}

/// Start (inclusive) and end (exclusive) of the billing cycle containing `today`.
pub fn cycle_bounds(today: i64, reset_day: u32) -> (i64, i64) {
    let reset_day = reset_day.clamp(1, 31);
    let anchor = |y: i64, m: u32| days_from_civil(y, m, reset_day.min(days_in_month(y, m)));
    let (y, m, _) = civil_from_days(today);
    let this_month = anchor(y, m);
    let (sy, sm) = if today >= this_month {
        (y, m)
    } else if m == 1 {
        (y - 1, 12)
    } else {
        (y, m - 1)
    };
    let start = anchor(sy, sm);
    let (ny, nm) = if sm == 12 { (sy + 1, 1) } else { (sy, sm + 1) };
    (start, anchor(ny, nm))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// At or below the healthy line.
    Under,
    /// Above the line but within the tolerance margin.
    Near,
    /// Above the line by more than the margin.
    Over,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Under => "under",
            Status::Near => "near",
            Status::Over => "over",
        }
    }
}

pub fn classify(used_pct: f64, expected_pct: f64, margin: f64) -> Status {
    let diff = used_pct - expected_pct;
    if diff <= 0.0 {
        Status::Under
    } else if diff <= margin.max(0.0) {
        Status::Near
    } else {
        Status::Over
    }
}

#[derive(Debug, Clone)]
pub struct Pace {
    pub cycle_start: i64,
    pub cycle_end: i64,
    pub total_working_days: f64,
    pub elapsed_working_days: f64,
    pub remaining_working_days: f64,
    /// Healthy usage right now, 0–100.
    pub expected_pct: f64,
    pub today_is_working_day: bool,
    /// True when the cycle has no working days at all and we fell back to
    /// plain calendar time.
    pub calendar_fallback: bool,
}

pub fn working_days_between(cfg: &PaceConfig, from: i64, to_exclusive: i64) -> f64 {
    (from..to_exclusive).filter(|d| cfg.is_working_day(*d)).count() as f64
}

/// Elapsed working time (in working days) from `start` to `t`.
fn elapsed_at(cfg: &PaceConfig, start: i64, t: LocalTime) -> f64 {
    if t.day < start {
        return 0.0;
    }
    let mut e = working_days_between(cfg, start, t.day);
    if cfg.is_working_day(t.day) {
        e += cfg.day_fraction(t.minute);
    }
    e
}

/// Healthy usage percentage at time `t`, for the cycle [start, end).
pub fn expected_pct_at(cfg: &PaceConfig, start: i64, end: i64, t: LocalTime) -> f64 {
    let total = working_days_between(cfg, start, end);
    if total <= 0.0 {
        let span = ((end - start) * MINUTES_PER_DAY) as f64;
        let pos = ((t.day - start) * MINUTES_PER_DAY + t.minute as i64) as f64;
        return (pos / span * 100.0).clamp(0.0, 100.0);
    }
    (elapsed_at(cfg, start, t) / total * 100.0).clamp(0.0, 100.0)
}

pub fn compute(cfg: &PaceConfig, now: LocalTime) -> Pace {
    let (start, end) = cycle_bounds(now.day, cfg.reset_day);
    let total = working_days_between(cfg, start, end);
    let elapsed = if total > 0.0 { elapsed_at(cfg, start, now) } else { 0.0 };
    Pace {
        cycle_start: start,
        cycle_end: end,
        total_working_days: total,
        elapsed_working_days: elapsed,
        remaining_working_days: (total - elapsed).max(0.0),
        expected_pct: expected_pct_at(cfg, start, end, now),
        today_is_working_day: cfg.is_working_day(now.day),
        calendar_fallback: total <= 0.0,
    }
}

/// Minutes since cycle start ("x" coordinate for charts).
pub fn cycle_x(start: i64, t: LocalTime) -> i64 {
    (t.day - start) * MINUTES_PER_DAY + t.minute as i64
}

/// The healthy-usage line for a whole cycle as a piecewise-linear list of
/// (minutes since cycle start, expected %) points. Exact: the line only bends
/// at midnight, work start and work end.
pub fn pace_curve(cfg: &PaceConfig, start: i64, end: i64) -> Vec<(i64, f64)> {
    let mut pts: Vec<(i64, f64)> = Vec::new();
    let mut push = |x: i64, y: f64| {
        if let Some(last) = pts.last() {
            if last.0 == x {
                return;
            }
        }
        pts.push((x, (y * 100.0).round() / 100.0));
    };
    for day in start..end {
        let mut minutes = vec![0u32];
        if cfg.work_end_min > cfg.work_start_min {
            minutes.push(cfg.work_start_min);
            minutes.push(cfg.work_end_min);
        }
        for m in minutes {
            let t = LocalTime { day, minute: m.min(1439) };
            push(cycle_x(start, t), expected_pct_at(cfg, start, end, t));
        }
    }
    push((end - start) * MINUTES_PER_DAY, 100.0);
    // Drop collinear middle points to keep the payload small.
    let mut out: Vec<(i64, f64)> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let cross = (b.0 - a.0) as f64 * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) as f64;
            if cross.abs() < 1e-6 {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i64, m: u32, dd: u32) -> i64 {
        days_from_civil(y, m, dd)
    }

    #[test]
    fn civil_roundtrip() {
        for z in -800_000..800_000i64 {
            if z % 997 != 0 {
                continue;
            }
            let (y, m, dd) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, dd), z);
        }
        assert_eq!(d(1970, 1, 1), 0);
        assert_eq!(fmt_ymd(d(2026, 11, 9)), "2026-11-09");
        assert_eq!(parse_ymd("2024-02-29"), Some(d(2024, 2, 29)));
        assert_eq!(parse_ymd("2025-02-29"), None);
        assert_eq!(parse_ymd("2026-12-01T00:00:00Z"), Some(d(2026, 12, 1)));
    }

    #[test]
    fn weekdays() {
        assert_eq!(weekday(d(1970, 1, 1)), 4); // Thursday
        assert_eq!(weekday(d(2026, 11, 9)), 1); // Monday
        assert_eq!(weekday(d(2026, 11, 8)), 0); // Sunday
    }

    #[test]
    fn hhmm() {
        assert_eq!(parse_hhmm("09:30"), Some(570));
        assert_eq!(parse_hhmm("24:00"), Some(1440));
        assert_eq!(parse_hhmm("24:01"), None);
        assert_eq!(parse_hhmm("x"), None);
    }

    #[test]
    fn cycle_basic_first_of_month() {
        assert_eq!(cycle_bounds(d(2026, 11, 9), 1), (d(2026, 11, 1), d(2026, 12, 1)));
        assert_eq!(cycle_bounds(d(2026, 11, 1), 1), (d(2026, 11, 1), d(2026, 12, 1)));
        assert_eq!(cycle_bounds(d(2026, 12, 31), 1), (d(2026, 12, 1), d(2027, 1, 1)));
    }

    #[test]
    fn cycle_custom_day_and_year_wrap() {
        assert_eq!(cycle_bounds(d(2026, 11, 9), 15), (d(2026, 10, 15), d(2026, 11, 15)));
        assert_eq!(cycle_bounds(d(2026, 11, 15), 15), (d(2026, 11, 15), d(2026, 12, 15)));
        assert_eq!(cycle_bounds(d(2027, 1, 3), 15), (d(2026, 12, 15), d(2027, 1, 15)));
    }

    #[test]
    fn cycle_short_months_clamp() {
        // Reset on the 31st: Feb resets on its last day.
        assert_eq!(cycle_bounds(d(2026, 2, 27), 31), (d(2026, 1, 31), d(2026, 2, 28)));
        assert_eq!(cycle_bounds(d(2026, 2, 28), 31), (d(2026, 2, 28), d(2026, 3, 31)));
        assert_eq!(cycle_bounds(d(2024, 2, 29), 30), (d(2024, 2, 29), d(2024, 3, 30)));
    }

    #[test]
    fn working_days_in_november_2026() {
        let cfg = PaceConfig::default(); // Mon–Fri
        let (s, e) = cycle_bounds(d(2026, 11, 9), 1);
        assert_eq!(working_days_between(&cfg, s, e), 21.0);
    }

    #[test]
    fn expected_pace_mid_day() {
        let cfg = PaceConfig::default();
        // Mon 9 Nov 2026, 13:30 — 5 full working days done (2–6 Nov), today 50%.
        let p = compute(&cfg, LocalTime { day: d(2026, 11, 9), minute: 13 * 60 + 30 });
        assert!((p.elapsed_working_days - 5.5).abs() < 1e-9);
        assert!((p.expected_pct - 5.5 / 21.0 * 100.0).abs() < 1e-9);
        assert!((p.remaining_working_days - 15.5).abs() < 1e-9);
        assert!(p.today_is_working_day);
    }

    #[test]
    fn before_and_after_hours() {
        let cfg = PaceConfig::default();
        let early = compute(&cfg, LocalTime { day: d(2026, 11, 9), minute: 7 * 60 });
        let late = compute(&cfg, LocalTime { day: d(2026, 11, 9), minute: 22 * 60 });
        assert!((early.elapsed_working_days - 5.0).abs() < 1e-9);
        assert!((late.elapsed_working_days - 6.0).abs() < 1e-9);
    }

    #[test]
    fn weekend_is_flat() {
        let cfg = PaceConfig::default();
        let fri = compute(&cfg, LocalTime { day: d(2026, 11, 6), minute: 23 * 60 });
        let sun = compute(&cfg, LocalTime { day: d(2026, 11, 8), minute: 12 * 60 });
        assert!((fri.expected_pct - sun.expected_pct).abs() < 1e-9);
        assert!(!sun.today_is_working_day);
    }

    #[test]
    fn holidays_reduce_total_and_freeze_line() {
        let mut cfg = PaceConfig::default();
        cfg.excluded.insert(d(2026, 11, 9));
        let p = compute(&cfg, LocalTime { day: d(2026, 11, 9), minute: 15 * 60 });
        assert_eq!(p.total_working_days, 20.0);
        assert!((p.elapsed_working_days - 5.0).abs() < 1e-9);
        assert!(!p.today_is_working_day);
    }

    #[test]
    fn israeli_week() {
        let mut cfg = PaceConfig::default();
        cfg.work_days = [true, true, true, true, true, false, false]; // Sun–Thu
        let (s, e) = cycle_bounds(d(2026, 11, 9), 1);
        // Nov 2026 starts on a Sunday: 5 Sun + 5 Mon + 4 Tue + 4 Wed + 4 Thu.
        assert_eq!(working_days_between(&cfg, s, e), 22.0);
        // Sun 1 Nov … Thu 5 Nov = 5 days, Sun 8 Nov = 1 → 6 before Mon 9th.
        let p = compute(&cfg, LocalTime { day: d(2026, 11, 9), minute: 0 });
        assert!((p.elapsed_working_days - 6.0).abs() < 1e-9);
    }

    #[test]
    fn no_working_days_falls_back_to_calendar() {
        let mut cfg = PaceConfig::default();
        cfg.work_days = [false; 7];
        let p = compute(&cfg, LocalTime { day: d(2026, 11, 16), minute: 0 });
        assert!(p.calendar_fallback);
        assert!((p.expected_pct - 50.0).abs() < 1e-9); // 15 of 30 days
    }

    #[test]
    fn curve_is_monotonic_and_ends_at_100() {
        let mut cfg = PaceConfig::default();
        cfg.excluded.insert(d(2026, 11, 11));
        let (s, e) = cycle_bounds(d(2026, 11, 9), 1);
        let c = pace_curve(&cfg, s, e);
        assert_eq!(c.first().unwrap(), &(0, 0.0));
        assert_eq!(c.last().unwrap(), &((e - s) * MINUTES_PER_DAY, 100.0));
        for w in c.windows(2) {
            assert!(w[1].0 > w[0].0);
            assert!(w[1].1 >= w[0].1);
        }
        // Spot check against the direct computation.
        let t = LocalTime { day: d(2026, 11, 9), minute: 13 * 60 + 30 };
        let x = cycle_x(s, t);
        let i = c.iter().position(|p| p.0 > x).unwrap();
        let (a, b) = (c[i - 1], c[i]);
        let interp = a.1 + (b.1 - a.1) * (x - a.0) as f64 / (b.0 - a.0) as f64;
        assert!((interp - expected_pct_at(&cfg, s, e, t)).abs() < 0.02);
    }

    #[test]
    fn classification() {
        assert_eq!(classify(10.0, 26.0, 5.0), Status::Under);
        assert_eq!(classify(28.0, 26.0, 5.0), Status::Near);
        assert_eq!(classify(32.0, 26.0, 5.0), Status::Over);
    }
}
