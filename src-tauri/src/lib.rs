mod api;
mod auth;
mod icon;
mod pace;
mod proxy;
mod proxy_util;
mod store;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, TimeZone, Timelike};
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "copilot-pacer";
const WINDOW: &str = "main";
const NOT_SIGNED_IN: &str = "Not signed in. Sign in with GitHub, or log in with the GitHub CLI (`gh auth login`).";

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct Inner {
    settings: store::Settings,
    usage: Option<api::Usage>,
    error: Option<String>,
    token: Option<(String, auth::Source)>,
    history: Vec<store::Snapshot>,
    last_fetch: Option<Instant>,
    fetching: bool,
    pinned: bool,
    last_hidden: Option<Instant>,
    last_notified_day: Option<i64>,
    device_login_gen: u64,
    /// Cached automatic proxy decision.
    proxy: Option<proxy::Resolved>,
}

pub struct Shared {
    inner: Mutex<Inner>,
    store: store::Store,
    wake: tokio::sync::Notify,
    fetch_lock: tokio::sync::Mutex<()>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

type SharedState<'a> = State<'a, Arc<Shared>>;

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

fn to_local<Tz: TimeZone>(dt: chrono::DateTime<Tz>) -> pace::LocalTime {
    pace::LocalTime {
        day: pace::days_from_civil(dt.year() as i64, dt.month(), dt.day()),
        minute: dt.hour() * 60 + dt.minute(),
    }
}

fn local_now() -> pace::LocalTime {
    to_local(Local::now())
}

fn unix_to_local(ts: i64) -> Option<pace::LocalTime> {
    Local.timestamp_opt(ts, 0).single().map(to_local)
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------------------------------------------------------------------------
// Status payload (what the popup renders)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PaceView {
    expected_pct: f64,
    diff_pct: Option<f64>,
    /// "under" | "near" | "over" | "unlimited" | "unknown"
    status: &'static str,
    cycle_start: String,
    /// Exclusive end = next reset date.
    cycle_end: String,
    total_working_days: f64,
    elapsed_working_days: f64,
    remaining_working_days: f64,
    daily_budget_pct: Option<f64>,
    projected_pct: Option<f64>,
    today_is_working_day: bool,
    calendar_fallback: bool,
    now_x: i64,
    cycle_minutes: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NetworkView {
    mode: String,
    /// Proxy in use (credentials redacted), None = direct.
    proxy: Option<String>,
    /// Where it came from; None until automatic detection has run.
    source: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AuthView {
    source: Option<auth::Source>,
    login: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    usage: Option<api::Usage>,
    pace: PaceView,
    /// (minutes since cycle start, % used)
    history: Vec<(i64, f64)>,
    /// (minutes since cycle start, healthy %)
    curve: Vec<(i64, f64)>,
    auth: AuthView,
    error: Option<String>,
    fetching: bool,
    settings: store::Settings,
    network: NetworkView,
}

fn build_status(shared: &Shared) -> StatusPayload {
    let inner = shared.lock();
    let cfg = inner.settings.pace_config();
    let now = local_now();
    let p = pace::compute(&cfg, now);
    let cycle_minutes = (p.cycle_end - p.cycle_start) * pace::MINUTES_PER_DAY;

    let usage = inner.usage.clone();
    let (status, diff, budget, projected) = match &usage {
        None => ("unknown", None, None, None),
        Some(u) if u.unlimited => ("unlimited", None, None, None),
        Some(u) => {
            let st = pace::classify(u.used_pct, p.expected_pct, inner.settings.over_margin);
            let budget = (p.remaining_working_days > 0.05)
                .then(|| (100.0 - u.used_pct).max(0.0) / p.remaining_working_days);
            let projected = (p.elapsed_working_days >= 0.5 && p.total_working_days > 0.0)
                .then(|| u.used_pct / p.elapsed_working_days * p.total_working_days);
            (st.as_str(), Some(u.used_pct - p.expected_pct), budget, projected)
        }
    };

    let history = inner
        .history
        .iter()
        .filter_map(|(ts, used)| {
            let t = unix_to_local(*ts)?;
            let x = pace::cycle_x(p.cycle_start, t);
            (0..=cycle_minutes).contains(&x).then_some((x, *used))
        })
        .collect();

    StatusPayload {
        pace: PaceView {
            expected_pct: p.expected_pct,
            diff_pct: diff,
            status,
            cycle_start: pace::fmt_ymd(p.cycle_start),
            cycle_end: pace::fmt_ymd(p.cycle_end),
            total_working_days: p.total_working_days,
            elapsed_working_days: p.elapsed_working_days,
            remaining_working_days: p.remaining_working_days,
            daily_budget_pct: budget,
            projected_pct: projected,
            today_is_working_day: p.today_is_working_day,
            calendar_fallback: p.calendar_fallback,
            now_x: pace::cycle_x(p.cycle_start, now),
            cycle_minutes,
        },
        curve: pace::pace_curve(&cfg, p.cycle_start, p.cycle_end),
        history,
        auth: AuthView {
            source: inner.token.as_ref().map(|t| t.1),
            login: usage.as_ref().and_then(|u| u.login.clone()),
        },
        usage,
        error: inner.error.clone(),
        fetching: inner.fetching,
        network: match inner.settings.proxy_mode.as_str() {
            "manual" => NetworkView {
                mode: "manual".into(),
                proxy: Some(proxy_util::redact(&inner.settings.proxy_url)),
                source: Some("manual setting".into()),
            },
            "none" => NetworkView { mode: "none".into(), proxy: None, source: Some("proxy disabled".into()) },
            _ => NetworkView {
                mode: "auto".into(),
                proxy: inner.proxy.as_ref().and_then(|r| r.url.as_deref()).map(proxy_util::redact),
                source: inner.proxy.as_ref().map(|r| r.source.clone()),
            },
        },
        settings: inner.settings.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tray + notifications
// ---------------------------------------------------------------------------

fn status_color(status: &str) -> icon::Rgb {
    match status {
        "under" => icon::GREEN,
        "near" => icon::AMBER,
        "over" => icon::RED,
        _ => icon::GRAY,
    }
}

fn update_tray(app: &AppHandle, st: &StatusPayload) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let used = st.usage.as_ref().filter(|u| !u.unlimited).map(|u| u.used_pct);
    let rgba = icon::render(&icon::Gauge {
        used_pct: used,
        expected_pct: used.map(|_| st.pace.expected_pct),
        color: status_color(st.pace.status),
    });
    let _ = tray.set_icon(Some(Image::new_owned(rgba, icon::SIZE, icon::SIZE)));

    let tooltip = match (&st.usage, used) {
        (Some(u), _) if u.unlimited => "Copilot: unlimited".to_string(),
        (_, Some(u)) => format!(
            "Copilot: {:.0}% used · healthy {:.0}% · {}",
            u,
            st.pace.expected_pct,
            match st.pace.status {
                "under" => "on pace",
                "near" => "slightly ahead",
                _ => "ahead of pace",
            }
        ),
        _ => match &st.error {
            Some(_) => "Copilot Pacer: needs attention".to_string(),
            None => "Copilot Pacer: loading…".to_string(),
        },
    };
    let _ = tray.set_tooltip(Some(tooltip));

    #[cfg(target_os = "macos")]
    {
        let title = if st.settings.show_percent_in_menu_bar {
            Some(match used {
                Some(u) => format!("{u:.0}%"),
                None if st.usage.as_ref().is_some_and(|u| u.unlimited) => "∞".to_string(),
                None => "–".to_string(),
            })
        } else {
            None
        };
        let _ = tray.set_title(title);
    }
}

fn maybe_notify(app: &AppHandle, shared: &Shared, st: &StatusPayload) {
    if !st.settings.notify_over_pace || st.pace.status != "over" {
        return;
    }
    let today = local_now().day;
    {
        let mut inner = shared.lock();
        if inner.last_notified_day == Some(today) {
            return;
        }
        inner.last_notified_day = Some(today);
    }
    let used = st.usage.as_ref().map(|u| u.used_pct).unwrap_or(0.0);
    let mut body = format!(
        "{used:.0}% used; the healthy level right now is {:.0}%.",
        st.pace.expected_pct
    );
    if let Some(b) = st.pace.daily_budget_pct {
        body.push_str(&format!(" To last the cycle, aim for ≤{b:.1}% per working day."));
    }
    let _ = app
        .notification()
        .builder()
        .title("Copilot usage is ahead of pace")
        .body(body)
        .show();
}

fn publish(app: &AppHandle, shared: &Shared) {
    let st = build_status(shared);
    update_tray(app, &st);
    maybe_notify(app, shared, &st);
    let _ = app.emit("status-updated", &st);
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

async fn resolve_token(shared: &Shared, refresh: bool) -> Option<(String, auth::Source)> {
    if !refresh {
        if let Some(t) = shared.lock().token.clone() {
            return Some(t);
        }
    }
    let found = tauri::async_runtime::spawn_blocking(|| {
        auth::keychain_get()
            .map(|t| (t, auth::Source::Device))
            .or_else(|| auth::gh_token().map(|t| (t, auth::Source::GhCli)))
    })
    .await
    .ok()
    .flatten();
    shared.lock().token = found.clone();
    found
}

fn proxy_is_auto(shared: &Shared) -> bool {
    shared.lock().settings.proxy_mode == "auto"
}

/// Build an HTTP client honouring the proxy setting. In automatic mode the
/// detected proxy is cached; `redetect` forces a fresh detection (e.g. after
/// a network error, since VPN/Wi-Fi changes alter the right answer).
async fn http_client(shared: &Shared, redetect: bool) -> Result<reqwest::Client, String> {
    let (mode, manual, cached) = {
        let inner = shared.lock();
        (inner.settings.proxy_mode.clone(), inner.settings.proxy_url.clone(), inner.proxy.clone())
    };
    let choice = match mode.as_str() {
        "none" => api::ProxyChoice::NoProxy,
        "manual" => api::ProxyChoice::Url(manual),
        _ => {
            let resolved = match cached {
                Some(r) if !redetect => r,
                _ => {
                    let r = proxy::detect().await;
                    shared.lock().proxy = Some(r.clone());
                    r
                }
            };
            match resolved.url {
                Some(url) => api::ProxyChoice::Url(url),
                None => api::ProxyChoice::System,
            }
        }
    };
    api::client(&choice)
}

async fn fetch_once(shared: &Shared) -> Result<api::Usage, String> {
    let mut http = http_client(shared, false).await?;
    let token = resolve_token(shared, false).await.ok_or(NOT_SIGNED_IN)?;

    let mut result = api::fetch_usage(&http, &token.0, now_unix()).await;
    if matches!(result, Err(api::FetchError::Network(_))) && proxy_is_auto(shared) {
        // The network may have changed: detect the proxy again and retry once.
        http = http_client(shared, true).await?;
        result = api::fetch_usage(&http, &token.0, now_unix()).await;
    }

    match result {
        Ok(u) => Ok(u),
        Err(api::FetchError::Unauthorized(code, msg)) => {
            // The token may have been rotated (e.g. `gh auth refresh`): try once more.
            if let Some(fresh) = resolve_token(shared, true).await {
                if fresh.0 != token.0 {
                    if let Ok(u) = api::fetch_usage(&http, &fresh.0, now_unix()).await {
                        return Ok(u);
                    }
                }
            }
            Err(match token.1 {
                auth::Source::GhCli => format!(
                    "Copilot's usage endpoint didn't accept your GitHub CLI token (HTTP {code}: {msg}). Use “Sign in with GitHub” instead."
                ),
                auth::Source::Device => format!(
                    "GitHub rejected the saved sign-in (HTTP {code}: {msg}). Please sign in again."
                ),
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn do_fetch(app: &AppHandle, shared: &Shared) {
    let _guard = shared.fetch_lock.lock().await;
    shared.lock().fetching = true;
    let _ = app.emit("fetching", true);

    let result = fetch_once(shared).await;

    let history_to_save = {
        let mut inner = shared.lock();
        inner.fetching = false;
        inner.last_fetch = Some(Instant::now());
        match result {
            Ok(u) => {
                store::push_snapshot(&mut inner.history, u.fetched_at, u.used_pct);
                inner.usage = Some(u);
                inner.error = None;
                Some(inner.history.clone())
            }
            Err(e) => {
                inner.error = Some(e);
                None
            }
        }
    };
    if let Some(h) = history_to_save {
        let _ = shared.store.save_history(&h);
    }
    publish(app, shared);
}

async fn poll_loop(app: AppHandle, shared: Arc<Shared>) {
    loop {
        let due = {
            let inner = shared.lock();
            match inner.last_fetch {
                None => true,
                Some(t) => t.elapsed() >= Duration::from_secs(inner.settings.refresh_minutes as u64 * 60),
            }
        };
        if due {
            do_fetch(&app, &shared).await;
        } else {
            // The healthy line moves with time even when usage doesn't.
            publish(&app, &shared);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            _ = shared.wake.notified() => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Popup window
// ---------------------------------------------------------------------------

fn position_near(w: &WebviewWindow, rect: tauri::Rect) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let pos: PhysicalPosition<f64> = rect.position.to_physical(scale);
    let size: tauri::PhysicalSize<f64> = rect.size.to_physical(scale);
    let Ok(win) = w.outer_size() else { return };
    let (ww, wh) = (win.width as f64, win.height as f64);
    let (cx, cy) = (pos.x + size.width / 2.0, pos.y + size.height / 2.0);

    let monitors = w.available_monitors().unwrap_or_default();
    let mon = monitors.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        cx >= p.x as f64
            && cx < p.x as f64 + s.width as f64
            && cy >= p.y as f64
            && cy < p.y as f64 + s.height as f64
    });
    let (mx, my, mw, mh) = match mon {
        Some(m) => (
            m.position().x as f64,
            m.position().y as f64,
            m.size().width as f64,
            m.size().height as f64,
        ),
        None => (0.0, 0.0, 100_000.0, 100_000.0),
    };

    let gap = 6.0 * scale;
    let x = (cx - ww / 2.0).clamp(mx + gap, (mx + mw - ww - gap).max(mx + gap));
    // Menu bar at the top (macOS) → open below; taskbar at the bottom → above.
    let y = if cy < my + mh / 2.0 {
        pos.y + size.height + gap
    } else {
        pos.y - wh - gap
    };
    let y = y.clamp(my, (my + mh - wh).max(my));
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn show_window(app: &AppHandle, anchor: Option<tauri::Rect>) {
    let Some(w) = app.get_webview_window(WINDOW) else { return };
    match anchor {
        Some(rect) => position_near(&w, rect),
        None => {
            if !w.is_visible().unwrap_or(false) {
                let _ = w.center();
            }
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit("popup-shown", ());
}

fn toggle_window(app: &AppHandle, anchor: tauri::Rect) {
    let Some(w) = app.get_webview_window(WINDOW) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    // Clicking the tray icon first blurs (and hides) the popup; don't
    // immediately re-open it.
    let shared = app.state::<Arc<Shared>>();
    let recently_hidden = shared
        .lock()
        .last_hidden
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !recently_hidden {
        show_window(app, Some(anchor));
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DeviceLoginResult {
    ok: bool,
    error: Option<String>,
}

#[tauri::command]
fn get_status(shared: SharedState<'_>) -> StatusPayload {
    build_status(&shared)
}

#[tauri::command]
async fn refresh(app: AppHandle, shared: SharedState<'_>) -> Result<StatusPayload, String> {
    let shared = shared.inner().clone();
    do_fetch(&app, &shared).await;
    Ok(build_status(&shared))
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    shared: SharedState<'_>,
    settings: store::Settings,
) -> Result<StatusPayload, String> {
    let mut s = settings;
    s.validate()?;
    s.initialized = true;
    shared.store.save_settings(&s)?;
    {
        let mut inner = shared.lock();
        // A different proxy should take effect right away.
        if inner.settings.proxy_url != s.proxy_url || inner.settings.proxy_mode != s.proxy_mode {
            inner.last_fetch = None;
        }
        inner.last_notified_day = None;
        inner.settings = s;
    }
    shared.wake.notify_one();
    publish(&app, &shared);
    Ok(build_status(&shared))
}

/// Called by the UI at startup with freshly computed holiday dates.
#[tauri::command]
fn set_excluded_days(app: AppHandle, shared: SharedState<'_>, days: Vec<String>) -> Result<(), String> {
    let snapshot = {
        let mut inner = shared.lock();
        if inner.settings.excluded_days == days {
            return Ok(());
        }
        inner.settings.excluded_days = days;
        inner.settings.clone()
    };
    shared.store.save_settings(&snapshot)?;
    publish(&app, &shared);
    Ok(())
}

#[tauri::command]
async fn start_device_login(app: AppHandle, shared: SharedState<'_>) -> Result<auth::DeviceCode, String> {
    let shared = shared.inner().clone();
    let mut http = http_client(&shared, false).await?;
    let code = match auth::request_device_code(&http).await {
        Ok(c) => c,
        Err(_) if proxy_is_auto(&shared) => {
            http = http_client(&shared, true).await?;
            auth::request_device_code(&http).await?
        }
        Err(e) => return Err(e),
    };
    let gen = {
        let mut inner = shared.lock();
        inner.device_login_gen += 1;
        inner.device_login_gen
    };
    let _ = app.opener().open_url(code.verification_uri.clone(), None::<&str>);

    let dc = code.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = dc.interval.max(1);
        let deadline = Instant::now() + Duration::from_secs(dc.expires_in.max(60));
        let finish = |ok: bool, error: Option<String>| {
            let _ = app.emit("device-login", DeviceLoginResult { ok, error });
        };
        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            if shared.lock().device_login_gen != gen {
                return; // cancelled or superseded
            }
            if Instant::now() > deadline {
                finish(false, Some("The code expired. Please try again.".into()));
                return;
            }
            match auth::poll_device_token(&http, &dc.device_code).await {
                auth::PollResult::Pending => {}
                auth::PollResult::SlowDown => interval += 5,
                auth::PollResult::Failed(msg) => {
                    finish(false, Some(msg));
                    return;
                }
                auth::PollResult::Token(token) => {
                    let t2 = token.clone();
                    let saved = tauri::async_runtime::spawn_blocking(move || auth::keychain_set(&t2))
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()));
                    shared.lock().token = Some((token, auth::Source::Device));
                    finish(
                        true,
                        saved.err().map(|e| format!("Signed in, but couldn't save to the keychain: {e}")),
                    );
                    do_fetch(&app, &shared).await;
                    return;
                }
            }
        }
    });
    Ok(code)
}

/// Re-run automatic proxy detection, then refresh usage.
#[tauri::command]
async fn redetect_proxy(app: AppHandle, shared: SharedState<'_>) -> Result<StatusPayload, String> {
    let shared = shared.inner().clone();
    shared.lock().proxy = None;
    if proxy_is_auto(&shared) {
        http_client(&shared, true).await?;
    }
    do_fetch(&app, &shared).await;
    Ok(build_status(&shared))
}

#[tauri::command]
fn cancel_device_login(shared: SharedState<'_>) {
    shared.lock().device_login_gen += 1;
}

#[tauri::command]
async fn sign_out(app: AppHandle, shared: SharedState<'_>) -> Result<StatusPayload, String> {
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(auth::keychain_delete)
        .await
        .map_err(|e| e.to_string())??;
    {
        let mut inner = shared.lock();
        inner.token = None;
        inner.usage = None;
        inner.error = None;
    }
    // Falls back to the GitHub CLI token if there is one.
    do_fetch(&app, &shared).await;
    Ok(build_status(&shared))
}

#[tauri::command]
fn set_pinned(shared: SharedState<'_>, pinned: bool) {
    shared.lock().pinned = pinned;
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.hide();
    }
}

/// Only GitHub URLs (used for the device sign-in page).
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/") {
        return Err("Refusing to open a non-GitHub URL".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app, None);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Menu-bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let store = store::Store::new(app.path().app_config_dir()?);
            let shared = Arc::new(Shared {
                inner: Mutex::new(Inner {
                    settings: store.load_settings(),
                    usage: None,
                    error: None,
                    token: None,
                    history: store.load_history(),
                    last_fetch: None,
                    fetching: false,
                    pinned: false,
                    last_hidden: None,
                    last_notified_day: None,
                    device_login_gen: 0,
                    proxy: None,
                }),
                store,
                wake: tokio::sync::Notify::new(),
                fetch_lock: tokio::sync::Mutex::new(()),
            });
            app.manage(shared.clone());

            // --- Tray -------------------------------------------------------
            let open_i = MenuItem::with_id(app, "open", "Show usage", true, None::<&str>)?;
            let refresh_i = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Copilot Pacer", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &refresh_i, &settings_i, &sep, &quit_i])?;

            let initial = icon::render(&icon::Gauge {
                used_pct: None,
                expected_pct: None,
                color: icon::GRAY,
            });
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(Image::new_owned(initial, icon::SIZE, icon::SIZE))
                .tooltip("Copilot Pacer")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_window(app, None),
                    "settings" => {
                        show_window(app, None);
                        let _ = app.emit("show-settings", ());
                    }
                    "refresh" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let shared = app.state::<Arc<Shared>>().inner().clone();
                            do_fetch(&app, &shared).await;
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle(), rect);
                    }
                })
                .build(app)?;

            // --- Popup behaviour -------------------------------------------
            if let Some(w) = app.get_webview_window(WINDOW) {
                let handle = app.handle().clone();
                w.on_window_event(move |event| match event {
                    WindowEvent::Focused(false) => {
                        let shared = handle.state::<Arc<Shared>>();
                        let mut inner = shared.lock();
                        if !inner.pinned {
                            inner.last_hidden = Some(Instant::now());
                            drop(inner);
                            if let Some(w) = handle.get_webview_window(WINDOW) {
                                let _ = w.hide();
                            }
                        }
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        if let Some(w) = handle.get_webview_window(WINDOW) {
                            let _ = w.hide();
                        }
                    }
                    _ => {}
                });
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(poll_loop(handle, shared));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            refresh,
            save_settings,
            set_excluded_days,
            start_device_login,
            cancel_device_login,
            sign_out,
            set_pinned,
            redetect_proxy,
            hide_window,
            open_url,
            quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Copilot Pacer")
        .run(|_app, event| {
            // Keep running in the tray when the window is closed.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
