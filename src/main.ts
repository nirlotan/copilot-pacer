import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { chartSvg } from './chart';
import { days, escapeHtml as h, lastDayOf, paceSentence, pct, relativeTime, shortDate } from './format';
import { firstRunDefaults, parseRegion, ymd } from './holidays-core';
import {
  computeExcludedDays,
  daysOffInRange,
  HOLIDAY_TYPES,
  listCountries,
  listStates,
  regionLabel,
  type DayOff,
} from './holidays';
import type { DeviceCode, Settings, Status } from './types';

const root = document.getElementById('app')!;

let status: Status | null = null;
let view: 'overview' | 'settings' = 'overview';
let draft: Settings | null = null;
let device: { code: DeviceCode; error?: string } | null = null;
let launchDraft = false;
let saveMessage = '';

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const IS_MAC = navigator.userAgent.includes('Mac');

const ICON = {
  refresh: '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  gear: '<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>',
  back: '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  close: '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 4 8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>',
};

// ---------------------------------------------------------------------------
// Holidays (memoised; date-holidays is not free to evaluate)
// ---------------------------------------------------------------------------

let daysOffCache: { key: string; value: DayOff[] } | null = null;
function cycleDaysOff(s: Settings, start: string, end: string): DayOff[] {
  const key = JSON.stringify([s.holidayRegions, s.holidayTypes, s.customDaysOff, start, end]);
  if (daysOffCache?.key !== key) {
    daysOffCache = { key, value: daysOffInRange(s.holidayRegions, s.holidayTypes, s.customDaysOff, start, end) };
  }
  return daysOffCache.value;
}

let countries: { code: string; name: string }[] | null = null;
const getCountries = () => (countries ??= listCountries());

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function render() {
  if (view === 'settings' && draft) renderSettings();
  else renderOverview();
}

function header(left: string, right: string) {
  return `<header class="top">${left}<div class="actions">${right}</div></header>`;
}

