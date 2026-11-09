import { parseYmd } from './holidays-core';

export function pct(n: number, digits = 0): string {
  return `${n.toFixed(digits)}%`;
}

export function days(n: number): string {
  const r = Math.round(n * 10) / 10;
  const s = Number.isInteger(r) ? String(r) : r.toFixed(1);
  return `${s} ${r === 1 ? 'day' : 'days'}`;
}

export function shortDate(s: string, locale?: string): string {
  return parseYmd(s).toLocaleDateString(locale, { month: 'short', day: 'numeric' });
}

/** Last day of a cycle given its exclusive end. */
export function lastDayOf(endExclusive: string): string {
  const d = parseYmd(endExclusive);
  d.setDate(d.getDate() - 1);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function relativeTime(unixSeconds: number, nowMs = Date.now()): string {
  const s = Math.max(0, Math.round(nowMs / 1000 - unixSeconds));
  if (s < 45) return 'just now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

export function paceSentence(diff: number | null, status: string): string {
  if (status === 'unlimited') return 'Unlimited plan';
  if (diff === null) return 'Waiting for usage…';
  const pts = Math.abs(diff).toFixed(1);
  if (Math.abs(diff) < 0.05) return 'Exactly on pace';
  return diff < 0 ? `${pts} pts under pace` : `${pts} pts over pace`;
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}
