import { describe, expect, it } from 'vitest';

import {
  analyzeScheduledLocalTime,
  scheduledScheduleSpecFromInput,
  validateScheduledCron,
  validateScheduledEvery,
  validateScheduledWeeklyDays,
} from '../src/lib/scheduled-task-authoring';

describe('scheduled task authoring', () => {
  it('resolves an ordinary local wall-clock time', () => {
    expect(analyzeScheduledLocalTime('2026-08-10', '09:30', 'Asia/Shanghai')).toEqual({
      kind: 'valid',
      earlierInstant: '2026-08-10T01:30:00Z',
      earlierOffset: '+08:00',
    });
  });

  it('rejects invalid local date, time, and timezone values', () => {
    expect(analyzeScheduledLocalTime('2026-02-30', '09:30', 'UTC')).toEqual({ kind: 'invalid' });
    expect(analyzeScheduledLocalTime('2026-08-10', '25:00', 'UTC')).toEqual({ kind: 'invalid' });
    expect(analyzeScheduledLocalTime('2026-08-10', '09:30', 'Invalid/Zone')).toEqual({ kind: 'invalid' });
  });

  it('detects a nonexistent DST wall-clock time', () => {
    expect(analyzeScheduledLocalTime('2026-03-08', '02:30', 'America/New_York')).toEqual({
      kind: 'nonexistent',
    });
  });

  it('returns both candidates for an ambiguous DST wall-clock time', () => {
    expect(analyzeScheduledLocalTime('2026-11-01', '01:30', 'America/New_York')).toEqual({
      kind: 'ambiguous',
      earlierInstant: '2026-11-01T05:30:00Z',
      laterInstant: '2026-11-01T06:30:00Z',
      earlierOffset: '-04:00',
      laterOffset: '-05:00',
    });
  });

  it('normalizes an authoring At input for browser preview results', () => {
    expect(scheduledScheduleSpecFromInput({
      kind: 'At',
      localDate: '2026-11-01',
      localTime: '01:30',
      timezone: 'America/New_York',
      disambiguation: 'later',
    })).toEqual({
      kind: 'At',
      at: '2026-11-01T06:30:00Z',
      timezone: 'America/New_York',
    });
  });

  it('accepts only the six-field Cron product syntax', () => {
    expect(validateScheduledCron('0 0 9 * * MON-FRI')).toBeNull();
    expect(validateScheduledCron('0 9 * * MON-FRI')).toBe('invalid-cron');
    expect(validateScheduledCron('not a cron')).toBe('invalid-cron');
  });

  it('requires at least one weekly day', () => {
    expect(validateScheduledWeeklyDays([])).toBe('empty-weekdays');
    expect(validateScheduledWeeklyDays(['Mon'])).toBeNull();
  });

  it('accepts only positive safe integer Every values', () => {
    expect(validateScheduledEvery('1')).toBeNull();
    expect(validateScheduledEvery('0')).toBe('invalid-every-value');
    expect(validateScheduledEvery('-1')).toBe('invalid-every-value');
    expect(validateScheduledEvery('1.5')).toBe('invalid-every-value');
    expect(validateScheduledEvery('')).toBe('invalid-every-value');
    expect(validateScheduledEvery('9007199254740992')).toBe('invalid-every-value');
  });
});