function renderOverview() {
  const s = status;
  if (!s) {
    root.innerHTML = `<div class="empty">Loading…</div>`;
    return;
  }
  const u = s.usage;
  const p = s.pace;
  const plan = u?.plan ? `<span class="tag">${h(u.plan)}</span>` : '';
  const top = header(
    `<div class="brand"><span class="dot ${p.status}"></span>Copilot pacing ${plan}</div>`,
    `<button class="icon-btn ${s.fetching ? 'spin' : ''}" data-act="refresh" title="Refresh now">${ICON.refresh}</button>
     <button class="icon-btn" data-act="settings" title="Settings">${ICON.gear}</button>`,
  );

  if (device) {
    root.innerHTML = top + deviceCard(device);
    return;
  }
  if (!u) {
    root.innerHTML = top + (s.fetching && !s.error ? `<div class="empty">Checking Copilot usage…</div>` : signInCard(s));
    return;
  }

  const last = lastDayOf(p.cycleEnd);
  const used = u.usedPct;
  const exp = p.expectedPct;
  const offs = cycleDaysOff(s.settings, p.cycleStart, p.cycleEnd);
  const todayStr = ymd(new Date());
  const upcoming = offs.filter((d) => d.date >= todayStr).slice(0, 3);

  const hero = u.unlimited
    ? `<section class="hero"><div class="figure"><div class="num">∞</div><div class="cap">Unlimited credits on this plan</div></div></section>`
    : `<section class="hero">
        <div class="figure"><div class="num">${used.toFixed(0)}<small>%</small></div><div class="cap">credits used</div></div>
        <div class="figure right"><div class="num muted">${exp.toFixed(0)}<small>%</small></div><div class="cap">healthy by now</div></div>
      </section>
      <div class="pill ${p.status}">${h(paceSentence(p.diffPct, p.status))}</div>
      <div class="meter" title="Used ${pct(used, 1)} · healthy ${pct(exp, 1)}">
        <div class="meter-fill ${p.status}" style="width:${Math.min(used, 100)}%"></div>
        <div class="meter-mark" style="left:${Math.min(exp, 100)}%"></div>
      </div>`;

  const chart = `<div class="chart">${chartSvg({
    curve: s.curve,
    history: s.history,
    nowX: p.nowX,
    cycleMinutes: p.cycleMinutes,
    current: u.unlimited ? null : used,
  })}</div>
    <div class="chart-legend"><span>${shortDate(p.cycleStart)}</span>
      <span class="keys"><i class="k-usage ${p.status}"></i>used <i class="k-pace"></i>healthy</span>
      <span>${shortDate(last)}</span></div>`;

  const stats = u.unlimited
    ? ''
    : `<dl class="stats">
        <div><dt>Daily budget</dt><dd>${p.dailyBudgetPct != null ? `${pct(p.dailyBudgetPct, 1)}<small> / work day</small>` : '—'}</dd></div>
        <div><dt>At this rate</dt><dd>${p.projectedPct != null ? `${pct(p.projectedPct)}<small> by reset</small>` : '—'}</dd></div>
        <div><dt>Work days elapsed</dt><dd>${(Math.round(p.elapsedWorkingDays * 10) / 10).toString()}<small> of ${p.totalWorkingDays}</small></dd></div>
        <div><dt>Work days left</dt><dd>${days(p.remainingWorkingDays)}</dd></div>
      </dl>`;

  const ghReset = u.githubResetDate?.slice(0, 10);
  const resetNote = ghReset && ghReset !== p.cycleEnd ? ` <span class="warn" title="Your reset day differs from GitHub's">· GitHub resets ${h(shortDate(ghReset))}</span>` : '';
  const cycle = `<p class="meta">Cycle ${h(shortDate(p.cycleStart))} – ${h(shortDate(last))}${resetNote}${
    p.calendarFallback ? ' · no working days set, using calendar time' : ''
  }${!p.todayIsWorkingDay ? ' · today is a day off' : ''}</p>`;

  const holidays = upcoming.length
    ? `<p class="meta">Days off ahead: ${upcoming.map((d) => `${h(shortDate(d.date))} <span class="muted">${h(d.name)}</span>`).join(', ')}</p>`
    : '';

  const error = s.error ? `<p class="banner">${h(s.error)}</p>` : '';
  const via = s.auth.source === 'device' ? 'GitHub sign-in' : s.auth.source === 'ghCli' ? 'GitHub CLI' : '';
  const who = s.auth.login ? ` as @${h(s.auth.login)}` : '';
  const footer = `<footer><span id="updated">Updated ${relativeTime(u.fetchedAt)}</span>${via ? ` · ${via}${who}` : ''}
    <button class="link" data-act="quit">Quit</button></footer>`;

  const launchPrompt =
    !s.settings.launchPromptDone && !s.launchAtLogin
      ? `<div class="prompt"><span>Start Copilot Pacer automatically when you log in?</span>
          <span class="prompt-actions"><button class="primary" data-act="launch-yes">Turn on</button>
          <button class="link" data-act="launch-no">Not now</button></span></div>`
      : '';

  root.innerHTML = top + updateBanner(s) + launchPrompt + error + hero + chart + stats + cycle + holidays + footer;
}

function updateBanner(s: Status): string {
  const u = s.update;
  const v = u.latest ? `v${h(u.latest)}` : 'a new version';
  const notes = u.releaseUrl ? `<button class="link" data-act="release-notes">What's new</button>` : '';
  switch (u.state) {
    case 'downloading':
      return `<div class="prompt"><span>Downloading Copilot Pacer ${v}…${
        u.progress != null ? ` ${Math.round(u.progress * 100)}%` : ''
      }</span></div>`;
    case 'ready':
      return `<div class="prompt"><span><b>Copilot Pacer ${v}</b> is ready to install.</span>
        <span class="prompt-actions"><button class="primary" data-act="install-update">Install</button>
        <button class="link" data-act="reveal-update">Show file</button>${notes}</span></div>`;
    case 'noInstaller':
      return `<div class="prompt"><span>Copilot Pacer ${v} is available.</span>
        <span class="prompt-actions"><button class="primary" data-act="release-notes">Download page</button></span></div>`;
  }
  if (!u.manual) return '';
  if (u.state === 'checking') return `<div class="prompt"><span>Checking for updates…</span></div>`;
  if (u.state === 'upToDate')
    return `<div class="prompt"><span>You're on the latest version (${h(s.version)}).</span>
      <span class="prompt-actions"><button class="link" data-act="dismiss-update">OK</button></span></div>`;
  if (u.state === 'error')
    return `<p class="banner">Update check failed: ${h(u.error ?? 'unknown error')}
      <button class="link" data-act="dismiss-update">Dismiss</button></p>`;
  return '';
}

