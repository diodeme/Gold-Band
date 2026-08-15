import { describe, expect, it } from 'vitest';
import {
  buildAcpTimeline,
  buildAcpTimelineProjection,
  createAcpSessionCacheKey,
  latestLiveSessionTimingFromEvents,
  limitAcpEvents,
  mergeAcpEvents,
  mergeOptimisticSession,
  nextLiveStreamingMarkdownTarget,
  objectiveActivityDescriptor,
  optimisticUserEvent,
  planAcpStopResponse,
  queryBlocksFromTool,
  restoreAcpLoadedEvents,
  shouldAwaitTerminalAcpStop,
  stabilizeTimelineItems,
  storeAcpLoadedEvents,
  timelineEventKey,
  timelineRenderKey,
} from '../../src/components/acp/ACPChatDialog';
import type { AcpSessionVm, AcpTimelineProjectionVm, AcpUiEventVm } from '../../src/types';

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
    timing: partial.timing,
    raw: partial.raw,
  };
}

function agentLaunch(id: string, seq: number, agentExecutionId: string) {
  return event({
    id,
    seq,
    timestamp: `${seq}Z`,
    kind: 'toolCall',
    toolCallId: `provider-${id}`,
    status: 'completed',
    title: 'Agent',
    raw: {
      rawInput: { description: id },
      _meta: {
        goldBandConversation: {
          branchId: 'root',
          launchedAgentExecutionId: agentExecutionId,
          toolName: 'Agent',
        },
      },
    },
  });
}

function projection(agents: AcpTimelineProjectionVm['agents'], todoEntries: AcpTimelineProjectionVm['todoEntries'] = []): AcpTimelineProjectionVm {
  return { agents, todoEntries };
}

function projectedAgent(agentExecutionId: string, overrides: Partial<AcpTimelineProjectionVm['agents'][number]> = {}) {
  return {
    agentExecutionId,
    parentAgentExecutionId: null,
    executionStatus: 'running',
    eventCount: 4,
    toolCallCount: 2,
    readFileCount: 1,
    writtenFileCount: 0,
    hasAttention: false,
    title: 'Agent',
    description: agentExecutionId,
    todoEntries: [],
    ...overrides,
  };
}

