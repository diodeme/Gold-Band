import { describe, expect, it } from 'vitest';
import { formatCompactRelativeTime } from '@/lib/datetime';

const NOW_MS = Date.UTC(2026, 6, 28, 0, 0, 0);
const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

function timestampAgo(elapsedMs: number) {
  return new Date(NOW_MS - elapsedMs).toISOString();
}

describe('formatCompactRelativeTime', () => {
  it.each([
    [0, '刚刚'],
    [59 * MINUTE_MS, '59m'],
    [60 * MINUTE_MS, '1h'],
    [23 * HOUR_MS, '23h'],
    [24 * HOUR_MS, '1d'],
    [6 * DAY_MS, '6d'],
    [7 * DAY_MS, '1w'],
    [27 * DAY_MS, '3w'],
    [28 * DAY_MS, '4w'],
    [29 * DAY_MS, '4w'],
    [30 * DAY_MS, '1mo'],
    [359 * DAY_MS, '11mo'],
    [360 * DAY_MS, '12mo'],
    [364 * DAY_MS, '12mo'],
    [365 * DAY_MS, '1y'],
  ])('formats an elapsed duration of %i ms as %s', (elapsedMs, expected) => {
    expect(formatCompactRelativeTime(timestampAgo(elapsedMs), '刚刚', NOW_MS)).toBe(expected);
  });

  it('supports the internal Unix-seconds timestamp format', () => {
    const oneHourAgo = `${Math.floor((NOW_MS - HOUR_MS) / 1000)}Z`;

    expect(formatCompactRelativeTime(oneHourAgo, '刚刚', NOW_MS)).toBe('1h');
  });

  it('treats future timestamps as just now and hides invalid timestamps', () => {
    expect(formatCompactRelativeTime(new Date(NOW_MS + HOUR_MS).toISOString(), '刚刚', NOW_MS)).toBe('刚刚');
    expect(formatCompactRelativeTime('not-a-time', '刚刚', NOW_MS)).toBe('');
    expect(formatCompactRelativeTime(null, '刚刚', NOW_MS)).toBe('');
  });
});