function updateStatusText(): string {
  const s = status;
  if (!s) return '';
  const u = s.update;
  const v = u.latest ? `v${h(u.latest)}` : '';
  switch (u.state) {
    case 'checking':
      return 'Checking…';
    case 'downloading':
      return `Downloading ${v}…${u.progress != null ? ` ${Math.round(u.progress * 100)}%` : ''}`;
    case 'ready':
      return `${v} downloaded and ready to install. <button type="button" class="link" data-act="install-update">Install</button>`;
    case 'noInstaller':
      return `${v} is available. <button type="button" class="link" data-act="release-notes">Download page</button>`;
    case 'upToDate':
      return "You're up to date.";
    case 'error':
      return `<span class="warn">${h(u.error ?? 'Update check failed')}</span>`;
    default:
      return '';
  }
}

function signInCard(s: Status) {
  return `<section class="card">
    <h2>Connect to GitHub Copilot</h2>
    <p>Copilot Pacer reads the same credit meter the Copilot editor extension shows.</p>
    <button class="primary" data-act="device-login">Sign in with GitHub</button>
    <p class="hint">Use the GitHub CLI? Run <code>gh auth login</code>, then <button class="link" data-act="refresh">try again</button>.</p>
    ${s.error && !s.error.startsWith('Not signed in') ? `<p class="banner">${h(s.error)}</p>` : ''}
  </section>`;
}

function deviceCard(d: { code: DeviceCode; error?: string }) {
  return `<section class="card">
    <h2>Sign in with GitHub</h2>
    <p>Enter this code on the GitHub page that opened in your browser:</p>
    <div class="code" data-act="copy-code" title="Click to copy">${h(d.code.userCode)}</div>
    <div class="row">
      <button data-act="copy-code">Copy code</button>
      <button data-act="open-github">Open GitHub</button>
    </div>
    ${d.error ? `<p class="banner">${h(d.error)}</p>` : `<p class="hint"><span class="pulse"></span>Waiting for you to approve…</p>`}
    <button class="link" data-act="cancel-device">Cancel</button>
  </section>`;
}

