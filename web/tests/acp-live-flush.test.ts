import { describe, expect, it } from 'vitest';
import {
  decideAcpLiveEventFlush,
  isCoalescableAcpLiveEvent,
  mergeAcpLiveToolEvent,
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

  it('coalesces non-terminal tool calls while keeping terminal tool updates immediate', () => {
    expect(isCoalescableAcpLiveEvent({
      kind: 'toolCall',
      toolCallId: 'call-1',
      status: 'running',
    })).toBe(true);
    expect(isCoalescableAcpLiveEvent({
      kind: 'toolCallUpdate',
      toolCallId: 'call-1',
      status: 'in_progress',
    })).toBe(true);
    expect(isCoalescableAcpLiveEvent({
      kind: 'toolCallUpdate',
      toolCallId: 'call-1',
      status: 'completed',
    })).toBe(false);
    expect(isCoalescableAcpLiveEvent({
      kind: 'toolCall',
      status: 'running',
    })).toBe(false);
  });

  it('preserves tool call display fields when pending updates collapse to the latest frame', () => {
    const merged = mergeAcpLiveToolEvent(
      {
        id: 'tool-start',
        kind: 'toolCall',
        toolCallId: 'call-1',
        seq: 10,
        timestamp: '10Z',
        title: 'Read file',
        content: 'D:/project/file.ts',
        status: 'running',
        startedSeq: 10,
        startedAt: '10Z',
        raw: { toolCall: { rawInput: { file_path: 'D:/project/file.ts' } } },
      },
      {
        id: 'tool-update',
        kind: 'toolCallUpdate',
        toolCallId: 'call-1',
        seq: 11,
        timestamp: '11Z',
        title: null,
        content: null,
        status: 'running',
        endedSeq: 11,
        endedAt: '11Z',
        raw: { output: 'ok' },
      },
      (previous, next) => ({ ...(previous as object), ...(next as object) }),
    );

    expect(merged.id).toBe('tool-update');
    expect(merged.title).toBe('Read file');
    expect(merged.content).toBe('D:/project/file.ts');
    expect(merged.startedSeq).toBe(10);
    expect(merged.endedSeq).toBe(11);
    expect(merged.raw).toEqual({
      toolCall: { rawInput: { file_path: 'D:/project/file.ts' } },
      output: 'ok',
    });
  });
});
