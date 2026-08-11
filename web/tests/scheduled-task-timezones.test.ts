import { describe, expect, it } from 'vitest';

import {
  getScheduledSystemTimezone,
  getScheduledTimezones,
} from '../src/lib/scheduled-task-timezones';
import {
  getPreferredScheduledTimezone,
  rememberScheduledTimezone,
  SCHEDULED_TIMEZONE_STORAGE_KEY,
} from '../src/lib/scheduled-task-timezone-preference';

describe('scheduled task timezones', () => {
  it('provides the complete IANA catalog with UTC and the system zone', () => {
    const zones = getScheduledTimezones();
    const systemZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

    expect(zones.length).toBeGreaterThan(300);
    expect(zones).toContain('UTC');
    expect(zones).toContain(systemZone);
    expect(new Set(zones).size).toBe(zones.length);
  });

  it('uses a valid system IANA timezone', () => {
    expect(getScheduledSystemTimezone(() => 'Asia/Shanghai')).toBe('Asia/Shanghai');
  });

  it('falls back to UTC for an empty or invalid system timezone', () => {
    expect(getScheduledSystemTimezone(() => '')).toBe('UTC');
    expect(getScheduledSystemTimezone(() => 'Invalid/Zone')).toBe('UTC');
  });

  it('falls back to UTC when resolving the system timezone throws', () => {
    expect(getScheduledSystemTimezone(() => {
      throw new Error('resolver failed');
    })).toBe('UTC');
  });

  it('defaults to the system timezone and remembers the latest valid user choice', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    expect(getPreferredScheduledTimezone(storage, 'Asia/Hong_Kong')).toBe('Asia/Hong_Kong');
    expect(rememberScheduledTimezone('America/New_York', storage)).toBe(true);
    expect(values.get(SCHEDULED_TIMEZONE_STORAGE_KEY)).toBe('America/New_York');
    expect(getPreferredScheduledTimezone(storage, 'Asia/Hong_Kong')).toBe('America/New_York');
    expect(rememberScheduledTimezone('Invalid/Zone', storage)).toBe(false);
    expect(getPreferredScheduledTimezone(storage, 'Asia/Hong_Kong')).toBe('America/New_York');
  });
});