function renderSettings() {
  const d = draft!;
  const s = status;
  const regionChips = d.holidayRegions.length
    ? d.holidayRegions.map((r) => `<span class="chip">${h(regionLabel(r))}<button type="button" data-act="rm-region" data-v="${h(r)}" title="Remove">${ICON.close}</button></span>`).join('')
    : `<span class="muted">No holiday calendars</span>`;
  const dayChips = d.customDaysOff.length
    ? d.customDaysOff.map((c) => `<span class="chip">${h(shortDate(c))}<button type="button" data-act="rm-day" data-v="${h(c)}" title="Remove">${ICON.close}</button></span>`).join('')
    : `<span class="muted">None</span>`;

  let preview = '';
  if (s) {
    const offs = cycleDaysOff(d, s.pace.cycleStart, s.pace.cycleEnd);
    preview = `<p class="hint">This cycle: ${
      offs.length ? offs.map((o) => `${h(shortDate(o.date))} <span class="muted">${h(o.name)}</span>`).join(', ') : 'no days off'
    }</p>`;
  }
  const ghReset = s?.usage?.githubResetDate?.slice(0, 10);

  root.innerHTML =
    header(
      `<button class="icon-btn" data-act="back" title="Back">${ICON.back}</button><div class="brand">Settings</div>`,
      '',
    ) +
    `<form id="settings-form" class="settings" autocomplete="off">
      <fieldset>
        <legend>General</legend>
        <label class="toggle"><input type="checkbox" name="launchAtLogin" ${launchDraft ? 'checked' : ''}> Launch at login</label>
        <label class="toggle"><input type="checkbox" name="showInDock" ${d.showInDock ? 'checked' : ''}> ${
          IS_MAC ? 'Show in Dock' : 'Show in taskbar while open'
        }</label>
        ${IS_MAC ? `<label class="toggle"><input type="checkbox" name="showPercentInMenuBar" ${d.showPercentInMenuBar ? 'checked' : ''}> Show % next to the menu-bar icon</label>` : ''}
        <p class="hint">${IS_MAC ? 'Copilot Pacer always stays in the menu bar.' : 'Copilot Pacer always stays in the system tray.'}</p>
        <label class="toggle"><input type="checkbox" name="checkUpdates" ${d.checkUpdates ? 'checked' : ''}> Check for updates automatically</label>
        <div class="field"><span>Version ${h(s?.version ?? '')}</span>
          <button type="button" data-act="check-updates">Check now</button></div>
        <p class="hint" id="update-status">${updateStatusText()}</p>
      </fieldset>

      <fieldset>
        <legend>Billing cycle</legend>
        <label class="field"><span>Credits reset on day</span>
          <select name="resetDay">${Array.from({ length: 31 }, (_, i) => i + 1)
            .map((n) => `<option value="${n}" ${n === d.resetDay ? 'selected' : ''}>${n}</option>`)
            .join('')}</select></label>
        <p class="hint">${ghReset ? `GitHub reports your next reset on ${h(shortDate(ghReset))}. ` : ''}Short months use their last day.</p>
      </fieldset>

      <fieldset>
        <legend>Work week</legend>
        <div class="weekdays">${WEEKDAYS.map(
          (w, i) => `<label title="${w}"><input type="checkbox" name="wd${i}" ${d.workDays[i] ? 'checked' : ''}><span>${w.slice(0, 2)}</span></label>`,
        ).join('')}</div>
        <div class="row2">
          <label class="field"><span>Start</span><input type="time" name="workStart" value="${h(d.workStart)}"></label>
          <label class="field"><span>End</span><input type="time" name="workEnd" value="${h(d.workEnd)}"></label>
        </div>
        <p class="hint">Today's share of the budget grows across these hours.</p>
      </fieldset>

      <fieldset>
        <legend>Holidays</legend>
        <div class="chips">${regionChips}</div>
        <div class="row2 add">
          <select id="add-country"><option value="">Add a country…</option>${getCountries()
            .map((c) => `<option value="${c.code}">${h(c.name)}</option>`)
            .join('')}</select>
          <select id="add-state" hidden></select>
          <button type="button" data-act="add-region">Add</button>
        </div>
        <div class="checks">${HOLIDAY_TYPES.map(
          (t) => `<label><input type="checkbox" name="type-${t.id}" ${d.holidayTypes.includes(t.id) ? 'checked' : ''}> ${t.label}</label>`,
        ).join('')}</div>
        <span class="sub">Personal days off</span>
        <div class="chips">${dayChips}</div>
        <div class="row2 add"><input type="date" id="add-day"><button type="button" data-act="add-day">Add</button></div>
        ${preview}
      </fieldset>

      <fieldset>
        <legend>Updates &amp; alerts</legend>
        <label class="field"><span>Check usage every</span>
          <select name="refreshMinutes">${[5, 10, 15, 30, 60]
            .map((m) => `<option value="${m}" ${m === d.refreshMinutes ? 'selected' : ''}>${m} minutes</option>`)
            .join('')}</select></label>
        <label class="toggle"><input type="checkbox" name="notifyOverPace" ${d.notifyOverPace ? 'checked' : ''}> Notify me when I'm over pace</label>
        <label class="field"><span>Over-pace tolerance</span>
          <span class="suffix"><input type="number" name="overMargin" min="0" max="50" step="0.5" value="${d.overMargin}"> pts</span></label>
      </fieldset>

      <fieldset>
        <legend>Account</legend>
        <div id="account">${accountBlock()}</div>
      </fieldset>

      <fieldset>
        <legend>Network</legend>
        <label class="field"><span>Proxy</span>
          <select name="proxyMode">${[
            ['auto', 'Automatic (system)'],
            ['manual', 'Manual'],
            ['none', 'No proxy'],
          ]
            .map(([v, l]) => `<option value="${v}" ${d.proxyMode === v ? 'selected' : ''}>${l}</option>`)
            .join('')}</select></label>
        <div id="proxy-auto" ${d.proxyMode === 'auto' ? '' : 'hidden'}>
          <p class="hint" id="proxy-detected">${networkText()}</p>
          <button type="button" data-act="redetect">Detect again</button>
        </div>
        <div id="proxy-manual" ${d.proxyMode === 'manual' ? '' : 'hidden'}>
          <label class="field col"><span>Proxy URL</span>
            <input type="text" name="proxyUrl" placeholder="http://proxy.example.com:8080" value="${h(d.proxyUrl)}"></label>
          <p class="hint">http://, https:// or socks5:// — credentials as http://user:pass@host:port.</p>
        </div>
      </fieldset>

      <div class="save-bar"><span class="msg">${h(saveMessage)}</span><button class="primary" type="submit">Save</button></div>
    </form>`;
}

