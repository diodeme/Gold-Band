import { describe, expect, it } from 'vitest';
import { buildAcpTimeline, buildAcpTimelineProjection, collectTimelineItemKeys, createAcpSessionCacheKey, isTopLevelPlanEvent, latestLiveSessionTimingFromEvents, latestStreamingMarkdownItemKey, latestStreamingMarkdownItemKeyFromEvents, limitAcpEvents, mergeAcpEvents, objectiveActivityDescriptor, pruneExpandedTimelineItems, queryBlocksFromTool, restoreAcpLoadedEvents, shouldAwaitTerminalAcpStop, stabilizeTimelineItems, storeAcpLoadedEvents, timelineEventKey } from '../../src/components/acp/ACPChatDialog';
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
    timing: partial.timing,
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

    expect(timeline.map(timelineEventKey)).toEqual(['activity-tool-call-1', 'textDelta-message-1']);
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
      'activity-thoughtDelta-thought-1',
      'textDelta-message-1',
    ]);
    const activity = timeline[0];
    expect(activity?.kind).toBe('activityBatch');
    if (!activity || activity.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(activity.events.map(timelineEventKey)).toEqual([
      'thoughtDelta-thought-1',
      'tool-call-1',
    ]);
    expect(activity.events[0]?.content).toBe('thinking more');
    expect(activity.events[1]?.status).toBe('completed');
    expect(activity.live).toBe(false);
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
    expect(keys).toContain('activity-tool-call-1');
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
    expect(inner.events.map(timelineEventKey)).toEqual(['activity-tool-call-powershell']);
  });

  it('keeps a nested permission request inside the owning agent branch', () => {
    const projection = buildAcpTimelineProjection([
      event({
        id: 'agent', seq: 1, timestamp: '1Z', kind: 'toolCall',
        toolCallId: 'call-agent', status: 'running',
        raw: { _meta: { agentTranscript: { agentLaunch: true } }, rawInput: { description: 'inspect' } },
      }),
      event({
        id: 'permission-1', seq: 2, timestamp: '2Z', kind: 'permissionRequest',
        toolCallId: 'call-shell', status: 'pending', title: 'PowerShell',
        raw: {
          options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
          toolCall: { rawInput: { command: 'git status --short' } },
          _meta: { agentTranscript: { parentToolCallId: 'call-agent' } },
        },
      }),
    ]);

    const agent = projection.timeline[0];
    expect(agent?.kind).toBe('childAgentGroup');
    if (!agent || agent.kind !== 'childAgentGroup') throw new Error('missing agent group');
    expect(agent.events.map(timelineEventKey)).toEqual(['activity-permissionRequest-permission-1']);
    const activity = agent.events[0];
    expect(activity?.kind).toBe('activityBatch');
    if (!activity || activity.kind !== 'activityBatch') throw new Error('missing permission activity');
    expect(activity.events.map(timelineEventKey)).toEqual(['permissionRequest-permission-1']);
  });

  it('uses timing embedded in a permission event to enter permission wait immediately', () => {
    const timing = {
      sessionElapsedSeconds: 12,
      revision: 9,
      observedAt: '2Z',
      paused: true,
      waitReason: 'permission',
    };
    const permission = event({
      id: 'permission-1', seq: 2, timestamp: '2Z', kind: 'permissionRequest',
      status: 'pending', timing,
    });

    expect(latestLiveSessionTimingFromEvents([permission])).toMatchObject(timing);
  });

  it('ignores unversioned permission timing when rebuilding historical events', () => {
    const permission = event({
      id: 'permission-1', seq: 2, timestamp: '2Z', kind: 'permissionRequest',
      status: 'selected',
      timing: {
        sessionElapsedSeconds: 12,
        revision: 9,
        observedAt: null,
        paused: false,
        waitReason: null,
      },
    });

    expect(latestLiveSessionTimingFromEvents([permission])).toBeNull();
  });

  it('does not wait for another terminal snapshot after stop already returned one', () => {
    expect(shouldAwaitTerminalAcpStop({ sessionId: 'session-1', status: 'cancelled' })).toBe(false);
    expect(shouldAwaitTerminalAcpStop({ sessionId: 'session-1', status: 'cancelling' })).toBe(true);
    expect(shouldAwaitTerminalAcpStop(null)).toBe(false);
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

  it('keeps cumulative plan snapshots in the established child-agent plan stream', () => {
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
    expect(projection.todoEntries).toEqual([]);
    const outer = projection.timeline[0];
    expect(outer?.kind).toBe('childAgentGroup');
    if (!outer || outer.kind !== 'childAgentGroup') throw new Error('missing child agent group');
    expect(outer.todoEntries).toEqual([
      { content: 'Shared task', status: 'pending', priority: undefined },
    ]);
    expect(isTopLevelPlanEvent(events[2]!, events)).toBe(false);
  });

  it('does not treat an accepted background Agent launch as completed execution', () => {
    const launch = event({
      id: 'agent-launch', seq: 1, timestamp: '1Z', kind: 'toolCall',
      toolCallId: 'call-agent', status: 'completed',
      raw: {
        _meta: { agentTranscript: { agentLaunch: true } },
        rawInput: { run_in_background: true, description: 'inspect ACP' },
        rawOutput: 'Async agent launched successfully',
      },
    });

    const queued = buildAcpTimelineProjection([launch], 'running').timeline[0];
    expect(queued?.kind).toBe('childAgentGroup');
    if (!queued || queued.kind !== 'childAgentGroup') throw new Error('missing queued agent');
    expect(queued.status).toBe('queued');

    const childTool = event({
      id: 'child-tool', seq: 2, timestamp: '2Z', kind: 'toolCall',
      toolCallId: 'call-read', status: 'completed', title: 'Read file',
      raw: {
        _meta: { agentTranscript: { parentToolCallId: 'call-agent' } },
        rawInput: { file_path: 'src/acp/client.rs' },
      },
    });
    const running = buildAcpTimelineProjection([launch, childTool], 'running').timeline[0];
    expect(running?.kind === 'childAgentGroup' ? running.status : null).toBe('running');

    const interrupted = buildAcpTimelineProjection([launch, childTool], 'cancelled').timeline[0];
    expect(interrupted?.kind === 'childAgentGroup' ? interrupted.status : null).toBe('interrupted');
  });

  it('uses the full-session Agent projection when the finite event window only contains anchors', () => {
    const launch = event({
      id: 'agent-anchor', seq: 1, timestamp: '1Z', kind: 'toolCall',
      toolCallId: 'call-agent', status: 'completed', title: 'Agent',
      raw: {
        _meta: { agentTranscript: { agentLaunch: true, toolName: 'Agent' } },
        rawInput: { run_in_background: true, description: 'inspect ACP' },
      },
    });
    const projection = buildAcpTimelineProjection([launch], 'running', {
      todoEntries: [{ content: 'Main task', status: 'in_progress' }],
      agents: [{
        toolCallId: 'call-agent',
        parentToolCallId: null,
        launchStatus: 'completed',
        executionStatus: 'running',
        eventCount: 97,
        toolCallCount: 42,
        readFileCount: 18,
        writtenFileCount: 3,
        todoEntries: [{ content: 'Inspect timeline', status: 'completed' }],
      }],
    });

    expect(projection.todoEntries).toEqual([{ content: 'Main task', status: 'in_progress' }]);
    const agent = projection.timeline[0];
    expect(agent?.kind).toBe('childAgentGroup');
    if (!agent || agent.kind !== 'childAgentGroup') throw new Error('missing projected Agent');
    expect(agent).toMatchObject({
      status: 'running',
      eventCount: 97,
      toolCallCount: 42,
      readFileCount: 18,
      writtenFileCount: 3,
      todoEntries: [{ content: 'Inspect timeline', status: 'completed' }],
    });
  });

  it('preserves unrelated Agent branch identity when a live event updates one branch', () => {
    const agent = (id: string, seq: number) => event({
      id: `agent-${id}`, seq, timestamp: `${seq}Z`, kind: 'toolCall',
      toolCallId: `call-${id}`, status: 'running', title: 'Agent',
      raw: { _meta: { agentTranscript: { agentLaunch: true, toolName: 'Agent' } }, rawInput: { description: id } },
    });
    const agentA = agent('a', 1);
    const agentB = agent('b', 2);
    const initial = buildAcpTimeline([agentA, agentB]);
    const updated = buildAcpTimeline([
      agentA,
      agentB,
      event({
        id: 'tool-b', seq: 3, timestamp: '3Z', kind: 'toolCall',
        toolCallId: 'read-b', status: 'running', title: 'Read file',
        raw: { _meta: { agentTranscript: { parentToolCallId: 'call-b', toolName: 'Read' } }, rawInput: { file_path: 'b.ts' } },
      }),
    ]);
    const stable = stabilizeTimelineItems(updated, initial);

    expect(stable[0]).toBe(initial[0]);
    expect(stable[1]).not.toBe(initial[1]);
  });

  it('archives each activity batch when formal text begins and keeps the next batch separate', () => {
    const timeline = buildAcpTimelineProjection([
      event({ id: 'thought-1', seq: 1, timestamp: '1Z', kind: 'thoughtDelta', content: 'inspect' }),
      event({ id: 'read-1', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'read-1', status: 'completed', title: 'Read file' }),
      event({ id: 'text-1', seq: 3, timestamp: '3Z', kind: 'textDelta', content: 'Finding one.' }),
      event({ id: 'grep-1', seq: 4, timestamp: '4Z', kind: 'toolCall', toolCallId: 'grep-1', status: 'running', title: 'Grep `parentToolCallId`' }),
    ], 'running').timeline;

    expect(timeline.map(timelineEventKey)).toEqual([
      'activity-thoughtDelta-thought-1',
      'textDelta-text-1',
      'activity-tool-grep-1',
    ]);
    expect(timeline[0]?.kind === 'activityBatch' ? timeline[0].live : null).toBe(false);
    expect(timeline[2]?.kind === 'activityBatch' ? timeline[2].live : null).toBe(true);
  });

  it('keeps failed tools, permission decisions, and later tools in one activity segment until text begins', () => {
    const events = [
      event({ id: 'glob-failed', seq: 1, timestamp: '1Z', kind: 'toolCall', toolCallId: 'glob-failed', status: 'failed', title: 'Glob `*`' }),
      event({ id: 'glob-completed', seq: 2, timestamp: '2Z', kind: 'toolCall', toolCallId: 'glob-completed', status: 'completed', title: 'Glob `README*`' }),
      event({ id: 'read-failed', seq: 3, timestamp: '3Z', kind: 'toolCall', toolCallId: 'read-failed', status: 'failed', title: 'Read README.zh-CN.md' }),
      event({
        id: 'permission', seq: 4, timestamp: '4Z', kind: 'permissionRequest',
        toolCallId: 'bash', status: 'selected', title: 'Bash',
        raw: { optionId: 'allow', options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }] },
      }),
      event({ id: 'bash', seq: 5, timestamp: '5Z', kind: 'toolCall', toolCallId: 'bash', status: 'completed', title: 'Bash `pwd`' }),
      event({ id: 'read-completed', seq: 6, timestamp: '6Z', kind: 'toolCall', toolCallId: 'read-completed', status: 'completed', title: 'Read README.md' }),
      event({ id: 'answer', seq: 7, timestamp: '7Z', kind: 'textDelta', content: 'README content.' }),
    ];

    const timeline = buildAcpTimelineProjection(events, 'completed').timeline;
    expect(timeline.map(timelineEventKey)).toEqual([
      'activity-tool-glob-failed',
      'textDelta-answer',
    ]);
    const activity = timeline[0];
    expect(activity?.kind).toBe('activityBatch');
    if (!activity || activity.kind !== 'activityBatch') throw new Error('missing activity segment');
    expect(activity.events.map(timelineEventKey)).toEqual([
      'tool-glob-failed',
      'tool-glob-completed',
      'tool-read-failed',
      'permissionRequest-permission',
      'tool-bash',
      'tool-read-completed',
    ]);
  });

  it('keeps the activity row identity stable when later tools arrive while details are open', () => {
    const first = event({
      id: 'glob', seq: 1, timestamp: '1Z', kind: 'toolCall',
      toolCallId: 'glob', status: 'completed', title: 'Glob `README*`',
    });
    const later = event({
      id: 'read', seq: 2, timestamp: '2Z', kind: 'toolCall',
      toolCallId: 'read', status: 'failed', title: 'Read README.zh-CN.md',
    });
    const initial = buildAcpTimelineProjection([first], 'running').timeline;
    const updated = buildAcpTimelineProjection([first, later], 'running').timeline;
    const stable = stabilizeTimelineItems(updated, initial);

    expect(timelineEventKey(updated[0]!)).toBe(timelineEventKey(initial[0]!));
    expect(stable).toHaveLength(1);
    expect(stable[0]?.kind).toBe('activityBatch');
    if (!stable[0] || stable[0].kind !== 'activityBatch') throw new Error('missing stable activity');
    expect(stable[0].events.map(timelineEventKey)).toEqual(['tool-glob', 'tool-read']);
    expect(stable[0].events[0]).toBe(
      initial[0]?.kind === 'activityBatch' ? initial[0].events[0] : null,
    );
  });

  it('projects the ACP tool action literally without guessing command intent', () => {
    const descriptor = objectiveActivityDescriptor(event({
      id: 'powershell', seq: 1, timestamp: '1Z', kind: 'toolCall',
      toolCallId: 'powershell-1', status: 'running', title: 'PowerShell',
      raw: { rawInput: { command: 'npm run web:test' } },
    }));

    expect(descriptor).toEqual({
      kind: 'tool',
      name: 'PowerShell',
      parameter: 'npm run web:test',
    });
    expect(descriptor.parameter).not.toContain('运行测试');

    const grepDescriptor = objectiveActivityDescriptor(event({
      id: 'grep', seq: 2, timestamp: '2Z', kind: 'toolCall',
      toolCallId: 'grep-1', status: 'running', title: 'Grep `parentToolCallId`',
      raw: { rawInput: { pattern: 'parentToolCallId' } },
    }));
    expect(grepDescriptor.parameter).toBe('parentToolCallId');

    const normalizedDescriptor = objectiveActivityDescriptor(event({
      id: 'normalized-tool', seq: 3, timestamp: '3Z', kind: 'toolCall',
      toolCallId: 'read-1', status: 'running', title: 'Reading file',
      raw: {
        _meta: { agentTranscript: { toolName: 'Read' } },
        rawInput: { file_path: 'src/acp/client.rs' },
      },
    }));
    expect(normalizedDescriptor.name).toBe('Read');
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
