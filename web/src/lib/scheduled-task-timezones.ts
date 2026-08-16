import { timeZonesNames } from '@vvo/tzdb';

type IntlWithSupportedValues = typeof Intl & {
  supportedValuesOf?: (key: 'timeZone') => string[];
};

type SystemTimezoneResolver = () => string | null | undefined;

const defaultSystemTimezoneResolver: SystemTimezoneResolver = () => (
  Intl.DateTimeFormat().resolvedOptions().timeZone
);

export function getScheduledTimezones(): string[] {
  const supported = (Intl as IntlWithSupportedValues).supportedValuesOf?.('timeZone');
  const systemZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const zones = new Set<string>([
    'UTC',
    ...(supported && supported.length > 0 ? supported : timeZonesNames),
  ]);
  if (systemZone) zones.add(systemZone);
  return Array.from(zones).sort((left, right) => {
    if (left === 'UTC') return -1;
    if (right === 'UTC') return 1;
    return left.localeCompare(right);
  });
}

export function getScheduledSystemTimezone(
  resolver: SystemTimezoneResolver = defaultSystemTimezoneResolver,
): string {
  try {
    const timezone = resolver()?.trim();
    if (!timezone) return 'UTC';
    return getScheduledTimezones().includes(timezone) ? timezone : 'UTC';
  } catch {
    return 'UTC';
  }
}