function networkText() {
  const n = status?.network;
  if (!n || n.mode !== 'auto') return '';
  if (!n.source) return 'Not detected yet. It runs on the next request.';
  return n.proxy ? `Using <b>${h(n.proxy)}</b> from ${h(n.source)}.` : `Direct connection: ${h(n.source)}.`;
}

function accountBlock() {
  const s = status;
  const who = s?.auth.login ? ` as <b>@${h(s.auth.login)}</b>` : '';
  if (s?.auth.source === 'device') {
    return `<p>Signed in with GitHub${who}.</p><button type="button" data-act="sign-out">Sign out</button>`;
  }
  if (s?.auth.source === 'ghCli') {
    return `<p>Using your GitHub CLI login${who}.</p><button type="button" data-act="device-login">Sign in with GitHub instead</button>`;
  }
  return `<p>Not signed in.</p><button type="button" class="primary" data-act="device-login">Sign in with GitHub</button>`;
}

// ---------------------------------------------------------------------------
// Settings form <-> draft
// ---------------------------------------------------------------------------

function syncDraftFromForm() {
  const form = document.getElementById('settings-form') as HTMLFormElement | null;
  if (!form || !draft) return;
  const el = (n: string) => form.elements.namedItem(n) as HTMLInputElement | HTMLSelectElement | null;
  const checked = (n: string) => (el(n) as HTMLInputElement | null)?.checked ?? false;
  draft.resetDay = Number(el('resetDay')?.value ?? draft.resetDay);
  draft.workDays = WEEKDAYS.map((_, i) => checked(`wd${i}`));
  draft.workStart = el('workStart')?.value || draft.workStart;
  draft.workEnd = el('workEnd')?.value || draft.workEnd;
  draft.holidayTypes = HOLIDAY_TYPES.filter((t) => checked(`type-${t.id}`)).map((t) => t.id);
  draft.refreshMinutes = Number(el('refreshMinutes')?.value ?? draft.refreshMinutes);
  draft.notifyOverPace = checked('notifyOverPace');
  const margin = Number(el('overMargin')?.value);
  draft.overMargin = Number.isFinite(margin) ? margin : draft.overMargin;
  if (el('showPercentInMenuBar')) draft.showPercentInMenuBar = checked('showPercentInMenuBar');
  draft.proxyMode = (el('proxyMode')?.value as Settings['proxyMode']) ?? draft.proxyMode;
  draft.proxyUrl = el('proxyUrl')?.value.trim() ?? '';
  draft.showInDock = checked('showInDock');
  launchDraft = checked('launchAtLogin');
  draft.checkUpdates = checked('checkUpdates');
}

async function openSettings() {
  if (!status) return;
  draft = structuredClone(status.settings);
  launchDraft = status.launchAtLogin;
  saveMessage = '';
  view = 'settings';
  render();
  root.scrollTop = 0;
}

