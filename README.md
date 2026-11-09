# Copilot Pacer

A small macOS menu-bar / Windows system-tray app that shows how fast you're
using your **GitHub Copilot credits** (premium requests) compared with a
*healthy* pace for the billing cycle. The healthy pace is spread over **your
working days only**: weekends, public holidays and personal days off don't count.

- **Tray icon**: a ring filled to your usage and coloured by pace (green =
  at or under pace, amber = slightly over, red = over by more than your
  tolerance). A tick marks where usage *should* be now. On macOS the
  percentage is shown next to the icon.
- **Popup** (click the icon): % used vs. healthy %, a chart of usage against
  the pace line for the cycle, your daily budget for the rest of the cycle,
  a projection to the reset date, and upcoming days off.
- **Settings**: reset day of month (default the 1st), working weekdays,
  working hours, holiday calendars for any country/region (bundled, works
  offline), personal days off, refresh interval, over-pace notifications,
  launch at login, and proxy settings (automatic detection, including PAC
  files, or manual).

## How "healthy" is calculated

Each working day in the cycle gets an equal share of 100%. Days before
today count fully; today counts in proportion to how much of your working
hours (default 09:00–18:00) has passed. Non-working days add nothing, so the
line stays flat over weekends and holidays.

```
healthy % = (working days elapsed + today's fraction) / working days in cycle × 100
daily budget = (100 − used %) / working days left
```

The cycle runs from the reset day to the day before the next reset. If you
pick the 31st, shorter months reset on their last day. Holidays that run
sunset to sunset (e.g. Jewish holidays) count only for the actual day.

## Where the number comes from

The app calls `GET https://api.github.com/copilot_internal/user`, the same
endpoint the Copilot editor extensions use for their usage meter, and reads
the `premium_interactions` quota (shown as "Credits" in VS Code).
**This endpoint is undocumented and may change.** Parsing is defensive, and
if it breaks you'll see the error in the popup.

Sign-in, in order of preference:

1. **Sign in with GitHub** (device flow). Uses the public OAuth client ID of
   GitHub's Copilot editor plugins (`Iv1.b507a08c87ecfe98`), scope `read:user`.
   The token is stored in the macOS Keychain / Windows Credential Manager.
2. **GitHub CLI**: if you're logged in with `gh`, its token is used
   automatically (`gh auth token`). Some `gh` tokens aren't accepted by the
   Copilot endpoint. In that case the popup asks you to use option 1.

### Proxies

Settings → Network → **Proxy** has three modes:

- **Automatic (default)** checks, in order: proxy environment variables; on
  macOS the system proxy (`scutil --proxy`), including **PAC / WPAD** files,
  which are evaluated with the built-in JavaScript engine (`osascript`); on
  Windows the system resolver (`WebRequest.GetSystemWebProxy`, which handles
  PAC/WPAD); and finally the `HTTPS_PROXY` / `ALL_PROXY` your login shell sets.
  The result is cached and detected again after a network error (e.g. VPN
  on/off), or when you click **Detect again**.
- **Manual** uses an `http://`, `https://` or `socks5://` proxy URL you enter.
- **No proxy** always connects directly.

Proxies that require NTLM/Kerberos authentication aren't supported.

If your organisation enforces SAML SSO, you may need to authorise the token
for the org on github.com when prompted.

Nothing else leaves your machine. Settings and a small usage history live in
the app config directory (`~/Library/Application Support/com.copilot-pacer.tray`
on macOS, `%APPDATA%\com.copilot-pacer.tray` on Windows).

## Install

Grab the `.dmg` (macOS, universal) or `.exe` / `.msi` (Windows) from the
GitHub Actions run artifacts, or from a draft release when you push a `v*` tag.

The builds are **unsigned**:

- **macOS**: after copying to Applications, run
  `xattr -cr "/Applications/Copilot Pacer.app"` once, or right-click → Open.
- **Windows**: SmartScreen may warn; choose *More info → Run anyway*.

## Develop

Prerequisites: Node 20+, Rust (stable), and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
npm install
npm run tauri dev        # run with hot reload
npm test                 # TypeScript unit tests
npm run test:rust        # Rust unit tests (pacing math, icon, parsing)
npm run tauri build      # build installers for the current OS
```

Project layout:

| Path | What |
| --- | --- |
| `src-tauri/src/pace.rs` | Cycle bounds, working-day pacing, pace curve (pure, tested) |
| `src-tauri/src/icon.rs` | Tray gauge renderer (pure RGBA, tested) |
| `src-tauri/src/api.rs` | Copilot usage client + defensive parsing |
| `src-tauri/src/auth.rs` | Keychain, `gh` token lookup, GitHub device flow |
| `src-tauri/src/proxy.rs`, `proxy_util.rs`, `pac_eval.js` | Automatic proxy detection (env, macOS PAC via JXA, Windows resolver, login shell) |
| `src-tauri/src/store.rs` | Settings + usage history persistence |
| `src-tauri/src/lib.rs` | Tray, popup window, polling loop, commands, notifications |
| `src/` | Popup UI (vanilla TypeScript + Vite), holiday calendars via `date-holidays` |

Regenerate the app icons with `node scripts/make-icons.cjs` (needs `sharp`)
or `npx tauri icon path/to/1024.png`.

## Notes

- Polling defaults to every 10 minutes. The healthy line and tray colour are
  recalculated every minute in between.
- Over-pace notifications fire at most once per day.
- If your org's reset date differs from the reset day you set, the popup
  shows GitHub's date next to the cycle.
