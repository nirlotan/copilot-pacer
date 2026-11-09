// Pure helpers for turning holiday time ranges into whole local days off.

export function ymd(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function parseYmd(s: string): Date {
  const [y, m, d] = s.split('-').map(Number);
  return new Date(y, m - 1, d);
}

/**
 * Local calendar days whose noon falls inside [start, end).
 *
 * Holidays that run sunset-to-sunset (e.g. Jewish holidays start the evening
 * before) then count only for the actual day, while ordinary midnight-to-
 * midnight holidays count for their date.
 */
export function daysCoveredAtNoon(start: Date, end: Date): string[] {
  const out: string[] = [];
  const day = new Date(start.getFullYear(), start.getMonth(), start.getDate());
  while (day < end) {
    const noon = new Date(day.getFullYear(), day.getMonth(), day.getDate(), 12);
    if (noon >= start && noon < end) out.push(ymd(day));
    day.setDate(day.getDate() + 1);
  }
  return out;
}

/** "GB-ENG" → { country: "GB", state: "ENG" } */
export function parseRegion(code: string): { country: string; state?: string } {
  const [country, state] = code.split('-');
  return state ? { country, state } : { country };
}

/** Sensible first-run defaults from the system time zone and locale. */
export function firstRunDefaults(timeZone: string, locale: string): {
  workDays: boolean[];
  holidayRegions: string[];
} {
  const sunToThu = [true, true, true, true, true, false, false];
  const monToFri = [false, true, true, true, true, true, false];
  if (timeZone === 'Asia/Jerusalem' || timeZone === 'Asia/Tel_Aviv') {
    return { workDays: sunToThu, holidayRegions: ['IL'] };
  }
  const region = locale.split(/[-_]/)[1];
  return {
    workDays: monToFri,
    holidayRegions: region && /^[A-Z]{2}$/.test(region) ? [region] : [],
  };
}
