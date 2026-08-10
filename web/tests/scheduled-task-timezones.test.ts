import { describe, expect, it } from 'vitest';

import {
  getScheduledSystemTimezone,
  getScheduledTimezones,
} from '../src/lib/scheduled-task-timezones';

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
});