async function saveSettings() {
  if (!draft) return;
  syncDraftFromForm();
  draft.excludedDays = computeExcludedDays(draft.holidayRegions, draft.holidayTypes, draft.customDaysOff);
  try {
    status = await invoke<Status>('save_settings', { settings: draft });
    if (launchDraft !== status.launchAtLogin) {
      status = await invoke<Status>('set_launch_at_login', { enabled: launchDraft });
    }
    view = 'overview';
    draft = null;
    render();
  } catch (e) {
    saveMessage = String(e);
    renderSettings();
  }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function refresh() {
  if (status) {
    status.fetching = true;
    if (view === 'overview') render();
  }
  try {
    status = await invoke<Status>('refresh');
  } catch (e) {
    if (status) status.error = String(e);
  }
  if (view === 'overview') render();
}

async function startDeviceLogin() {
  view = 'overview';
  draft = null;
  try {
    await invoke('set_pinned', { pinned: true });
    const code = await invoke<DeviceCode>('start_device_login');
    device = { code };
    void navigator.clipboard?.writeText(code.userCode).catch(() => {});
  } catch (e) {
    await invoke('set_pinned', { pinned: false });
    if (status) status.error = String(e);
  }
  render();
}

async function endDeviceLogin(error?: string) {
  if (error && device) {
    device.error = error;
  } else {
    device = null;
  }
  await invoke('set_pinned', { pinned: false });
  render();
}

root.addEventListener('click', async (ev) => {
  const t = (ev.target as HTMLElement).closest<HTMLElement>('[data-act]');
  if (!t) return;
  const act = t.dataset.act;
  switch (act) {
    case 'refresh':
      return refresh();
    case 'settings':
      return openSettings();
    case 'back':
      view = 'overview';
      draft = null;
      return render();
    case 'check-updates':
      try {
        status = await invoke<Status>('check_for_updates');
      } catch (e) {
        if (status) status.error = String(e);
      }
      if (view === 'settings') {
        const el = document.getElementById('update-status');
        if (el) el.innerHTML = 'Checking…';
      } else render();
      return;
    case 'install-update':
      try {
        await invoke('install_update');
      } catch (e) {
        if (status) {
          status.update.state = 'error';
          status.update.manual = true;
          status.update.error = String(e);
        }
        render();
      }
      return;
    case 'reveal-update':
      await invoke('reveal_update').catch(() => {});
      return;
    case 'release-notes':
      if (status?.update.releaseUrl) await invoke('open_url', { url: status.update.releaseUrl }).catch(() => {});
      return;
    case 'dismiss-update':
      await invoke('dismiss_update_result');
      if (status) status.update.manual = false;
      return render();
    case 'launch-yes':
    case 'launch-no':
      try {
        status = await invoke<Status>(act === 'launch-yes' ? 'set_launch_at_login' : 'dismiss_launch_prompt', {
          enabled: true,
        });
      } catch (e) {
        if (status) status.error = String(e);
      }
      return render();
    case 'quit':
      return invoke('quit_app');
    case 'device-login':
      return startDeviceLogin();
    case 'copy-code':
      if (device) {
        await navigator.clipboard?.writeText(device.code.userCode).catch(() => {});
        t.classList.add('copied');
        setTimeout(() => t.classList.remove('copied'), 900);
      }
      return;
    case 'open-github':
      if (device) await invoke('open_url', { url: device.code.verificationUri });
      return;
    case 'cancel-device':
      await invoke('cancel_device_login');
      return endDeviceLogin();
    case 'redetect': {
      const out = document.getElementById('proxy-detected');
      if (out) out.textContent = 'Detecting…';
      (t as HTMLButtonElement).disabled = true;
      try {
        status = await invoke<Status>('redetect_proxy');
      } catch (e) {
        if (status) status.error = String(e);
      }
      (t as HTMLButtonElement).disabled = false;
      if (out) out.innerHTML = networkText() + (status?.error ? ` <span class="warn">${h(status.error)}</span>` : '');
      return;
    }
    case 'sign-out':
      status = await invoke<Status>('sign_out');
      if (view === 'settings') {
        const acc = document.getElementById('account');
        if (acc) acc.innerHTML = accountBlock();
      } else render();
      return;
    case 'rm-region':
      syncDraftFromForm();
      draft!.holidayRegions = draft!.holidayRegions.filter((r) => r !== t.dataset.v);
      return renderSettings();
    case 'rm-day':
      syncDraftFromForm();
      draft!.customDaysOff = draft!.customDaysOff.filter((r) => r !== t.dataset.v);
      return renderSettings();
    case 'add-region': {
      const c = (document.getElementById('add-country') as HTMLSelectElement).value;
      const st = (document.getElementById('add-state') as HTMLSelectElement).value;
      if (!c) return;
      const code = st ? `${c}-${st}` : c;
      syncDraftFromForm();
      // Replace another entry for the same country (e.g. "GB" → "GB-ENG").
      draft!.holidayRegions = [...draft!.holidayRegions.filter((r) => parseRegion(r).country !== c), code];
      return renderSettings();
    }
    case 'add-day': {
      const v = (document.getElementById('add-day') as HTMLInputElement).value;
      if (!v) return;
      syncDraftFromForm();
      if (!draft!.customDaysOff.includes(v)) draft!.customDaysOff = [...draft!.customDaysOff, v].sort();
      return renderSettings();
    }
  }
});

root.addEventListener('change', (ev) => {
  const t = ev.target as HTMLElement;
  if ((t as HTMLSelectElement).name === 'proxyMode') {
    const mode = (t as HTMLSelectElement).value;
    document.getElementById('proxy-auto')!.hidden = mode !== 'auto';
    document.getElementById('proxy-manual')!.hidden = mode !== 'manual';
  } else if (t.id === 'add-country') {
    const sel = document.getElementById('add-state') as HTMLSelectElement;
    const states = listStates((t as HTMLSelectElement).value);
    sel.hidden = states.length === 0;
    sel.innerHTML =
      `<option value="">Whole country</option>` + states.map((s) => `<option value="${s.code}">${h(s.name)}</option>`).join('');
  } else if (view === 'settings' && (t as HTMLInputElement).name?.startsWith('type-')) {
    // Holiday types changed: refresh the preview.
    syncDraftFromForm();
    renderSettings();
  }
});

root.addEventListener('submit', (ev) => {
  ev.preventDefault();
  void saveSettings();
});

document.addEventListener('keydown', (ev) => {
  if (ev.key !== 'Escape') return;
  if (view === 'settings') {
    view = 'overview';
    draft = null;
    render();
  } else {
    void invoke('hide_window');
  }
});

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

async function init() {
  await listen<Status>('status-updated', (e) => {
    status = e.payload;
    if (view === 'overview') render();
    else {
      const acc = document.getElementById('account');
      if (acc) acc.innerHTML = accountBlock();
      const net = document.getElementById('proxy-detected');
      if (net) net.innerHTML = networkText();
      const upd = document.getElementById('update-status');
      if (upd) upd.innerHTML = updateStatusText();
    }
  });
  await listen<boolean>('fetching', () => {
    if (status && view === 'overview') {
      status.fetching = true;
      document.querySelector('[data-act="refresh"]')?.classList.add('spin');
    }
  });
  await listen<{ ok: boolean; error: string | null }>('device-login', (e) => {
    void endDeviceLogin(e.payload.ok ? undefined : e.payload.error ?? 'Sign-in failed');
    if (e.payload.ok && e.payload.error && status) status.error = e.payload.error;
  });
  await listen('show-settings', () => void openSettings());
  await listen('popup-shown', () => {
    if (view === 'overview') render(); // refresh "updated x min ago"
  });

  status = await invoke<Status>('get_status');
  const s = status.settings;
  if (!s.initialized) {
    const defaults = firstRunDefaults(Intl.DateTimeFormat().resolvedOptions().timeZone, navigator.language);
    const known = new Set(getCountries().map((c) => c.code));
    const next: Settings = {
      ...s,
      workDays: defaults.workDays,
      holidayRegions: defaults.holidayRegions.filter((r) => known.has(r)),
    };
    next.excludedDays = computeExcludedDays(next.holidayRegions, next.holidayTypes, next.customDaysOff);
    status = await invoke<Status>('save_settings', { settings: next });
  } else {
    // Keep holiday dates fresh (library updates, new year).
    await invoke('set_excluded_days', {
      days: computeExcludedDays(s.holidayRegions, s.holidayTypes, s.customDaysOff),
    });
  }
  render();

  setInterval(() => {
    const el = document.getElementById('updated');
    if (el && status?.usage) el.textContent = `Updated ${relativeTime(status.usage.fetchedAt)}`;
  }, 30_000);
}

init().catch((e) => {
  root.innerHTML = `<p class="banner">Failed to start: ${h(String(e))}</p>`;
});
