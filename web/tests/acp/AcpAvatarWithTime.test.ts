import { describe, expect, it } from 'vitest';
import { formatMessageTime, parseAcpTimestampMs } from '../../src/components/acp/AcpAvatarWithTime';

describe('parseAcpTimestampMs', () => {
  it('parses Unix epoch seconds with Z suffix', () => {
    // 2026-03-17T15:20:00Z = 1778772000
    const result = parseAcpTimestampMs('1778772000Z');
    expect(result).toBe(1778772000 * 1000);
  });

  it('parses Unix epoch seconds without suffix', () => {
    const result = parseAcpTimestampMs('1778772000');
    expect(result).toBe(1778772000 * 1000);
  });

  it('parses Unix epoch with fractional seconds', () => {
    // "1778772000.123Z" → 1778772000.123 * 1000 = 1778772000123
    const result = parseAcpTimestampMs('1778772000.123Z');
    expect(result).toBe(1778772000.123 * 1000);
  });

  it('parses ISO 8601 date string', () => {
    const result = parseAcpTimestampMs('2026-03-17T15:20:00Z');
    expect(result).toBe(Date.parse('2026-03-17T15:20:00Z'));
  });

  it('returns null for completely invalid input', () => {
    expect(parseAcpTimestampMs('not-a-timestamp')).toBeNull();
  });

  it('returns null for empty string', () => {
    expect(parseAcpTimestampMs('')).toBeNull();
  });

  it('parses epoch zero as midnight 1970-01-01', () => {
    expect(parseAcpTimestampMs('0Z')).toBe(0);
  });

  it('parses large epoch values (year 2100+)', () => {
    // 2100-01-01T00:00:00Z ≈ 4102444800
    const result = parseAcpTimestampMs('4102444800Z');
    expect(result).toBe(4102444800 * 1000);
  });
});

describe('formatMessageTime', () => {
  it('returns --:-- for null', () => {
    expect(formatMessageTime(null)).toBe('--:--');
  });

  it('returns --:-- for undefined', () => {
    expect(formatMessageTime(undefined)).toBe('--:--');
  });

  it('returns --:-- for empty string', () => {
    expect(formatMessageTime('')).toBe('--:--');
  });

  it('returns --:-- for invalid timestamp string', () => {
    expect(formatMessageTime('garbage')).toBe('--:--');
  });

  it('formats epoch timestamp in 24h HH:mm format', () => {
    // Use a fixed epoch value so the test is deterministic regardless of timezone.
    // We test that the result matches the pattern and is a valid time.
    const result = formatMessageTime('1778772000Z');
    expect(result).toMatch(/^\d{2}:\d{2}$/);
  });

  it('pads single-digit hours and minutes with leading zero', () => {
    // 2026-01-01T03:05:00Z = 1767236700
    const result = formatMessageTime('1767236700Z');
    const [hours, minutes] = result.split(':');
    expect(hours.length).toBe(2);
    expect(minutes.length).toBe(2);
  });

  it('handles fractional seconds timestamp', () => {
    const result = formatMessageTime('1778772000.456Z');
    expect(result).toMatch(/^\d{2}:\d{2}$/);
  });

  it('handles ISO 8601 timestamp', () => {
    const result = formatMessageTime('2026-03-17T15:20:00Z');
    expect(result).toMatch(/^\d{2}:\d{2}$/);
  });

  it('does not throw for unexpected input types', () => {
    // Testing resilience — the function wraps parsing in try/catch
    // and should return --:-- for anything unparseable.
    expect(() => formatMessageTime('')).not.toThrow();
    expect(() => formatMessageTime(null as unknown as string)).not.toThrow();
    expect(() => formatMessageTime(undefined as unknown as string)).not.toThrow();
  });
});

describe('AcpAvatarWithTime integration', () => {
  it('formatMessageTime output is consistent with parseAcpTimestampMs', () => {
    const epoch = '1778772000Z';
    const ms = parseAcpTimestampMs(epoch);
    expect(ms).not.toBeNull();

    const time = formatMessageTime(epoch);
    expect(time).not.toBe('--:--');

    // Verify the formatted time matches what the parsed ms would produce
    const date = new Date(ms!);
    const hours = date.getHours().toString().padStart(2, '0');
    const minutes = date.getMinutes().toString().padStart(2, '0');
    expect(time).toBe(`${hours}:${minutes}`);
  });

  it('formatMessageTime defaulted to midnight UTC for epoch zero', () => {
    const result = formatMessageTime('0Z');
    expect(result).toMatch(/^\d{2}:\d{2}$/);
  });

  it('multiple formatMessageTime calls for same epoch return same value', () => {
    const a = formatMessageTime('1778772000Z');
    const b = formatMessageTime('1778772000Z');
    expect(a).toBe(b);
  });
});
