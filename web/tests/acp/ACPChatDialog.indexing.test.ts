import { describe, expect, it } from 'vitest';
import { buildAcpTimeline, buildAcpTimelineProjection, collectTimelineItemKeys, createAcpSessionCacheKey, isTopLevelPlanEvent, latestStreamingMarkdownItemKey, latestStreamingMarkdownItemKeyFromEvents, limitAcpEvents, mergeAcpEvents, pruneExpandedTimelineItems, queryBlocksFromTool, restoreAcpLoadedEvents, storeAcpLoadedEvents, timelineEventKey } from '../../src/components/acp/ACPChatDialog';
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
  it('marks only the newest active text or thought stream for paced Markdown', () => {
    const thought = event({
      id: 'thought-1',
      seq: 1,
      endedSeq: 4,
      timestamp: '1Z',
      kind: 'thoughtDelta',
      content: '**thinking**',
    });
    const text = event({
      id: 'message-1',
      seq: 2,
      endedSeq: 3,
      timestamp: '2Z',
      kind: 'textDelta',
      content: 'older response',
    });

    expect(latestStreamingMarkdownItemKey(buildAcpTimeline([thought, text]))).toBe(
      'thoughtDelta-thought-1',
    );
    expect(latestStreamingMarkdownItemKeyFromEvents([
      thought,
      text,
      event({
        id: 'optimistic-user',
        seq: Number.MAX_SAFE_INTEGER,
        timestamp: '3Z',
        kind: 'userTextDelta',
        content: 'prompt',
        raw: { optimistic: true },
      }),
    ])).toBe(
      'thoughtDelta-thought-1',
    );
    expect(latestStreamingMarkdownItemKeyFromEvents([
      thought,
      event({
        id: 'tool-1',
        seq: 5,
        timestamp: '5Z',
        kind: 'toolCall',
        toolCallId: 'tool-1',
      }),
    ])).toBeNull();
  });

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
      _meta: { agentTranscript: { parentToolCallId: 'call-agent-1' } },
    };

    const timeline = buildAcpTimeline([
      event({ id: 'agent-start', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-agent-1', title: 'agent', raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'sub task', prompt: 'do it', subagent_type: 'claude' } } }),
      event({ id: 'child-plan', seq: 2, timestamp: '2Z', kind: 'plan', raw: childPlanRaw }),
      event({ id: 'agent-end', seq: 3, timestamp: '3Z', kind: 'toolCallUpdate', toolCallId: 'call-agent-1', status: 'completed', title: 'agent' }),
    ]);

    // The child plan should be inside the child agent group, not dropped
    const rootKeys = timeline.map(timelineEventKey);
    // Root level should contain child-agent-group, not the plan directly
    expect(rootKeys).not.toContain('plan-child-plan');
    expect(rootKeys.some(k => k.startsWith('child-agent-'))).toBe(true);
  });

  it('keeps nested child-agent expansion keys while streaming updates rebuild the timeline', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'outer-agent',
        seq: 1,
        timestamp: '1Z',
        kind: 'toolCall',
        toolCallId: 'call-outer',
        status: 'running',
        raw: {
          _meta: { agentTranscript: { agentLaunch: true } },
          rawInput: { description: 'outer task', subagent_type: 'general-purpose' },
        },
      }),
      event({
        id: 'inner-agent',
        seq: 2,
        timestamp: '2Z',
        kind: 'toolCall',
        toolCallId: 'call-inner',
        status: 'pending',
        raw: {
          _meta: {
            agentTranscript: {
              agentLaunch: true,
              parentToolCallId: 'call-outer',
            },
          },
          rawInput: { description: 'inner task', subagent_type: 'general-purpose' },
        },
      }),
    ]);

    const outerKey = 'child-agent-call-outer-1';
    const innerKey = 'child-agent-call-inner-2';
    expect([...collectTimelineItemKeys(timeline)]).toEqual([
      outerKey,
      innerKey,
    ]);

    const expanded = {
      [outerKey]: true,
      [innerKey]: true,
      'child-agent-removed': true,
    };
    expect(pruneExpandedTimelineItems(expanded, timeline)).toEqual({
      [outerKey]: true,
      [innerKey]: true,
    });
  });

  it('recognizes a compacted agent lifecycle from its terminal tool update metadata', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'agent-completed', seq: 1, timestamp: '1Z', kind: 'toolCallUpdate',
        toolCallId: 'call-agent', status: 'completed',
        raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'review' } },
      }),
      event({
        id: 'agent-text', seq: 2, timestamp: '2Z', kind: 'textDelta', content: 'review complete',
        raw: { _meta: { agentTranscript: { parentToolCallId: 'call-agent' } } },
      }),
    ]);

    expect(timeline).toHaveLength(1);
    expect(timeline[0]?.kind).toBe('childAgentGroup');
    if (timeline[0]?.kind !== 'childAgentGroup') throw new Error('missing child agent group');
    expect(timeline[0].status).toBe('completed');
    expect(timeline[0].events.map(timelineEventKey)).toEqual(['textDelta-agent-text']);
  });

  it('keeps grandchild tool events inside their nested agent branch', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'outer-agent', seq: 1, timestamp: '1Z', kind: 'toolCall',
        toolCallId: 'call-outer', status: 'running',
        raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'outer' } },
      }),
      event({
        id: 'inner-agent', seq: 2, timestamp: '2Z', kind: 'toolCall',
        toolCallId: 'call-inner', status: 'pending',
        raw: {
          _meta: { agentTranscript: { agentLaunch: true, parentToolCallId: 'call-outer' } },
          rawInput: { description: 'inner' },
        },
      }),
      event({
        id: 'powershell', seq: 3, timestamp: '3Z', kind: 'toolCall',
        toolCallId: 'call-powershell', status: 'pending', title: 'PowerShell',
        raw: {
          _meta: { agentTranscript: { parentToolCallId: 'call-inner' } },
          rawInput: { command: 'Get-ChildItem -Force' },
        },
      }),
    ]);

    const outer = timeline[0];
    expect(outer?.kind).toBe('childAgentGroup');
    if (!outer || outer.kind !== 'childAgentGroup') throw new Error('missing outer agent');
    const inner = outer.events[0];
    expect(inner?.kind).toBe('childAgentGroup');
    if (!inner || inner.kind !== 'childAgentGroup') throw new Error('missing inner agent');
    expect(inner.status).toBe('running');
    expect(inner.events.map(timelineEventKey)).toEqual(['tool-call-powershell']);
  });

  it('projects an unscoped descendant plan into its established child-agent branch', () => {
    const outerAgent = event({
      id: 'outer-agent', seq: 1, timestamp: '1Z', kind: 'toolCall',
      toolCallId: 'call-outer', status: 'running',
      raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'outer' } },
    });
    const firstPlan = event({
      id: 'plan-1', seq: 2, timestamp: '2Z', kind: 'plan',
      raw: {
        entries: [{ content: 'Browse repository', status: 'in_progress' }],
        _meta: { agentTranscript: { parentToolCallId: 'call-outer' } },
      },
    });
    const aggregatePlan = event({
      id: 'plan-2', seq: 3, timestamp: '3Z', kind: 'plan',
      raw: {
        entries: [
          { content: 'Browse repository', status: 'completed' },
          { content: 'Map data flow', status: 'in_progress' },
        ],
      },
    });

    const projection = buildAcpTimelineProjection([
      outerAgent,
      firstPlan,
      aggregatePlan,
    ]);
    expect(projection.todoEntries).toEqual([]);
    const outer = projection.timeline[0];
    expect(outer?.kind).toBe('childAgentGroup');
    if (!outer || outer.kind !== 'childAgentGroup') throw new Error('missing outer agent');
    expect(outer.todoEntries).toEqual([
      { content: 'Browse repository', status: 'completed', priority: undefined },
      { content: 'Map data flow', status: 'in_progress', priority: undefined },
    ]);
    expect(outer.events).toEqual([]);
    expect(isTopLevelPlanEvent(aggregatePlan, [outerAgent, firstPlan, aggregatePlan])).toBe(false);
  });

  it('does not inherit a completed child-agent plan lineage after its lifecycle ends', () => {
    const events = [
      event({
        id: 'outer-agent', seq: 1, startedSeq: 1, endedSeq: 3,
        timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-outer', status: 'completed',
        raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'outer' } },
      }),
      event({
        id: 'child-plan', seq: 2, timestamp: '2Z', kind: 'plan',
        raw: {
          entries: [{ content: 'Shared task', status: 'in_progress' }],
          _meta: { agentTranscript: { parentToolCallId: 'call-outer' } },
        },
      }),
      event({
        id: 'main-plan', seq: 4, timestamp: '4Z', kind: 'plan',
        raw: { entries: [{ content: 'Shared task', status: 'pending' }] },
      }),
    ];

    const projection = buildAcpTimelineProjection(events);
    expect(projection.todoEntries).toEqual([
      { content: 'Shared task', status: 'pending', priority: undefined },
    ]);
    expect(isTopLevelPlanEvent(events[2]!, events)).toBe(true);
  });

  it('keeps nested agent todos inside their owners and removes them from the aggregate root plan', () => {
    const events = [
      event({
        id: 'child-a', seq: 1, timestamp: '1Z', kind: 'toolCall',
        toolCallId: 'call-a', status: 'running',
        raw: { rawInput: { description: 'A' }, _meta: { agentTranscript: { agentLaunch: true } } },
      }),
      event({
        id: 'child-b', seq: 2, timestamp: '2Z', kind: 'toolCall',
        toolCallId: 'call-b', status: 'running',
        raw: { rawInput: { description: 'B' }, _meta: { agentTranscript: { agentLaunch: true } } },
      }),
      event({
        id: 'aggregate', seq: 3, timestamp: '3Z', kind: 'plan',
        raw: { entries: [
          { content: 'A todo', status: 'in_progress' },
          { content: 'B todo', status: 'pending' },
          { content: 'Main todo', status: 'pending' },
        ] },
      }),
      event({
        id: 'plan-a', seq: 4, timestamp: '4Z', kind: 'plan',
        raw: { entries: [{ content: 'A todo', status: 'in_progress' }], _meta: { agentTranscript: { parentToolCallId: 'call-a' } } },
      }),
      event({
        id: 'plan-b', seq: 5, timestamp: '5Z', kind: 'plan',
        raw: { entries: [{ content: 'B todo', status: 'pending' }], _meta: { agentTranscript: { parentToolCallId: 'call-b' } } },
      }),
    ];

    const projection = buildAcpTimelineProjection(events);
    expect(projection.todoEntries).toEqual([
      { content: 'Main todo', status: 'pending', priority: undefined },
    ]);
    const groups = projection.timeline.filter(item => item.kind === 'childAgentGroup');
    expect(groups).toHaveLength(2);
    if (groups[0]?.kind !== 'childAgentGroup' || groups[1]?.kind !== 'childAgentGroup') {
      throw new Error('missing child groups');
    }
    expect(groups[0].todoEntries.map(entry => entry.content)).toEqual(['A todo']);
    expect(groups[1].todoEntries.map(entry => entry.content)).toEqual(['B todo']);
  });

  it('isTopLevelPlanEvent returns false for child-agent plans', () => {
    const childPlan = event({
      id: 'p1', seq: 1, timestamp: '1Z', kind: 'plan',
      raw: { _meta: { agentTranscript: { parentToolCallId: 'call-1' } } },
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

  it('mergeAcpEvents keeps stream display seq stable when newer content arrives', () => {
    const prev = [
      event({
        id: 'message-1',
        seq: 20,
        timestamp: '20Z',
        kind: 'textDelta',
        content: 'hello',
        endedSeq: 20,
      }),
    ];
    const next = [
      event({
        id: 'message-1',
        seq: 25,
        timestamp: '25Z',
        kind: 'textDelta',
        content: 'hello world',
        endedSeq: 25,
      }),
    ];

    const merged = mergeAcpEvents(prev, next);
    expect(merged).toHaveLength(1);
    expect(merged[0]!.seq).toBe(20);
    expect(merged[0]!.endedSeq).toBe(25);
    expect(merged[0]!.content).toBe('hello world');
    expect(timelineEventKey(buildAcpTimeline(merged)[0]!)).toBe('textDelta-message-1');
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

  it('separates reused task and run ids by cache namespace', () => {
    const oldKey = createAcpSessionCacheKey(
      'task-uuid-old',
      'task-021',
      'run-001',
      'round-001',
      'bootstrap',
      'attempt-001',
    );
    const newKey = createAcpSessionCacheKey(
      'task-uuid-new',
      'task-021',
      'run-001',
      'round-001',
      'bootstrap',
      'attempt-001',
    );
    storeAcpLoadedEvents(oldKey, [makeEvent('old-event', 'deleted task content')], 360);

    expect(restoreAcpLoadedEvents(newKey, [], 360)).toEqual([]);
    expect(restoreAcpLoadedEvents(oldKey, [], 360)).toHaveLength(1);
  });
});
