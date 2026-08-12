import { getScheduledSystemTimezone, getScheduledTimezones } from './scheduled-task-timezones';

export const SCHEDULED_TIMEZONE_STORAGE_KEY = 'gold-band:scheduled-task-timezone';

type TimezoneStorage = Pick<Storage, 'getItem' | 'setItem'>;

function browserStorage(): TimezoneStorage | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function getPreferredScheduledTimezone(
  storage: TimezoneStorage | null = browserStorage(),
  systemTimezone = getScheduledSystemTimezone(),
) {
  try {
    const stored = storage?.getItem(SCHEDULED_TIMEZONE_STORAGE_KEY)?.trim();
    if (stored && getScheduledTimezones().includes(stored)) return stored;
  } catch {
    // Fall back to the system timezone when preference storage is unavailable.
  }
  return systemTimezone;
}

export function rememberScheduledTimezone(
  timezone: string,
  storage: TimezoneStorage | null = browserStorage(),
) {
  if (!getScheduledTimezones().includes(timezone)) return false;
  try {
    storage?.setItem(SCHEDULED_TIMEZONE_STORAGE_KEY, timezone);
    return storage !== null;
  } catch {
    return false;
  }
}
