import { describe, expect, it } from 'vitest';
import {
  MAX_IMAGE_SCALE,
  MIN_IMAGE_SCALE,
  normalizedCtrlWheelScale,
} from '@/lib/image-zoom-gesture';

describe('normalized Ctrl-wheel image zoom', () => {
  it('bounds one large Windows wheel delta instead of jumping to a zoom limit', () => {
    expect(normalizedCtrlWheelScale(1, -10_000, 0, 800)).toBeCloseTo(1.433, 2);
    expect(normalizedCtrlWheelScale(1, 10_000, 0, 800)).toBeCloseTo(0.698, 2);
  });

  it('normalizes line and page deltas and remains monotonic', () => {
    const zoomedIn = normalizedCtrlWheelScale(1, -1, 1, 800);
    const zoomedOut = normalizedCtrlWheelScale(1, 1, 1, 800);
    expect(zoomedIn).toBeGreaterThan(1);
    expect(zoomedOut).toBeLessThan(1);
    expect(normalizedCtrlWheelScale(1, -1, 2, 800)).toBeCloseTo(1.433, 2);
  });

  it('clamps the final scale without overshoot or oscillation', () => {
    expect(normalizedCtrlWheelScale(MAX_IMAGE_SCALE, -120, 0, 800)).toBe(MAX_IMAGE_SCALE);
    expect(normalizedCtrlWheelScale(MIN_IMAGE_SCALE, 120, 0, 800)).toBe(MIN_IMAGE_SCALE);
  });

  it('keeps repeated same-direction input monotonic near both limits', () => {
    const zoomedIn = Array.from({ length: 20 }).reduce<number>(
      (scale) => normalizedCtrlWheelScale(scale, -100, 0, 800),
      7.5,
    );
    const zoomedOut = Array.from({ length: 20 }).reduce<number>(
      (scale) => normalizedCtrlWheelScale(scale, 100, 0, 800),
      0.12,
    );

    expect(zoomedIn).toBe(MAX_IMAGE_SCALE);
    expect(zoomedOut).toBe(MIN_IMAGE_SCALE);
    expect(normalizedCtrlWheelScale(zoomedIn, -100, 0, 800)).toBe(MAX_IMAGE_SCALE);
    expect(normalizedCtrlWheelScale(zoomedOut, 100, 0, 800)).toBe(MIN_IMAGE_SCALE);
  });
});
