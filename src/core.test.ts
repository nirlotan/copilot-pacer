import { test } from 'node:test';
import assert from 'node:assert/strict';
import { daysCoveredAtNoon, firstRunDefaults, parseRegion, ymd } from './holidays-core';
import { days, lastDayOf, paceSentence, relativeTime } from './format';
import { chartSvg } from './chart';

test('midnight-to-midnight holiday covers its own date', () => {
  assert.deepEqual(daysCoveredAtNoon(new Date(2026, 11, 25), new Date(2026, 11, 26)), ['2026-12-25']);
});

test('sunset-to-sunset holiday counts only the actual day', () => {
  // Yom Kippur 2026: 20 Sep 18:00 → 21 Sep 18:00
  assert.deepEqual(daysCoveredAtNoon(new Date(2026, 8, 20, 18), new Date(2026, 8, 21, 18)), ['2026-09-21']);
});

test('multi-day holiday covers every day', () => {
  assert.deepEqual(daysCoveredAtNoon(new Date(2026, 3, 1, 18), new Date(2026, 3, 4, 18)), [
    '2026-04-02',
    '2026-04-03',
    '2026-04-04',
  ]);
});

test('ymd uses local calendar', () => {
  assert.equal(ymd(new Date(2026, 0, 5, 23, 59)), '2026-01-05');
});

test('regions', () => {
  assert.deepEqual(parseRegion('GB-ENG'), { country: 'GB', state: 'ENG' });
  assert.deepEqual(parseRegion('IL'), { country: 'IL' });
});

test('first-run defaults', () => {
  const il = firstRunDefaults('Asia/Jerusalem', 'he-IL');
  assert.deepEqual(il.holidayRegions, ['IL']);
  assert.equal(il.workDays[0], true); // Sunday
  assert.equal(il.workDays[5], false); // Friday
  const gb = firstRunDefaults('Europe/London', 'en-GB');
  assert.deepEqual(gb.holidayRegions, ['GB']);
  assert.equal(gb.workDays[0], false);
  assert.deepEqual(firstRunDefaults('UTC', 'en').holidayRegions, []);
});

test('formatting', () => {
  assert.equal(lastDayOf('2026-12-01'), '2026-11-30');
  assert.equal(lastDayOf('2027-01-01'), '2026-12-31');
  assert.equal(days(1), '1 day');
  assert.equal(days(14.56), '14.6 days');
  assert.equal(paceSentence(-4.12, 'under'), '4.1 pts under pace');
  assert.equal(paceSentence(6, 'over'), '6.0 pts over pace');
  assert.equal(relativeTime(1000, 1000_000 + 120_000), '2 min ago');
});

test('chart renders pace, usage and now marker', () => {
  const svg = chartSvg({
    curve: [[0, 0], [43200, 100]],
    history: [[600, 2], [3000, 5]],
    nowX: 4000,
    cycleMinutes: 43200,
    current: 6,
  });
  assert.match(svg, /class="pace"/);
  assert.match(svg, /class="usage"/);
  assert.match(svg, /class="now"/);
  assert.ok(!svg.includes('NaN'));
});

test('chart copes with no data', () => {
  const svg = chartSvg({ curve: [], history: [], nowX: 0, cycleMinutes: 0 });
  assert.ok(!svg.includes('NaN'));
});
