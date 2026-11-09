// Holiday calendars via the bundled `date-holidays` library (offline).

import Holidays from 'date-holidays';
import { daysCoveredAtNoon, parseRegion, parseYmd } from './holidays-core';

export const HOLIDAY_TYPES = [
  { id: 'public', label: 'Public holidays' },
  { id: 'bank', label: 'Bank holidays' },
  { id: 'optional', label: 'Optional days' },
  { id: 'school', label: 'School holidays' },
  { id: 'observance', label: 'Observances' },
] as const;

const lang = (navigator.language || 'en').split('-')[0];

export function listCountries(): { code: string; name: string }[] {
  const all = new Holidays().getCountries(lang) as Record<string, string>;
  return Object.entries(all)
    .map(([code, name]) => ({ code, name }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function listStates(country: string): { code: string; name: string }[] {
  const s = new Holidays().getStates(country, lang) as Record<string, string> | undefined;
  return s ? Object.entries(s).map(([code, name]) => ({ code, name })).sort((a, b) => a.name.localeCompare(b.name)) : [];
}

export function regionLabel(code: string): string {
  const { country, state } = parseRegion(code);
  const countries = new Holidays().getCountries(lang) as Record<string, string>;
  const name = countries[country] ?? country;
  if (!state) return name;
  const states = (new Holidays().getStates(country, lang) as Record<string, string> | undefined) ?? {};
  return `${name} – ${states[state] ?? state}`;
}

export interface DayOff {
  date: string;
  name: string;
  region: string;
}

function forRegion(code: string): Holidays | null {
  const { country, state } = parseRegion(code);
  try {
    const hd = state ? new Holidays(country, state) : new Holidays(country);
    return hd;
  } catch {
    return null;
  }
}

export function holidaysForYears(regions: string[], types: string[], years: number[]): DayOff[] {
  const out: DayOff[] = [];
  for (const region of regions) {
    const hd = forRegion(region);
    if (!hd) continue;
    for (const year of years) {
      for (const h of hd.getHolidays(year, lang) || []) {
        if (!types.includes(h.type)) continue;
        for (const date of daysCoveredAtNoon(new Date(h.start), new Date(h.end))) {
          out.push({ date, name: h.name, region });
        }
      }
    }
  }
  return out;
}

/** All excluded dates (holidays + personal days), for last/this/next year. */
export function computeExcludedDays(regions: string[], types: string[], custom: string[], now = new Date()): string[] {
  const y = now.getFullYear();
  const set = new Set<string>(custom);
  for (const h of holidaysForYears(regions, types, [y - 1, y, y + 1])) set.add(h.date);
  return [...set].sort();
}

/** Named days off falling inside [start, endExclusive), for display. */
export function daysOffInRange(
  regions: string[],
  types: string[],
  custom: string[],
  start: string,
  endExclusive: string,
): DayOff[] {
  const s = parseYmd(start);
  const years = [...new Set([s.getFullYear(), parseYmd(endExclusive).getFullYear()])];
  const merged = new Map<string, DayOff>();
  for (const h of holidaysForYears(regions, types, years)) {
    if (h.date < start || h.date >= endExclusive) continue;
    const prev = merged.get(h.date);
    if (prev) {
      if (!prev.name.includes(h.name)) prev.name += ` / ${h.name}`;
    } else merged.set(h.date, { ...h });
  }
  for (const c of custom) {
    if (c >= start && c < endExclusive && !merged.has(c)) merged.set(c, { date: c, name: 'Personal day off', region: '' });
  }
  return [...merged.values()].sort((a, b) => a.date.localeCompare(b.date));
}
