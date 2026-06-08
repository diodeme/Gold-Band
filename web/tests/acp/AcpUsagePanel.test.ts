import { describe, expect, it } from 'vitest';
import { formatTokenCount } from '../../src/lib/format-token';

describe('formatTokenCount', () => {
  it('formats 0 as raw number', () => {
    expect(formatTokenCount(0)).toBe('0');
  });

  it('formats numbers below 1K as raw number', () => {
    expect(formatTokenCount(842)).toBe('842');
    expect(formatTokenCount(999)).toBe('999');
  });

  it('formats 1K with .0 suffix', () => {
    expect(formatTokenCount(1000)).toBe('1.0K');
  });

  it('formats numbers in K range with one decimal', () => {
    expect(formatTokenCount(1234)).toBe('1.2K');
    expect(formatTokenCount(12000)).toBe('12.0K');
    expect(formatTokenCount(123456)).toBe('123.5K');
  });

  it('formats 1M with .0 suffix', () => {
    expect(formatTokenCount(1_000_000)).toBe('1.0M');
  });

  it('formats numbers in M range with one decimal', () => {
    expect(formatTokenCount(1_234_567)).toBe('1.2M');
    expect(formatTokenCount(12_345_678)).toBe('12.3M');
  });

  it('is a pure function (same input → same output)', () => {
    expect(formatTokenCount(42)).toBe(formatTokenCount(42));
  });
});
