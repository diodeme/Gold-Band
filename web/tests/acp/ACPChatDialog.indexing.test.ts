import { describe, expect, it } from 'vitest';
import { buildAcpTimeline, timelineEventKey } from '../../src/components/acp/ACPChatDialog';
import type { AcpUiEventVm } from '../../src/types';

function event(partial: Partial<AcpUiEventVm> & Pick<AcpUiEventVm, 'id' | 'seq' | 'timestamp' | 'kind'>): AcpUiEventVm {
  return {
    id: partial.id,
    seq: partial.seq,
    timestamp: partial.timestamp,
    kind: partial.kind,
    sessionId: partial.sessionId ?? 's-1',
    content: partial.content ?? null,
    title: partial.title ?? null,
    toolCallId: partial.toolCallId ?? null,
    status: partial.status ?? null,
    startedSeq: partial.startedSeq ?? partial.seq,
    endedSeq: partial.endedSeq ?? partial.seq,
    startedAt: partial.startedAt ?? partial.timestamp,
    endedAt: partial.endedAt ?? partial.timestamp,
    raw: partial.raw,
  };
}

describe('ACPChatDialog timeline helpers', () => {
  it('keeps stable keys for timeline items', () => {
    const timeline = buildAcpTimeline([
      event({ id: 'tool-raw', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-1' }),
      event({ id: 'message-1', seq: 2, timestamp: '2Z', kind: 'textDelta', content: 'hello' }),
    ]);

    expect(timeline.map(timelineEventKey)).toEqual(['tool-call-1', 'textDelta-message-1']);
  });

  it('aggregates delta and tool updates into stable timeline items', () => {
    const timeline = buildAcpTimeline([
      event({ id: 'thought-1', seq: 1, timestamp: '1Z', kind: 'thoughtDelta', content: 'thinking' }),
      event({ id: 'thought-1', seq: 2, timestamp: '2Z', kind: 'thoughtDelta', content: 'thinking more' }),
      event({ id: 'tool-start', seq: 3, timestamp: '3Z', kind: 'toolCall', toolCallId: 'call-1', status: 'running', title: 'Read file' }),
      event({ id: 'tool-update', seq: 4, timestamp: '4Z', kind: 'toolCallUpdate', toolCallId: 'call-1', status: 'completed', title: 'Read file' }),
      event({ id: 'message-1', seq: 5, timestamp: '5Z', kind: 'textDelta', content: 'done' }),
    ]);

    expect(timeline.map(timelineEventKey)).toEqual([
      'thoughtDelta-thought-1',
      'tool-call-1',
      'textDelta-message-1',
    ]);
    const thought = timeline[0];
    const tool = timeline[1];
    expect(thought && !('events' in thought) ? thought.content : null).toBe('thinking more');
    expect(tool && !('events' in tool) ? tool.status : null).toBe('completed');
  });
});
