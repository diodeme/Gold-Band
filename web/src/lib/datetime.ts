function parseTimestamp(value?: string | null) {
  if (!value) return null;
  const trimmed = value.trim();
  const epoch = trimmed.match(/^(\d+(?:\.\d+)?)Z?$/);
  const date = epoch ? new Date(Number(epoch[1]) * 1000) : new Date(trimmed);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatLocalDateTime(value?: string | null, fallback = '-') {
  if (!value) return fallback;
  const date = parseTimestamp(value);
  if (!date) return value;
  const pad = (part: number) => part.toString().padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
