import { describe, expect, it, vi } from 'vitest';

import { registerHeartbeatActivityListeners } from '@/lib/heartbeat-activity';

describe('heartbeat activity lifecycle', () => {
  it('reports pointer, keyboard, and focus activity through one debounced boundary', () => {
    const target = new EventTarget();
    const reportActivity = vi.fn();
    let currentTime = 0;
    const dispose = registerHeartbeatActivityListeners(
      reportActivity,
      target,
      () => currentTime,
    );

    target.dispatchEvent(new Event('pointerdown'));
    currentTime = 59_999;
    target.dispatchEvent(new Event('focus'));
    currentTime = 60_000;
    target.dispatchEvent(new Event('focus'));
    currentTime = 120_000;
    target.dispatchEvent(new Event('keydown'));

    expect(reportActivity).toHaveBeenCalledTimes(3);

    dispose();
    currentTime = 180_000;
    target.dispatchEvent(new Event('pointerdown'));
    target.dispatchEvent(new Event('keydown'));
    target.dispatchEvent(new Event('focus'));
    expect(reportActivity).toHaveBeenCalledTimes(3);
  });
});
