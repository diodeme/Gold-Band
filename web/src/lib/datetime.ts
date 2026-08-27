export function parseTimestamp(value?: string | null) {
  if (!value) return null;
  const trimmed = value.trim();
  const epoch = trimmed.match(/^(\d+(?:\.\d+)?)Z?$/);
  const date = epoch ? new Date(Number(epoch[1]) * 1000) : new Date(trimmed);
  return Number.isNaN(date.getTime()) ? null : date;
}

const MILLISECONDS_PER_MINUTE = 60_000;
const MINUTES_PER_HOUR = 60;
const HOURS_PER_DAY = 24;
const DAYS_PER_WEEK = 7;
const DAYS_PER_MONTH = 30;
const DAYS_PER_YEAR = 365;

const COMPACT_RELATIVE_TIME_UNITS = {
  minute: 'm',
  hour: 'h',
  day: 'd',
  week: 'w',
  month: 'mo',
  year: 'y',
} as const;

export function formatLocalDateTime(value?: string | null, fallback = '-') {
  if (!value) return fallback;
  const date = parseTimestamp(value);
  if (!date) return value;
  const pad = (part: number) => part.toString().padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function formatCompactRelativeTime(
  value: string | null | undefined,
  justNowLabel: string,
  nowMs = Date.now(),
) {
  const date = parseTimestamp(value);
  if (!date) return '';

  const elapsedMs = Math.max(0, nowMs - date.getTime());
  const minutes = Math.floor(elapsedMs / MILLISECONDS_PER_MINUTE);
  if (minutes < 1) return justNowLabel;
  if (minutes < MINUTES_PER_HOUR) return `${minutes}${COMPACT_RELATIVE_TIME_UNITS.minute}`;

  const hours = Math.floor(minutes / MINUTES_PER_HOUR);
  if (hours < HOURS_PER_DAY) return `${hours}${COMPACT_RELATIVE_TIME_UNITS.hour}`;

  const days = Math.floor(hours / HOURS_PER_DAY);
  if (days < DAYS_PER_WEEK) return `${days}${COMPACT_RELATIVE_TIME_UNITS.day}`;
  if (days < DAYS_PER_MONTH) return `${Math.floor(days / DAYS_PER_WEEK)}${COMPACT_RELATIVE_TIME_UNITS.week}`;
  if (days < DAYS_PER_YEAR) return `${Math.floor(days / DAYS_PER_MONTH)}${COMPACT_RELATIVE_TIME_UNITS.month}`;
  return `${Math.floor(days / DAYS_PER_YEAR)}${COMPACT_RELATIVE_TIME_UNITS.year}`;
}

export function formatAgentMessageDetailedTime(
  value: string | null | undefined,
  justNowLabel: string,
  nowMs = Date.now(),
) {
  const date = parseTimestamp(value);
  if (!date) return '--:--';
  const pad = (part: number) => part.toString().padStart(2, '0');
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const relative = formatCompactRelativeTime(value, justNowLabel, nowMs);
  const differentYear = date.getFullYear() !== new Date(nowMs).getFullYear();
  const olderThanOneWeek = Math.max(0, nowMs - date.getTime()) > DAYS_PER_WEEK * HOURS_PER_DAY * MINUTES_PER_HOUR * MILLISECONDS_PER_MINUTE;
  const dateSuffix = differentYear
    ? `${pad(date.getMonth() + 1)}/${pad(date.getDate())}/${date.getFullYear()}`
    : olderThanOneWeek
      ? `${pad(date.getMonth() + 1)}/${pad(date.getDate())}`
      : '';
  return [time, relative, dateSuffix].filter(Boolean).join('  ');
}
