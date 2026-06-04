import { describe, expect, it } from 'vitest';
import { formatTokenCount, usageRatio, ratioPercent } from '../../src/components/acp/AcpUsagePanel';

describe('formatTokenCount', () => {
  it('formats 0', () => {
    expect(formatTokenCount(0)).toBe('0');
  });

  it('formats numbers with locale separators', () => {
    const result = formatTokenCount(1234);
    expect(result).toMatch(/1[,.  ]?234/);
  });

  it('formats large numbers', () => {
    const result = formatTokenCount(1234567);
    expect(result).toContain('1');
    expect(result).toContain('234');
    expect(result).toContain('567');
  });

  it('handles exactly 1000', () => {
    const result = formatTokenCount(1000);
    expect(result).toContain('1');
    expect(result).toContain('000');
  });

  it('is a pure function (same input → same output)', () => {
    expect(formatTokenCount(42)).toBe(formatTokenCount(42));
  });
});

describe('usageRatio', () => {
  it('returns 0 when size is 0', () => {
    expect(usageRatio(100, 0)).toBe(0);
  });

  it('returns 0 when size is negative', () => {
    expect(usageRatio(100, -1)).toBe(0);
  });

  it('returns 0 when used is 0', () => {
    expect(usageRatio(0, 200000)).toBe(0);
  });

  it('computes ratio for partial usage', () => {
    const ratio = usageRatio(50000, 200000);
    expect(ratio).toBe(0.25);
  });

  it('returns 1 when used equals size', () => {
    expect(usageRatio(200000, 200000)).toBe(1);
  });

  it('clamps to 1 when used exceeds size', () => {
    expect(usageRatio(250000, 200000)).toBe(1);
  });

  it('handles small decimal ratios', () => {
    const ratio = usageRatio(1, 200000);
    expect(ratio).toBeCloseTo(0.000005, 6);
  });
});

describe('ratioPercent', () => {
  it('formats 0%', () => {
    expect(ratioPercent(0)).toBe('0.0%');
  });

  it('formats 50%', () => {
    expect(ratioPercent(0.5)).toBe('50.0%');
  });

  it('formats 100%', () => {
    expect(ratioPercent(1)).toBe('100.0%');
  });

  it('formats sub-percent values to one decimal', () => {
    expect(ratioPercent(0.001)).toBe('0.1%');
  });

  it('rounds correctly', () => {
    // toFixed(1) rounds to nearest
    expect(ratioPercent(0.666)).toBe('66.6%');
    expect(ratioPercent(0.667)).toBe('66.7%');
  });

  it('formats >100% (clamped ratio)', () => {
    expect(ratioPercent(1.25)).toBe('125.0%');
  });
});
