import { describe, expect, it } from 'vitest';
import { calculateCenteredViewport } from '../src/components/workflowGraph';

describe('runtime graph fit viewport', () => {
  for (const width of [1400, 480, 1400]) {
    it(`fits the entire long graph into a ${width}px container`, () => {
      const bounds = { x: 40, y: 40, width: 6400, height: 600 };
      const viewport = calculateCenteredViewport(bounds, { width, height: 800 }, 0.22, 0.82, 0.4, 0.32);
      expect(bounds.x * viewport.zoom + viewport.x).toBeGreaterThanOrEqual(0);
      expect((bounds.x + bounds.width) * viewport.zoom + viewport.x).toBeLessThanOrEqual(width);
      expect(bounds.y * viewport.zoom + viewport.y).toBeGreaterThanOrEqual(0);
      expect((bounds.y + bounds.height) * viewport.zoom + viewport.y).toBeLessThanOrEqual(800);
    });
  }
  it('keeps short graphs within the readable maximum zoom', () => {
    expect(calculateCenteredViewport({x:0,y:0,width:260,height:138}, {width:1400,height:800},
      0.22, 0.82, 0.4, 0.32).zoom).toBe(0.82);
  });
});
