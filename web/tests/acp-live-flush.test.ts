import { describe, expect, it } from 'vitest';
import {
  decideAcpLiveEventFlush,
  shouldAutoScrollAfterAcpTimelineUpdate,
} from '@/lib/acp-live-flush';

describe('ACP live event flush policy', () => {
  it('buffers coalescable streaming events while live updates are paused', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: true,
      hasScheduledFlush: true,
    })).toEqual({
      buffer: true,
      applyImmediately: false,
      flushPendingBeforeApply: false,
      scheduleFlush: false,
      scheduleDelayMs: null,
    });
  });

  it('schedules exactly one flush for coalescable events while unpaused', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: false,
      hasScheduledFlush: false,
      flushDelayMs: 125,
    })).toEqual({
      buffer: true,
      applyImmediately: false,
      flushPendingBeforeApply: false,
      scheduleFlush: true,
      scheduleDelayMs: 125,
    });

    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: false,
      hasScheduledFlush: true,
    }).scheduleFlush).toBe(false);
  });

  it('keeps non-coalescable lifecycle events immediate without flushing cached streaming text while paused', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: false,
      paused: true,
      hasScheduledFlush: true,
    })).toEqual({
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: false,
      scheduleFlush: false,
      scheduleDelayMs: null,
    });
  });

  it('defers coalescable streaming flushes until the interaction quiet window ends', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: false,
      hasScheduledFlush: false,
      flushDelayMs: 125,
      deferRemainingMs: 180,
    })).toEqual({
      buffer: true,
      applyImmediately: false,
      flushPendingBeforeApply: false,
      scheduleFlush: true,
      scheduleDelayMs: 180,
    });
  });

  it('keeps lifecycle events immediate without flushing cached streaming text during transient interaction', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: false,
      paused: false,
      hasScheduledFlush: true,
      deferRemainingMs: 120,
    })).toEqual({
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: false,
      scheduleFlush: false,
      scheduleDelayMs: null,
    });
  });

  it('does not auto-scroll timeline updates during the interaction quiet window', () => {
    expect(shouldAutoScrollAfterAcpTimelineUpdate({
      pinned: true,
      deferRemainingMs: 100,
    })).toBe(false);
    expect(shouldAutoScrollAfterAcpTimelineUpdate({
      pinned: true,
      deferRemainingMs: 0,
    })).toBe(true);
    expect(shouldAutoScrollAfterAcpTimelineUpdate({
      pinned: false,
      deferRemainingMs: 0,
    })).toBe(false);
  });
});