describe('ACPChatDialog branch timeline helpers', () => {
  it('paces only a live text update after the current prompt and settles before tools', () => {
    const historical = event({ id: 'old-answer', seq: 4, timestamp: '4Z', kind: 'textDelta', content: 'already rendered' });
    expect(nextLiveStreamingMarkdownTarget(null, historical, 10)).toBeNull();

    const liveText = event({ id: 'new-answer', seq: 11, timestamp: '11Z', kind: 'textDelta', content: 'new response' });
    const target = nextLiveStreamingMarkdownTarget(null, liveText, 10);
    expect(target).toEqual({ key: 'textDelta-new-answer', position: 11 });

    const tool = event({ id: 'tool-1', seq: 12, timestamp: '12Z', kind: 'toolCall', toolCallId: 'tool-1' });
    expect(nextLiveStreamingMarkdownTarget(target, tool, 10)).toBeNull();
  });

  it('uses the semantic activity start cursor as the stable activity key', () => {
    const timeline = buildAcpTimeline([
      event({ id: 'tool-raw', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'call-1' }),
      event({ id: 'message-1', seq: 2, timestamp: '2Z', kind: 'textDelta', content: 'hello' }),
    ]);

    expect(timeline.map(timelineEventKey)).toEqual(['activity-1', 'textDelta-message-1']);
  });

  it('aggregates thought and tool updates into one semantic activity block', () => {
    const timeline = buildAcpTimeline([
      event({ id: 'thought-1', seq: 1, timestamp: '1Z', kind: 'thoughtDelta', content: 'thinking' }),
      event({ id: 'thought-1', seq: 2, timestamp: '2Z', kind: 'thoughtDelta', content: 'thinking more' }),
      event({ id: 'tool-start', seq: 3, timestamp: '3Z', kind: 'toolCall', toolCallId: 'call-1', status: 'running', title: 'Read file' }),
      event({ id: 'tool-update', seq: 4, timestamp: '4Z', kind: 'toolCallUpdate', toolCallId: 'call-1', status: 'completed', title: 'Read file' }),
      event({ id: 'message-1', seq: 5, timestamp: '5Z', kind: 'textDelta', content: 'done' }),
    ]);

    expect(timeline.map(timelineEventKey)).toEqual(['activity-1', 'textDelta-message-1']);
    const activity = timeline[0];
    expect(activity?.kind).toBe('activityBatch');
    if (!activity || activity.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(activity.events.map(timelineEventKey)).toEqual(['thoughtDelta-thought-1', 'tool-call-1']);
    expect(activity.events[0]?.content).toBe('thinking more');
    expect(activity.events[1]?.status).toBe('completed');
  });

  it('keeps TODO in the queried branch and never infers ownership from text overlap', () => {
    const root = buildAcpTimelineProjection([
      event({ id: 'root-plan', seq: 1, timestamp: '1Z', kind: 'plan', raw: { entries: [{ content: 'same text', status: 'pending' }] } }),
    ], 'running', projection([], [{ content: 'root task', status: 'in_progress' }]));
    const child = buildAcpTimelineProjection([
      event({ id: 'child-plan', seq: 1, timestamp: '1Z', kind: 'plan', raw: { entries: [{ content: 'same text', status: 'completed' }] } }),
    ], 'running', projection([], [{ content: 'child task', status: 'completed' }]));

    expect(root.todoEntries).toEqual([{ content: 'root task', status: 'in_progress' }]);
    expect(child.todoEntries).toEqual([{ content: 'child task', status: 'completed' }]);
    expect(root.timeline).toEqual([]);
    expect(child.timeline).toEqual([]);
  });

  it('projects an Agent launch as a link and does not mount child transcript events in the parent', () => {
    const launch = agentLaunch('inspect ACP', 1, 'agent-01');
    const parent = buildAcpTimelineProjection(
      [launch],
      'running',
      projection([projectedAgent('agent-01', { toolCallCount: 24, readFileCount: 11 })]),
    );

    expect(parent.timeline).toHaveLength(1);
    expect(parent.timeline[0]).toMatchObject({
      kind: 'agentLink',
      agentExecutionId: 'agent-01',
      status: 'running',
      toolCallCount: 24,
      readFileCount: 11,
    });

    const child = buildAcpTimelineProjection([
      event({ id: 'agent-prompt', seq: 1, timestamp: '1Z', kind: 'userTextDelta', content: 'inspect ACP', raw: { source: 'agentBranchPrompt' } }),
      event({ id: 'read', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'read-1', title: 'Read', status: 'completed' }),
      event({ id: 'answer', seq: 3, timestamp: '3Z', kind: 'textDelta', content: 'done' }),
    ], 'running', projection([]));
    expect(child.timeline.map(timelineEventKey)).toEqual(['userTextDelta-agent-prompt', 'activity-2', 'textDelta-answer']);
    expect(parent.timeline.some((item) => timelineEventKey(item).includes('read-1'))).toBe(false);
  });

  it('uses the persisted Agent lifecycle instead of launch tool status', () => {
    const launch = agentLaunch('background review', 1, 'agent-01');
    const running = buildAcpTimelineProjection(
      [launch],
      'running',
      projection([projectedAgent('agent-01', { executionStatus: 'running' })]),
    ).timeline[0];
    expect(running?.kind === 'agentLink' ? running.status : null).toBe('running');

    const interrupted = buildAcpTimelineProjection(
      [launch],
      'cancelled',
      projection([projectedAgent('agent-01', { executionStatus: 'interrupted' })]),
    ).timeline[0];
    expect(interrupted?.kind === 'agentLink' ? interrupted.status : null).toBe('interrupted');
  });

  it('does not treat a completed launch receipt as Agent completion before projection arrives', () => {
    const launch = agentLaunch('background review', 1, 'agent-01');
    const pendingProjection = buildAcpTimelineProjection(
      [launch],
      'running',
      projection([]),
    ).timeline[0];
    expect(pendingProjection?.kind === 'agentLink' ? pendingProjection.status : null).toBe('queued');

    const completedSession = buildAcpTimelineProjection(
      [launch],
      'completed',
      projection([]),
    ).timeline[0];
    expect(completedSession?.kind === 'agentLink' ? completedSession.status : null).toBe('completed');
  });

  it('preserves an unrelated Agent link object when another Agent projection changes', () => {
    const launches = [agentLaunch('A', 1, 'agent-a'), agentLaunch('B', 2, 'agent-b')];
    const initial = buildAcpTimelineProjection(
      launches,
      'running',
      projection([projectedAgent('agent-a'), projectedAgent('agent-b')]),
    ).timeline;
    const updated = buildAcpTimelineProjection(
      launches,
      'running',
      projection([
        projectedAgent('agent-a'),
        projectedAgent('agent-b', { toolCallCount: 3, eventCount: 5 }),
      ]),
    ).timeline;
    const stable = stabilizeTimelineItems(updated, initial);

    expect(stable[0]).toBe(initial[0]);
    expect(stable[1]).not.toBe(initial[1]);
  });

  it('archives each activity when formal text begins and starts a new live activity', () => {
    const timeline = buildAcpTimelineProjection([
      event({ id: 'thought-1', seq: 1, timestamp: '1Z', kind: 'thoughtDelta', content: 'inspect' }),
      event({ id: 'read-1', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'read-1', status: 'completed', title: 'Read file' }),
      event({ id: 'text-1', seq: 3, timestamp: '3Z', kind: 'textDelta', content: 'Finding one.' }),
      event({ id: 'grep-1', seq: 4, timestamp: '4Z', kind: 'toolCall', toolCallId: 'grep-1', status: 'running', title: 'Grep' }),
    ], 'running').timeline;

    expect(timeline.map(timelineEventKey)).toEqual(['activity-1', 'textDelta-text-1', 'activity-4']);
    expect(timeline[0]?.kind === 'activityBatch' ? timeline[0].live : null).toBe(false);
    expect(timeline[2]?.kind === 'activityBatch' ? timeline[2].live : null).toBe(true);
  });

  it('scopes message render identity to the conversation event window', () => {
    const message = event({
      id: 'provider-reused-id',
      seq: 1,
      timestamp: '1Z',
      kind: 'textDelta',
      content: 'first session',
    });

    expect(timelineRenderKey('session-a:root', message)).not.toBe(
      timelineRenderKey('session-b:root', message),
    );
  });

  it('keeps an archived activity terminal when a stale active snapshot arrives after stop', () => {
    const events = [
      event({ id: 'thought-1', seq: 1, timestamp: '1Z', kind: 'thoughtDelta', content: 'inspect' }),
      event({ id: 'read-1', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'read-1', status: 'completed', title: 'Read file' }),
    ];
    const live = buildAcpTimelineProjection(events, 'running').timeline;
    const archived = stabilizeTimelineItems(
      buildAcpTimelineProjection(events, 'cancelled').timeline,
      live,
    );
    const staleActive = stabilizeTimelineItems(
      buildAcpTimelineProjection(events, 'running').timeline,
      archived,
    );

    expect(live[0]?.kind === 'activityBatch' ? live[0].live : null).toBe(true);
    expect(archived[0]?.kind === 'activityBatch' ? archived[0].live : null).toBe(false);
    expect(staleActive[0]?.kind === 'activityBatch' ? staleActive[0].live : null).toBe(false);
  });

  it('keeps permission records out of activity audit rows', () => {
    const timeline = buildAcpTimelineProjection([
      event({ id: 'failed', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'failed', status: 'failed', title: 'Glob' }),
      event({ id: 'permission', seq: 2, timestamp: '2Z', kind: 'permissionRequest', toolCallId: 'shell', status: 'selected', title: 'Shell' }),
      event({ id: 'shell', seq: 3, timestamp: '3Z', kind: 'toolCall', toolCallId: 'shell', status: 'completed', title: 'Shell' }),
      event({ id: 'answer', seq: 4, timestamp: '4Z', kind: 'textDelta', content: 'done' }),
    ], 'completed').timeline;
    const activity = timeline[0];
    expect(activity?.kind).toBe('activityBatch');
    if (!activity || activity.kind !== 'activityBatch') throw new Error('missing activity');
    expect(activity.events.map(timelineEventKey)).toEqual(['tool-failed', 'tool-shell']);
  });

  it('projects objective ACP actions without guessing command intent', () => {
    const descriptor = objectiveActivityDescriptor(event({
      id: 'powershell', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'powershell-1', status: 'running', title: 'PowerShell', raw: { rawInput: { command: 'npm run web:test' } },
    }));
    expect(descriptor).toEqual({ kind: 'tool', name: 'PowerShell', parameter: 'npm run web:test' });

    const normalized = objectiveActivityDescriptor(event({
      id: 'read', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'read-1', status: 'running', title: 'Reading file',
      raw: { _meta: { goldBandConversation: { toolName: 'Read' } }, rawInput: { file_path: 'src/acp/client.rs' } },
    }));
    expect(normalized.name).toBe('Read');
  });

  it('uses versioned permission timing immediately', () => {
    const timing = { sessionElapsedSeconds: 12, revision: 9, observedAt: '2Z', paused: true, waitReason: 'permission' };
    expect(latestLiveSessionTimingFromEvents([
      event({ id: 'permission-1', seq: 2, timestamp: '2Z', kind: 'permissionRequest', status: 'pending', timing }),
    ])).toMatchObject(timing);
    expect(latestLiveSessionTimingFromEvents([
      event({ id: 'permission-2', seq: 3, timestamp: '3Z', kind: 'permissionRequest', status: 'selected', timing: { ...timing, observedAt: null, paused: false } }),
    ])).toBeNull();
  });

  it('does not wait for another terminal snapshot after stop already returned one', () => {
    expect(shouldAwaitTerminalAcpStop({ sessionId: 'session-1', status: 'cancelled' })).toBe(false);
    expect(shouldAwaitTerminalAcpStop({ sessionId: 'session-1', status: 'cancelling' })).toBe(true);
    expect(shouldAwaitTerminalAcpStop(null)).toBe(false);
  });

  it('keeps the selected session while an accepted stop settles through lifecycle updates', () => {
    const plan = planAcpStopResponse({
      status: 'accepted',
      session: null,
      lifecycle: { acp: { stopping: true } },
    });

    expect(plan.accepted).toBe(true);
    expect(plan.awaitTerminal).toBe(true);
    expect(plan.sessionSnapshot).toBeUndefined();
  });

  it('preserves multiple query parameters with the same label key', () => {
    const blocks = queryBlocksFromTool('Grep `pattern` in `src/`', { file_path: '/project/src/main.ts', pattern: 'TODO', glob: '*.ts' });
    expect(blocks.filter((block) => block.labelKey === 'acp.toolPath').length).toBeGreaterThanOrEqual(1);
    expect(blocks.filter((block) => block.labelKey === 'acp.toolQuery').length).toBeGreaterThanOrEqual(2);
    expect(blocks.some((block) => block.value === 'TODO')).toBe(true);
    expect(blocks.some((block) => block.value === '*.ts')).toBe(true);
  });
});

describe('ACPChatDialog finite branch event cache', () => {
  const makeEvent = (id: string, content: string) => event({ id, seq: Number(id.replace(/\D/g, '')) || 1, timestamp: '1Z', kind: 'textDelta', content });

  it('deduplicates snapshots by item key and keeps stream start stable', () => {
    const previous = event({ id: 'message-1', seq: 20, timestamp: '20Z', kind: 'textDelta', content: 'hello', endedSeq: 20 });
    const incoming = event({ id: 'message-1', seq: 25, timestamp: '25Z', kind: 'textDelta', content: 'hello world', endedSeq: 25 });
    const merged = mergeAcpEvents([previous], [incoming]);
    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ seq: 20, endedSeq: 25, content: 'hello world' });
  });

  it('trims the in-memory window from the requested side', () => {
    const events = Array.from({ length: 100 }, (_, index) => makeEvent(`e${index}`, `msg ${index}`));
    const limited = limitAcpEvents(events, 'start', 30);
    expect(limited).toHaveLength(30);
    expect(limited[0]?.content).toBe('msg 70');
    expect(limited[29]?.content).toBe('msg 99');
  });

  it('stores and restores a bounded branch event window', () => {
    const key = 'test-session-window';
    storeAcpLoadedEvents(key, Array.from({ length: 200 }, (_, index) => makeEvent(`e${index}`, `m${index}`)), 30);
    const restored = restoreAcpLoadedEvents(key, [], 30);
    expect(restored).toHaveLength(30);
    expect(restored[0]?.id).toBe('e170');
  });

  it('separates reused task/run IDs by cache namespace', () => {
    const oldKey = createAcpSessionCacheKey('task-uuid-old', 'task-021', 'run-001', 'round-001', 'bootstrap', 'attempt-001');
    const newKey = createAcpSessionCacheKey('task-uuid-new', 'task-021', 'run-001', 'round-001', 'bootstrap', 'attempt-001');
    storeAcpLoadedEvents(oldKey, [makeEvent('old-event', 'deleted task content')], 360);
    expect(restoreAcpLoadedEvents(newKey, [], 360)).toEqual([]);
    expect(restoreAcpLoadedEvents(oldKey, [], 360)).toHaveLength(1);
  });
});

