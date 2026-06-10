import { describe, expect, it } from 'vitest';
import { buildAcpTimeline, isTopLevelPlanEvent, limitAcpEvents, mergeAcpEvents, queryBlocksFromTool, restoreAcpLoadedEvents, storeAcpLoadedEvents, timelineEventKey } from '../../src/components/acp/ACPChatDialog';
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

  it('excludes top-level plan events from timeline', () => {
    const timeline = buildAcpTimeline([
      event({ id: 'tool-1', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-1', title: 'Read file' }),
      event({ id: 'plan-1', seq: 2, timestamp: '2Z', kind: 'plan', raw: { entries: [{ content: 'task 1', status: 'pending' }] } }),
      event({ id: 'msg-1', seq: 3, timestamp: '3Z', kind: 'textDelta', content: 'working...' }),
      event({ id: 'plan-2', seq: 4, timestamp: '4Z', kind: 'plan', raw: { entries: [{ content: 'task 1', status: 'completed' }] } }),
    ]);

    const keys = timeline.map(timelineEventKey);
    expect(keys).not.toContain('plan-plan-1');
    expect(keys).not.toContain('plan-plan-2');
    expect(keys).toContain('tool-call-1');
  });

  it('keeps child-agent plan events in timeline', () => {
    const childPlanRaw = {
      entries: [{ content: 'sub task', status: 'in_progress' }],
      _meta: { claudeCode: { parentToolUseId: 'call-agent-1' } },
    };

    const timeline = buildAcpTimeline([
      event({ id: 'agent-start', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-agent-1', title: 'agent', raw: { toolCall: { rawInput: { description: 'sub task', prompt: 'do it', subagent_type: 'claude' } } } }),
      event({ id: 'child-plan', seq: 2, timestamp: '2Z', kind: 'plan', raw: childPlanRaw }),
      event({ id: 'agent-end', seq: 3, timestamp: '3Z', kind: 'toolCallUpdate', toolCallId: 'call-agent-1', status: 'completed', title: 'agent' }),
    ]);

    // The child plan should be inside the child agent group, not dropped
    const rootKeys = timeline.map(timelineEventKey);
    // Root level should contain child-agent-group, not the plan directly
    expect(rootKeys).not.toContain('plan-child-plan');
    expect(rootKeys.some(k => k.startsWith('child-agent-'))).toBe(true);
  });

  it('isTopLevelPlanEvent returns false for child-agent plans', () => {
    const childPlan = event({
      id: 'p1', seq: 1, timestamp: '1Z', kind: 'plan',
      raw: { _meta: { claudeCode: { parentToolUseId: 'call-1' } } },
    });
    expect(isTopLevelPlanEvent(childPlan)).toBe(false);

    const topPlan = event({
      id: 'p2', seq: 2, timestamp: '2Z', kind: 'plan',
      raw: {},
    });
    expect(isTopLevelPlanEvent(topPlan)).toBe(true);
  });

  it('preserves multiple params with same labelKey but different values', () => {
    const blocks = queryBlocksFromTool('Grep `pattern` in `src/`', {
      file_path: '/project/src/main.ts',
      pattern: 'TODO',
      glob: '*.ts',
    });

    const pathBlocks = blocks.filter(b => b.labelKey === 'acp.toolPath');
    const queryBlocks = blocks.filter(b => b.labelKey === 'acp.toolQuery');

    // Should keep distinct values even with same labelKey
    expect(pathBlocks.length).toBeGreaterThanOrEqual(1);
    expect(queryBlocks.length).toBeGreaterThanOrEqual(2);
    expect(blocks.some(b => b.value === 'TODO')).toBe(true);
    expect(blocks.some(b => b.value === '*.ts')).toBe(true);
  });
});

describe('ACPChatDialog event cache', () => {
  function makeEvent(id: string, content: string): AcpUiEventVm {
    return event({ id, seq: 1, timestamp: '1Z', kind: 'textDelta', content });
  }

  it('mergeAcpEvents deduplicates by key preferring next', () => {
    const prev = [makeEvent('e1', 'old')];
    const next = [makeEvent('e1', 'new')];
    const merged = mergeAcpEvents(prev, next);
    expect(merged).toHaveLength(1);
    expect(merged[0]!.content).toBe('new');
  });

  it('limitAcpEvents trims from start when exceeding page size', () => {
    const events = Array.from({ length: 100 }, (_, i) => makeEvent(`e${i}`, `msg ${i}`));
    const limited = limitAcpEvents(events, 'start', 30);
    expect(limited).toHaveLength(30);
    expect(limited[0]!.content).toBe('msg 70');
    expect(limited[29]!.content).toBe('msg 99');
  });

  it('limitAcpEvents returns all events when under limit', () => {
    const events = [makeEvent('e1', 'a'), makeEvent('e2', 'b')];
    const limited = limitAcpEvents(events, 'start', 360);
    expect(limited).toHaveLength(2);
  });

  it('storeAcpLoadedEvents persists and restoreAcpLoadedEvents retrieves', () => {
    const key = 'test-session-1';
    const events = [makeEvent('e10', 'hello'), makeEvent('e20', 'world')];
    storeAcpLoadedEvents(key, events, 360);
    const restored = restoreAcpLoadedEvents(key, [], 360);
    expect(restored).toHaveLength(2);
    expect(restored[0]!.content).toBe('hello');
  });

  it('storeAcpLoadedEvents trims to page size', () => {
    const key = 'test-session-2';
    const events = Array.from({ length: 200 }, (_, i) => makeEvent(`e${i}`, `m${i}`));
    storeAcpLoadedEvents(key, events, 30);
    const restored = restoreAcpLoadedEvents(key, [], 30);
    expect(restored).toHaveLength(30);
    // Should keep only the last 30
    expect(restored[0]!.id).toBe('e170');
  });
});
