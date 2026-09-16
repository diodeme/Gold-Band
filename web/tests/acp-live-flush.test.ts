import { describe, expect, it } from 'vitest';
import {
  AcpLatestWinsEventBuffer,
  decideAcpLiveEventFlush,
  isCoalescableAcpLiveEvent,
  mergeAcpLiveStreamEvent,
  mergeAcpLiveToolEvent,
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

  it('keeps non-coalescable lifecycle events immediate and flushes cached streaming text while paused', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: false,
      paused: true,
      hasScheduledFlush: true,
    })).toEqual({
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: true,
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

  it('keeps lifecycle events immediate and flushes cached streaming text during transient interaction', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: false,
      paused: false,
      hasScheduledFlush: true,
      deferRemainingMs: 120,
    })).toEqual({
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: true,
      scheduleFlush: false,
      scheduleDelayMs: null,
    });
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

  it('keeps newer text stream content when an older shorter frame arrives later', () => {
    const merged = mergeAcpLiveStreamEvent(
      {
        id: 'assistant-message-1',
        kind: 'textDelta',
        seq: 10,
        timestamp: '10Z',
        content: '我先建立验收清单并读取当前节点可见的报告文件。',
        endedSeq: 10,
      },
      {
        id: 'assistant-message-1',
        kind: 'textDelta',
        seq: 9,
        timestamp: '9Z',
        content: '我先建立验收清单',
        endedSeq: 9,
      },
    );

    expect(merged.seq).toBe(10);
    expect(merged.content).toBe('我先建立验收清单并读取当前节点可见的报告文件。');
  });

  it('does not let empty thought stream frames clear accumulated content', () => {
    const merged = mergeAcpLiveStreamEvent(
      {
        id: 'assistant-thought-1',
        kind: 'thoughtDelta',
        seq: 10,
        timestamp: '10Z',
        content: 'carefully and avoid vague references.',
        endedSeq: 10,
      },
      {
        id: 'assistant-thought-1',
        kind: 'thoughtDelta',
        seq: 11,
        timestamp: '11Z',
        content: '',
        endedSeq: 11,
      },
    );

    expect(merged.seq).toBe(11);
    expect(merged.content).toBe('carefully and avoid vague references.');
  });

  it('bounds interaction deferral by the pending batch starvation deadline', () => {
    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: false,
      hasScheduledFlush: false,
      flushDelayMs: 125,
      deferRemainingMs: 180,
      maxDeferRemainingMs: 40,
    }).scheduleDelayMs).toBe(40);

    expect(decideAcpLiveEventFlush({
      coalescable: true,
      paused: false,
      hasScheduledFlush: false,
      flushDelayMs: 0,
      deferRemainingMs: 180,
      maxDeferRemainingMs: 40,
    }).scheduleDelayMs).toBe(40);
  });

  it('evicts the oldest distinct identity when the latest-wins buffer reaches capacity', () => {
    const pending = new AcpLatestWinsEventBuffer<string>(3);

    for (let index = 0; index < 5; index += 1) {
      pending.replace(`stream-${index}`, `value-${index}`);
    }

    expect(pending.size).toBe(3);
    expect(pending.drain()).toEqual(['value-2', 'value-3', 'value-4']);
  });

  it('replays the 6021-frame incident with a bounded latest-wins single flight', () => {
    type ReplayEvent = {
      id: string;
      kind: string;
      content?: string;
      toolCallId?: string;
      status?: string;
      seq: number;
    };
    const pending = new AcpLatestWinsEventBuffer<ReplayEvent>(64);
    const finalText = new Map<string, string>();
    const publishedText = new Map<string, string>();
    let rawFrameCount = 0;
    let scheduled = false;
    let scheduledFlights = 0;
    let inFlight = 0;
    let maxInFlight = 0;
    let maxPending = 0;

    const flush = () => {
      if (!scheduled && pending.size === 0) return;
      scheduled = false;
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      for (const event of pending.drain()) {
        if (event.kind === 'thoughtDelta' || event.kind === 'textDelta') {
          publishedText.set(`${event.kind}:${event.id}`, event.content ?? '');
        }
      }
      inFlight -= 1;
    };
    const enqueue = (event: ReplayEvent) => {
      rawFrameCount += 1;
      const decision = decideAcpLiveEventFlush({
        coalescable: isCoalescableAcpLiveEvent(event),
        paused: false,
        flushDelayMs: 125,
        hasScheduledFlush: scheduled,
      });
      if (decision.flushPendingBeforeApply) flush();
      if (!decision.buffer) return;
      const key = event.toolCallId ? `tool:${event.toolCallId}` : `stream:${event.kind}:${event.id}`;
      const previous = pending.get(key);
      const merged = event.toolCallId
        ? mergeAcpLiveToolEvent(previous, event)
        : mergeAcpLiveStreamEvent(previous, event);
      pending.replace(key, merged);
      maxPending = Math.max(maxPending, pending.size);
      if (decision.scheduleFlush) {
        scheduled = true;
        scheduledFlights += 1;
      }
    };
    const replayTextFrames = (kind: 'thoughtDelta' | 'textDelta', count: number, idOffset: number) => {
      for (let seq = 0; seq < count; seq += 1) {
        const id = `message-${idOffset + (seq % 12)}`;
        const chunk = `${kind === 'thoughtDelta' ? '想' : '答'}${seq.toString(36)}`.slice(0, 16);
        const cumulative = `${finalText.get(`${kind}:${id}`) ?? ''}${chunk}`;
        finalText.set(`${kind}:${id}`, cumulative);
        enqueue({ id, kind, content: cumulative, seq });
      }
    };

    replayTextFrames('thoughtDelta', 5209, 0);
    replayTextFrames('textDelta', 534, 12);
    for (let seq = 0; seq < 35; seq += 1) {
      enqueue({ id: `tool-${seq}`, kind: 'toolCall', toolCallId: `tool-${seq}`, status: 'running', seq });
    }
    for (let seq = 0; seq < 145; seq += 1) {
      const terminal = seq >= 110;
      enqueue({
        id: `tool-update-${seq}`,
        kind: 'toolCallUpdate',
        toolCallId: `tool-${seq % 35}`,
        status: terminal ? 'completed' : 'running',
        seq,
      });
    }
    for (let seq = 0; seq < 58; seq += 1) enqueue({ id: `usage-${seq}`, kind: 'usageUpdate', seq });
    enqueue({ id: 'session-info', kind: 'sessionInfoUpdate', seq: 0 });
    enqueue({ id: 'commands', kind: 'availableCommandsUpdate', seq: 0 });
    rawFrameCount += 38; // Responses and non-session/update protocol frames from the capture.
    flush();

    expect(rawFrameCount).toBe(6021);
    expect(finalText.size).toBe(24);
    expect(Math.max(...[...finalText.values()].map((value) => value.length))).toBeGreaterThan(0);
    expect(publishedText).toEqual(finalText);
    expect(maxPending).toBeLessThanOrEqual(59); // 24 text streams + 35 tool identities.
    expect(maxPending).toBeLessThan(rawFrameCount / 100);
    expect(maxInFlight).toBe(1);
    expect(scheduledFlights).toBeGreaterThan(0);
    expect(pending.size).toBe(0);
    expect(scheduled).toBe(false);
  });
});
