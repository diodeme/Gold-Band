import { describe, expect, it } from 'vitest';
import {
  MAX_IMAGE_SCALE,
  MIN_IMAGE_SCALE,
  normalizedWheelScale,
  fitImageScale,
} from '@/lib/image-zoom-gesture';

describe('normalized wheel image zoom', () => {
  it('fits intrinsic image dimensions to the viewport without enlarging small images', () => {
    expect(fitImageScale({ width: 1365, height: 900 }, { width: 600, height: 800 })).toBeCloseTo(560 / 1365);
    expect(fitImageScale({ width: 100, height: 100 }, { width: 600, height: 800 })).toBe(1);
    expect(fitImageScale({ width: 1365, height: 900 }, { width: 390, height: 600 })).toBeCloseTo(350 / 1365);
  });
  it('bounds one large Windows wheel delta instead of jumping to a zoom limit', () => {
    expect(normalizedWheelScale(1, -10_000, 0, 800)).toBeCloseTo(1.433, 2);
    expect(normalizedWheelScale(1, 10_000, 0, 800)).toBeCloseTo(0.698, 2);
  });

  it('normalizes line and page deltas and remains monotonic', () => {
    const zoomedIn = normalizedWheelScale(1, -1, 1, 800);
    const zoomedOut = normalizedWheelScale(1, 1, 1, 800);
    expect(zoomedIn).toBeGreaterThan(1);
    expect(zoomedOut).toBeLessThan(1);
    expect(normalizedWheelScale(1, -1, 2, 800)).toBeCloseTo(1.433, 2);
  });

  it('clamps the final scale without overshoot or oscillation', () => {
    expect(normalizedWheelScale(MAX_IMAGE_SCALE, -120, 0, 800)).toBe(MAX_IMAGE_SCALE);
    expect(normalizedWheelScale(MIN_IMAGE_SCALE, 120, 0, 800)).toBe(MIN_IMAGE_SCALE);
  });

  it('keeps repeated same-direction input monotonic near both limits', () => {
    const zoomedIn = Array.from({ length: 20 }).reduce<number>(
      (scale) => normalizedWheelScale(scale, -100, 0, 800),
      7.5,
    );
    const zoomedOut = Array.from({ length: 20 }).reduce<number>(
      (scale) => normalizedWheelScale(scale, 100, 0, 800),
      0.12,
    );

    expect(zoomedIn).toBe(MAX_IMAGE_SCALE);
    expect(zoomedOut).toBe(MIN_IMAGE_SCALE);
    expect(normalizedWheelScale(zoomedIn, -100, 0, 800)).toBe(MAX_IMAGE_SCALE);
    expect(normalizedWheelScale(zoomedOut, 100, 0, 800)).toBe(MIN_IMAGE_SCALE);
  });
});