describe('ACPChatDialog optimistic prompt placement', () => {
  it('keeps a pending prompt at its captured canonical boundary when a response arrives first', () => {
    const previous = event({
      id: 'previous-answer',
      seq: 10,
      timestamp: '10Z',
      kind: 'textDelta',
      content: 'previous turn',
    });
    const response = event({
      id: 'current-answer',
      seq: 12,
      timestamp: '12Z',
      kind: 'textDelta',
      content: 'current turn response',
    });
    const optimistic = optimisticUserEvent('current prompt', 'prompt-1', [], 10);
    const session = { events: [previous, response] } as AcpSessionVm;

    expect(mergeOptimisticSession(session, [optimistic])?.events.map((item) => item.id)).toEqual([
      'previous-answer',
      optimistic.id,
      'current-answer',
    ]);
  });

  it('replaces the anchored optimistic prompt with its canonical promptId without moving the turn', () => {
    const optimistic = optimisticUserEvent('same prompt text', 'prompt-1', [], 10);
    const canonical = event({
      id: 'canonical-prompt',
      seq: 11,
      timestamp: '11Z',
      kind: 'userTextDelta',
      content: 'same prompt text',
      raw: { source: 'goldBandPrompt', promptId: 'prompt-1' },
    });
    const response = event({
      id: 'current-answer',
      seq: 12,
      timestamp: '12Z',
      kind: 'textDelta',
      content: 'current turn response',
    });
    const session = { events: [canonical, response] } as AcpSessionVm;

    const merged = mergeOptimisticSession(session, [optimistic]);
    expect(merged).toBe(session);
    expect(merged?.events.map((item) => item.id)).toEqual([
      'canonical-prompt',
      'current-answer',
    ]);
  });
});
