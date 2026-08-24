import { describe, expect, it } from 'vitest';
import {
  applyPendingElicitationEventsToSession,
  buildAcpTimeline,
  applyAgentBranchResultToSession,
  calculateSessionElapsedSeconds,
  canInferPendingInteractionFromWindow,
  clearPendingOptimisticPromptsAfterStop,
  contextCompactionUsageBefore,
  createLiveAcpSessionShell,
  createVisibleAcpSession,
  latestLiveSessionTimingFromEvents,
  latestSessionTimingFromEvents,
  liveTimelineUpdatesFromEvents,
  mergeAcpEvents,
  partitionAcpLiveTimingUpdates,
  pendingElicitationFromEvents,
  pendingPermissionFromEvents,
  promptRetryFooterKind,
  reconcileAcpSessionForDisplay,
  runtimeControlMessageParts,
  isAcpSessionReadyForInitialDisplay,
  isAcpConversationAtBottom,
  stabilizeAcpSessionTimingForDisplay,
  stabilizeAcpSessionTimingPatchForDisplay,
  useSessionTimingSeconds,
  acpSessionLoadErrorReason,
  visibleAcpBannerError,
} from '../src/components/acp/ACPChatDialog';
import type { AcpSessionVm, AcpUiEventVm } from '../src/types';

function event(partial: Partial<AcpUiEventVm>): AcpUiEventVm {
  return {
    id: partial.id ?? `event-${partial.seq ?? 1}`,
    seq: partial.seq ?? 1,
    timestamp: partial.timestamp ?? `${partial.seq ?? 1}Z`,
    kind: partial.kind ?? 'textDelta',
    sessionId: partial.sessionId ?? 'session-1',
    content: partial.content,
    title: partial.title,
    toolCallId: partial.toolCallId,
    status: partial.status,
    startedSeq: partial.startedSeq,
    endedSeq: partial.endedSeq,
    startedAt: partial.startedAt,
    endedAt: partial.endedAt,
    raw: partial.raw,
    timing: partial.timing,
  };
}

function session(partial: Partial<AcpSessionVm>): AcpSessionVm {
  return {
    branchId: partial.branchId,
    parentBranchId: partial.parentBranchId,
    readOnly: partial.readOnly,
    branchExecution: partial.branchExecution,
    sessionId: partial.sessionId ?? 'session-1',
    provider: partial.provider ?? 'claude-acp',
    status: partial.status ?? 'running',
    sessionUpdatedAt: partial.sessionUpdatedAt,
    sessionElapsedSeconds: partial.sessionElapsedSeconds,
    timing: partial.timing,
    title: partial.title,
    adapterId: partial.adapterId,
    adapterDisplayName: partial.adapterDisplayName,
    systemPromptAppend: partial.systemPromptAppend,
    config: partial.config,
    restored: partial.restored ?? false,
    events: partial.events ?? [],
    eventPage: partial.eventPage ?? {
      loadedCount: 0,
      total: 0,
      hasOlder: false,
      hasNewer: false,
    },
    pendingPermissions: partial.pendingPermissions ?? [],
    pendingElicitations: partial.pendingElicitations ?? [],
    diagnostics: partial.diagnostics ?? {
      rawFrameCount: 0,
      eventCount: 0,
      errorCount: 0,
    },
  };
}

describe('ACP chat event handling', () => {
  it('stops the read-only Agent session when its canonical result arrives', () => {
    const current = session({
      status: 'running',
      sessionUpdatedAt: '10Z',
      timing: {
        sessionElapsedSeconds: 9,
        activeTurnStartedAt: '1Z',
        activeTurnLastActivityAt: '10Z',
        paused: false,
      },
    });
    const updated = applyAgentBranchResultToSession(current, event({
      seq: 11,
      timestamp: '11Z',
      kind: 'textDelta',
      status: 'completed',
      raw: { source: 'agentBranchResult' },
    }));

    expect(updated).toMatchObject({
      status: 'completed',
      sessionUpdatedAt: '11Z',
      timing: {
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        paused: true,
      },
    });
  });

  it('does not treat the end of a truncated event window as the conversation bottom', () => {
    expect(isAcpConversationAtBottom(true, true)).toBe(false);
    expect(isAcpConversationAtBottom(true, false)).toBe(true);
    expect(isAcpConversationAtBottom(false, false)).toBe(false);
  });

  it('shows runtime control failures in the session banner', () => {
    const acpSession = session({
      diagnostics: {
        rawFrameCount: 0,
        eventCount: 0,
        errorCount: 0,
      },
    });

    expect(
      visibleAcpBannerError(
        'Round 数已达上限：max rounds exceeded for $new-round: 2 > 1',
        acpSession,
        [],
      ),
    ).toBe('Round 数已达上限：max rounds exceeded for $new-round: 2 > 1');
  });

  it('keeps the structured provider detail instead of replacing it with a generic adapter hint', () => {
    const providerError = 'ACP `initialize` failed: Already initialized (Internal error)';
    const acpSession = session({
      diagnostics: {
        rawFrameCount: 2,
        eventCount: 0,
        errorCount: 1,
        lastError: providerError,
      },
    });

    expect(
      acpSessionLoadErrorReason(null, null, acpSession, 'Generic missing session'),
    ).toBe(providerError);
  });

  it('uses raw permission request id instead of display id', () => {
    const permission = pendingPermissionFromEvents(
      [
        event({
          id: 'permission-0',
          seq: 10,
          kind: 'permissionRequest',
          status: 'pending',
          title: 'Write file',
          raw: {
            requestId: '0',
            options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
          },
        }),
      ],
      new Set(),
    );

    expect(permission?.requestId).toBe('0');
    expect(permission?.raw).toMatchObject({ requestId: '0' });
  });

  it('derives legacy permission request id from display id and dismisses by canonical id', () => {
    const events = [
      event({
        id: 'permission-permission-0',
        seq: 10,
        kind: 'permissionRequest',
        status: 'pending',
        title: 'Write file',
        raw: {
          options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
        },
      }),
    ];

    expect(pendingPermissionFromEvents(events, new Set())?.requestId).toBe('0');
    expect(pendingPermissionFromEvents(events, new Set(['0']))).toBeNull();
  });

  it('does not surface answered elicitation requests after a response event arrives', () => {
    const events = [
      event({
        id: 'elicit-1',
        seq: 10,
        kind: 'elicitationRequest',
        status: 'pending',
        content: 'Choose one',
        raw: { type: 'object', properties: { answer: { type: 'string' } } },
      }),
      event({
        id: 'elicit-1-response',
        seq: 11,
        kind: 'elicitationResponse',
        status: 'completed',
        raw: { elicitationId: 'elicit-1', action: 'accept' },
      }),
    ];

    expect(pendingElicitationFromEvents(events, new Map())).toBeNull();
  });

  it('removes the answered interaction card without creating an answer bubble and keeps the tool card', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'ask-tool',
        seq: 10,
        kind: 'toolCall',
        toolCallId: 'ask-call',
        status: 'pending',
        title: 'Asking for your input',
        raw: { _meta: { goldBandConversation: { toolName: 'AskUserQuestion' } } },
      }),
      event({
        id: 'elicit-answered',
        seq: 11,
        kind: 'elicitationRequest',
        status: 'pending',
        content: 'Choose one',
        raw: { type: 'object', properties: { answer: { type: 'string' } } },
      }),
      event({
        id: 'elicit-answered-response',
        seq: 12,
        kind: 'elicitationResponse',
        status: 'completed',
        raw: {
          elicitationId: 'elicit-answered',
          action: 'accept',
          content: { answer: 'Tea' },
        },
      }),
      event({
        id: 'ask-tool-update',
        seq: 13,
        kind: 'toolCallUpdate',
        toolCallId: 'ask-call',
        status: 'completed',
        content: 'User answered questions',
      }),
    ]);

    expect(timeline).toHaveLength(1);
    expect(timeline[0]).toMatchObject({ kind: 'activityBatch' });
    if (timeline[0]?.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(timeline[0].events[0]).toMatchObject({
      kind: 'toolCall',
      toolCallId: 'ask-call',
      status: 'completed',
      content: 'User answered questions',
    });
  });

  it('keeps unanswered elicitation requests pending until a response event exists', () => {
    const events = [
      event({
        id: 'elicit-2',
        seq: 10,
        kind: 'elicitationRequest',
        status: 'pending',
        content: 'Choose one',
        raw: { type: 'object', properties: { answer: { type: 'string' } } },
      }),
    ];

    expect(pendingElicitationFromEvents(events, new Map())?.elicitationId).toBe('elicit-2');
  });

  it('projects a live elicitation into authoritative session state without relying on timing', () => {
    const current = session({
      status: 'running',
      timing: {
        sessionElapsedSeconds: 12,
        paused: false,
      },
    });
    const message = 'Choose a database';
    const requestedSchema = {
      type: 'object',
      properties: { database: { type: 'string' } },
    };
    const updated = applyPendingElicitationEventsToSession(current, [event({
      id: 'elicit-live',
      seq: 20,
      kind: 'elicitationRequest',
      status: 'pending',
      content: message,
      raw: {
        message,
        toolCallId: 'ask-tool-1',
        requestedSchema,
      },
    })]);

    expect(updated?.timing?.paused).toBe(false);
    expect(updated?.pendingElicitations).toEqual([{
      elicitationId: 'elicit-live',
      message,
      toolCallId: 'ask-tool-1',
      requestedSchema,
      raw: {
        message,
        toolCallId: 'ask-tool-1',
        requestedSchema,
      },
    }]);
  });

  it('clears authoritative elicitation state on response or terminal session', () => {
    const pending = session({
      status: 'running',
      pendingElicitations: [{
        elicitationId: 'elicit-live',
        message: 'Choose',
        requestedSchema: { type: 'object' },
        raw: {},
      }],
    });
    const resolved = applyPendingElicitationEventsToSession(pending, [event({
      id: 'elicit-live-response',
      seq: 21,
      kind: 'elicitationResponse',
      status: 'completed',
      raw: { elicitationId: 'elicit-live', action: 'accept' },
    })]);
    const terminal = applyPendingElicitationEventsToSession(
      { ...pending, status: 'cancelled' },
      [],
    );

    expect(resolved?.pendingElicitations).toEqual([]);
    expect(terminal?.pendingElicitations).toEqual([]);
  });

  it('preserves a live pending elicitation across a stale active session snapshot', () => {
    const pendingRequest = {
      elicitationId: 'elicit-live',
      message: 'Choose',
      requestedSchema: { type: 'object' },
      raw: {},
    };
    const live = session({ pendingElicitations: [pendingRequest] });
    const staleSnapshot = session({
      pendingElicitations: [],
      events: [],
    });
    const resolvedSnapshot = session({
      pendingElicitations: [],
      events: [event({
        id: 'elicit-live-response',
        seq: 22,
        kind: 'elicitationResponse',
        status: 'completed',
        raw: { elicitationId: 'elicit-live', action: 'accept' },
      })],
    });

    expect(
      reconcileAcpSessionForDisplay(live, staleSnapshot)?.pendingElicitations,
    ).toEqual([pendingRequest]);
    expect(
      reconcileAcpSessionForDisplay(live, resolvedSnapshot)?.pendingElicitations,
    ).toEqual([]);
  });

  it('does not infer pending interactions from a terminal session history window', () => {
    const failedSession = session({
      status: 'failed',
      timing: {
        paused: true,
        waitReason: 'elicitation',
      },
    });

    expect(
      canInferPendingInteractionFromWindow(
        failedSession,
        false,
        'elicitation',
      ),
    ).toBe(false);
  });

  it('does not infer a stale pending card while newer history pages exist', () => {
    const runningSession = session({
      status: 'running',
      timing: {
        paused: true,
        waitReason: 'elicitation',
      },
    });

    expect(
      canInferPendingInteractionFromWindow(
        runningSession,
        true,
        'elicitation',
      ),
    ).toBe(false);
  });

  it('infers only the interaction kind described by the current session wait', () => {
    const runningSession = session({
      status: 'running',
      timing: {
        paused: true,
        waitReason: 'elicitation',
      },
    });

    expect(
      canInferPendingInteractionFromWindow(
        runningSession,
        false,
        'elicitation',
      ),
    ).toBe(true);
    expect(
      canInferPendingInteractionFromWindow(
        runningSession,
        false,
        'permission',
      ),
    ).toBe(false);
  });

  it('does not infer a pending card from an active non-waiting session', () => {
    const runningSession = session({
      status: 'running',
      timing: {
        paused: false,
      },
    });

    expect(
      canInferPendingInteractionFromWindow(
        runningSession,
        false,
        'elicitation',
      ),
    ).toBe(false);
  });

  it('recovers a pending card from the full typed elicitation request after refresh', () => {
    const message = 'Round 11 | 歧义：23.5%\n\n管理端 API 与菜单的权限标识如何设计？';
    const requestedSchema = {
      type: 'object' as const,
      properties: {
        question_0: {
          type: 'string' as const,
          title: '管理端权限标识',
          oneOf: [{ const: 'admin-only', title: 'admin-only' }],
        },
      },
    };
    const events = [
      event({
        id: 'elicit-full-request',
        seq: 10,
        kind: 'elicitationRequest',
        status: 'pending',
        content: message,
        raw: {
          mode: 'form',
          sessionId: 'session-1',
          toolCallId: 'call-1',
          message,
          requestedSchema,
          _meta: { source: 'claude-agent-acp' },
        },
      }),
    ];

    expect(pendingElicitationFromEvents(events, new Map())).toEqual({
      elicitationId: 'elicit-full-request',
      message,
      requestedSchema,
    });
  });

  it('does not resurface older pending elicitation requests after a newer one was answered', () => {
    const events = [
      event({
        id: 'elicit-old',
        seq: 10,
        kind: 'elicitationRequest',
        status: 'pending',
        content: 'Old question',
        raw: { type: 'object', properties: { answer: { type: 'string' } } },
      }),
      event({
        id: 'elicit-new',
        seq: 20,
        kind: 'elicitationRequest',
        status: 'pending',
        content: 'New question',
        raw: { type: 'object', properties: { answer: { type: 'string' } } },
      }),
      event({
        id: 'elicit-new-response',
        seq: 21,
        kind: 'elicitationResponse',
        status: 'completed',
        raw: { elicitationId: 'elicit-new', action: 'accept' },
      }),
    ];

    expect(pendingElicitationFromEvents(events, new Map())).toBeNull();
  });

  it('keeps tool call updates merged by tool id', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'tool-call-a',
        seq: 1,
        kind: 'toolCall',
        toolCallId: 'call-a',
        status: 'pending',
        title: 'Write',
        raw: { rawInput: { file_path: 'a.py' } },
      }),
      event({
        id: 'tool-call-a-update',
        seq: 2,
        kind: 'toolCallUpdate',
        toolCallId: 'call-a',
        status: 'completed',
        content: 'done',
      }),
    ]);

    expect(timeline).toHaveLength(1);
    expect(timeline[0]).toMatchObject({ kind: 'activityBatch' });
    if (timeline[0]?.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(timeline[0].events[0]).toMatchObject({
      kind: 'toolCall',
      toolCallId: 'call-a',
      status: 'completed',
      content: 'done',
    });
  });

  it('keeps stable text and thought stream items merged without creating duplicate rows', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'assistant-message-m1',
        seq: 1,
        kind: 'textDelta',
        content: 'hello',
      }),
      event({
        id: 'assistant-message-m1',
        seq: 2,
        kind: 'textDelta',
        content: 'hello world',
      }),
      event({
        id: 'assistant-thought-m1',
        seq: 3,
        kind: 'thoughtDelta',
        content: 'thinking',
      }),
      event({
        id: 'assistant-thought-m1',
        seq: 4,
        kind: 'thoughtDelta',
        content: 'thinking done',
      }),
    ]);

    expect(timeline).toHaveLength(2);
    expect(timeline[0]).toMatchObject({ kind: 'textDelta', content: 'hello world' });
    expect(timeline[1]).toMatchObject({ kind: 'activityBatch' });
    if (timeline[1]?.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(timeline[1].events[0]).toMatchObject({ kind: 'thoughtDelta', content: 'thinking done' });
  });

  it('keeps repeated Gold Band user prompts when prompt ids differ', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'gold-band-user-prompt-71',
        seq: 71,
        timestamp: '1782356175Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'acp-prompt-1' },
      }),
      event({
        id: 'gold-band-user-prompt-207',
        seq: 207,
        timestamp: '1782356183Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'acp-prompt-2' },
      }),
      event({
        id: 'gold-band-user-prompt-381',
        seq: 381,
        timestamp: '1782356193Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'acp-prompt-3' },
      }),
    ]);

    expect(timeline).toHaveLength(3);
    expect(timeline.map((item) => 'content' in item ? item.content : null)).toEqual(['继续', '继续', '继续']);
  });

  it('extracts the latest timing patch from an event window', () => {
    const timing = latestSessionTimingFromEvents([
      event({
        seq: 1,
        kind: 'userTextDelta',
        raw: { source: 'goldBandPrompt' },
        timing: {
          sessionElapsedSeconds: 0,
          activeTurnStartedAt: '100Z',
          activeTurnLastActivityAt: '100Z',
          permissionWaitStartedAt: null,
          paused: false,
          reason: 'active',
        },
      }),
      event({
        seq: 2,
        kind: 'textDelta',
        timing: {
          sessionElapsedSeconds: 12,
          activeTurnStartedAt: '100Z',
          activeTurnLastActivityAt: '112Z',
          permissionWaitStartedAt: null,
          paused: false,
          reason: 'active',
        },
      }),
    ]);

    expect(timing).toEqual({
      sessionElapsedSeconds: 12,
      revision: null,
      observedAt: null,
      activeTurnStartedAt: '100Z',
      activeTurnLastActivityAt: '112Z',
      permissionWaitStartedAt: null,
      userWaitStartedAt: null,
      waitReason: null,
      paused: false,
    });
  });

  it('keeps live-only timing and usage updates out of the timeline event merge', () => {
    const updates = [
      event({
        seq: 3,
        kind: 'timingUpdate',
        timing: {
          sessionElapsedSeconds: 13,
          activeTurnStartedAt: '100Z',
          activeTurnLastActivityAt: '113Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'tick',
        },
      }),
      event({
        seq: 4,
        kind: 'usageUpdate',
        raw: { sessionUpdate: 'usage_update', used: 7_920, size: 258_400 },
      }),
      event({ seq: 5, kind: 'textDelta', content: 'hello' }),
    ];

    expect(latestSessionTimingFromEvents(updates)?.sessionElapsedSeconds).toBe(13);
    expect(liveTimelineUpdatesFromEvents(updates).map((update) => update.kind)).toEqual([
      'textDelta',
    ]);
  });

  it('splits marked fenced runtime control JSON from assistant text', () => {
    const content = 'hello!\n```json\n{"a":"b"}\n```';
    const parts = runtimeControlMessageParts(event({
      kind: 'textDelta',
      content,
      raw: {
        runtimeControlOutputDisplay: {
          kind: 'workflow-output',
          artifactName: 'accept-result',
          jsonText: '{"a":"b"}',
          start: content.indexOf('```json'),
          end: content.length,
          fenced: true,
          parseStatus: 'valid',
        },
      },
    }));

    expect(parts.visibleText).toBe('hello!');
    expect(parts.display?.jsonText).toBe('{"a":"b"}');
  });

  it('splits marked bare runtime control JSON from assistant text', () => {
    const content = 'hello!\n{"a":"b"}';
    const parts = runtimeControlMessageParts(event({
      kind: 'textDelta',
      content,
      raw: {
        runtimeControlOutputDisplay: {
          kind: 'dynamic-node-completion',
          artifactName: 'dynamic-node-completion',
          jsonText: '{"a":"b"}',
          start: content.indexOf('{'),
          end: content.length,
          fenced: false,
          parseStatus: 'valid',
        },
      },
    }));

    expect(parts.visibleText).toBe('hello!');
    expect(parts.display?.kind).toBe('dynamic-node-completion');
  });

  it('hides the message bubble text when marked runtime control output is only JSON', () => {
    const content = '{"a":"b"}';
    const parts = runtimeControlMessageParts(event({
      kind: 'textDelta',
      content,
      raw: {
        runtimeControlOutputDisplay: {
          kind: 'workflow-output',
          artifactName: 'test-result',
          jsonText: content,
          start: 0,
          end: content.length,
          parseStatus: 'valid',
        },
      },
    }));

    expect(parts.visibleText).toBe('');
    expect(parts.display?.jsonText).toBe(content);
  });

  it('does not split unmarked assistant JSON', () => {
    const content = 'hello!\n{"a":"b"}';
    const parts = runtimeControlMessageParts(event({
      kind: 'textDelta',
      content,
    }));

    expect(parts.display).toBeNull();
    expect(parts.visibleText).toBe(content);
  });

  it('splits every marked runtime repair attempt independently', () => {
    const attempts = ['A', 'B', 'C'].map((label, index) => {
      const content = `${label}\n{"try":${index + 1}}`;
      return runtimeControlMessageParts(event({
        id: `assistant-${label}`,
        seq: 20 + index,
        kind: 'textDelta',
        content,
        raw: {
          runtimeControlOutputDisplay: {
            kind: 'dynamic-node-completion',
            artifactName: 'dynamic-node-completion',
            jsonText: `{"try":${index + 1}}`,
            start: content.indexOf('{'),
            end: content.length,
            parseStatus: 'valid',
          },
        },
      }));
    });

    expect(attempts.map((parts) => parts.visibleText)).toEqual(['A', 'B', 'C']);
    expect(attempts.map((parts) => parts.display?.jsonText)).toEqual([
      '{"try":1}',
      '{"try":2}',
      '{"try":3}',
    ]);
  });

  it('separates live timing updates from deferred timeline rendering work', () => {
    const timingUpdate = event({
      seq: 3,
      kind: 'timingUpdate',
      timing: {
        sessionElapsedSeconds: 38,
        activeTurnStartedAt: '100Z',
        activeTurnLastActivityAt: '138Z',
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: false,
        reason: 'tick',
      },
    });
    const textUpdate = event({ seq: 4, kind: 'textDelta', content: 'streaming text' });

    const partitioned = partitionAcpLiveTimingUpdates([textUpdate, timingUpdate]);

    expect(partitioned.timingUpdates).toEqual([timingUpdate]);
    expect(partitioned.timelineUpdates).toEqual([textUpdate]);
  });

  it('does not treat historical event timing as the live session timing source', () => {
    const updates = [
      event({
        seq: 240,
        kind: 'usageUpdate',
        timing: {
          sessionElapsedSeconds: 20,
          activeTurnStartedAt: '1782980435Z',
          activeTurnLastActivityAt: '1782980455Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'active',
        },
      }),
      event({
        seq: 819,
        kind: 'permissionRequest',
        status: 'selected',
        timing: {
          sessionElapsedSeconds: 69,
          activeTurnStartedAt: '1782980458Z',
          activeTurnLastActivityAt: '1782980531Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'active',
        },
      }),
    ];

    expect(latestSessionTimingFromEvents(updates)?.sessionElapsedSeconds).toBe(69);
    expect(latestLiveSessionTimingFromEvents(updates)).toBeNull();
  });

  it('uses timingUpdate as the live session timing source even with older historical events present', () => {
    const updates = [
      event({
        seq: 240,
        kind: 'usageUpdate',
        timing: {
          sessionElapsedSeconds: 20,
          activeTurnStartedAt: '1782980435Z',
          activeTurnLastActivityAt: '1782980455Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'active',
        },
      }),
      event({
        seq: 910,
        kind: 'timingUpdate',
        timing: {
          sessionElapsedSeconds: 85,
          activeTurnStartedAt: '1782980458Z',
          activeTurnLastActivityAt: '1782980555Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'tick',
        },
      }),
    ];

    expect(latestLiveSessionTimingFromEvents(updates)?.sessionElapsedSeconds).toBe(85);
  });

  it('uses event timing for event-only live session shells while waiting for a session payload', () => {
    const shell = createLiveAcpSessionShell([
      event({
        seq: 248,
        kind: 'thoughtDelta',
        timing: {
          sessionElapsedSeconds: 27,
          activeTurnStartedAt: '1782981381Z',
          activeTurnLastActivityAt: '1782981408Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
          reason: 'active',
        },
      }),
    ], 'running');

    expect(shell.timing?.sessionElapsedSeconds).toBe(27);
    expect(shell.sessionElapsedSeconds).toBe(27);
  });

  it('treats active sessions with visible timeline events as displayable during readiness loading', () => {
    expect(isAcpSessionReadyForInitialDisplay(session({
      status: 'running',
      systemPromptAppend: null,
      config: null,
      events: [
        event({
          id: 'assistant-message-1',
          seq: 100,
          kind: 'textDelta',
          content: 'The run is already streaming.',
        }),
      ],
    }))).toBe(true);
  });

  it('keeps metadata-only active sessions behind the readiness loading gate', () => {
    expect(isAcpSessionReadyForInitialDisplay(session({
      status: 'running',
      systemPromptAppend: null,
      config: null,
      events: [
        event({
          id: 'available-commands-1',
          seq: 100,
          kind: 'availableCommands',
          raw: { sessionUpdate: 'available_commands_update' },
        }),
      ],
    }))).toBe(false);
  });

  it('keeps a terminal summary with unloaded events behind the readiness loading gate', () => {
    expect(isAcpSessionReadyForInitialDisplay(session({
      status: 'completed',
      events: [],
      eventPage: {
        loadedCount: 0,
        total: 24,
        oldestSeq: null,
        newestSeq: null,
        hasOlder: true,
        hasNewer: false,
        oldestCursor: null,
        newestCursor: null,
      },
    }))).toBe(false);
  });

  it('treats a canonical Agent branch VM as ready without root session metadata', () => {
    expect(isAcpSessionReadyForInitialDisplay(session({
      branchId: 'agent-1',
      parentBranchId: 'root',
      readOnly: true,
      branchExecution: {
        agentExecutionId: 'agent-1',
        parentAgentExecutionId: null,
        executionStatus: 'interrupted',
        eventCount: 9,
        toolCallCount: 4,
        readFileCount: 2,
        writtenFileCount: 1,
        hasAttention: false,
        todoEntries: [],
      },
      status: 'interrupted',
      systemPromptAppend: null,
      config: null,
      events: [],
    }))).toBe(true);
  });

  it('keeps snapshot prompt events visible while live events are still catching up', () => {
    const prompt = event({
      id: 'gold-band-user-prompt-1',
      seq: 1,
      kind: 'userTextDelta',
      content: 'Build the feature',
      status: 'completed',
      raw: { source: 'goldBandPrompt', synthetic: true },
    });
    const assistant = event({
      id: 'assistant-message-1',
      seq: 12,
      kind: 'textDelta',
      content: 'Working on it',
    });

    const visible = createVisibleAcpSession(
      session({
        events: [prompt],
        eventPage: {
          loadedCount: 1,
          total: 1,
          oldestSeq: 1,
          newestSeq: 1,
          hasOlder: false,
          hasNewer: false,
        },
      }),
      [assistant],
      100,
    );

    expect(visible.events.map((item) => item.id)).toEqual([
      'gold-band-user-prompt-1',
      'assistant-message-1',
    ]);
    expect(visible.eventPage).toEqual({
      loadedCount: 1,
      total: 1,
      oldestSeq: 1,
      newestSeq: 1,
      hasOlder: false,
      hasNewer: false,
    });
  });

  it('uses backend timing as the session elapsed source of truth', () => {
    expect(
      useSessionTimingSeconds(
        {
          sessionElapsedSeconds: 12,
          activeTurnStartedAt: '100Z',
          activeTurnLastActivityAt: '112Z',
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: false,
        },
        null,
        true,
      ),
    ).toBe(12);
  });

  it('keeps session timing monotonic when stale terminal payloads arrive out of order', () => {
    const current = session({
      status: 'cancelled',
      sessionUpdatedAt: '1782986478Z',
      sessionElapsedSeconds: 61,
      timing: {
        sessionElapsedSeconds: 61,
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
    });
    const stale = session({
      status: 'cancelled',
      sessionUpdatedAt: '1782986478Z',
      sessionElapsedSeconds: 47,
      timing: {
        sessionElapsedSeconds: 47,
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
    });

    const stabilized = stabilizeAcpSessionTimingForDisplay(current, stale);

    expect(stabilized?.timing?.sessionElapsedSeconds).toBe(61);
    expect(stabilized?.sessionElapsedSeconds).toBe(61);
    expect(stabilized?.status).toBe('cancelled');
  });

  it('rejects stale session timing by revision even when status payload arrives later', () => {
    const current = session({
      status: 'cancelled',
      sessionUpdatedAt: '1782986478Z',
      timing: {
        sessionElapsedSeconds: 61,
        revision: 830,
        observedAt: '1782986478Z',
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
    });
    const stale = session({
      status: 'cancelled',
      sessionUpdatedAt: '1782986478Z',
      timing: {
        sessionElapsedSeconds: 47,
        revision: 678,
        observedAt: '1782986453Z',
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
      events: [
        event({
          seq: 900,
          kind: 'textDelta',
          content: 'late event metadata still applies',
        }),
      ],
    });

    const stabilized = stabilizeAcpSessionTimingForDisplay(current, stale);

    expect(stabilized?.timing?.sessionElapsedSeconds).toBe(61);
    expect(stabilized?.timing?.revision).toBe(830);
    expect(stabilized?.events).toHaveLength(1);
  });

  it('accepts newer session timing by revision even if the elapsed value is lower', () => {
    const current = session({
      timing: {
        sessionElapsedSeconds: 61,
        revision: 830,
        observedAt: '1782986478Z',
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
    });
    const next = session({
      timing: {
        sessionElapsedSeconds: 2,
        revision: 831,
        observedAt: '1782986479Z',
        activeTurnStartedAt: '1782986479Z',
        activeTurnLastActivityAt: '1782986481Z',
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: false,
      },
    });

    expect(stabilizeAcpSessionTimingForDisplay(current, next)?.timing?.sessionElapsedSeconds).toBe(2);
  });

  it('routes live timing patches through the same revision guard', () => {
    const current = session({
      timing: {
        sessionElapsedSeconds: 83,
        revision: 340,
        observedAt: '1782987741Z',
        activeTurnStartedAt: '1782987649Z',
        activeTurnLastActivityAt: '1782987741Z',
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: false,
      },
    });
    const staleLiveTiming = {
      sessionElapsedSeconds: 68,
      revision: 333,
      observedAt: '1782987715Z',
      activeTurnStartedAt: '1782987649Z',
      activeTurnLastActivityAt: '1782987715Z',
      permissionWaitStartedAt: null,
      userWaitStartedAt: null,
      waitReason: null,
      paused: false,
    };

    const stabilized = stabilizeAcpSessionTimingPatchForDisplay(current, staleLiveTiming);

    expect(stabilized?.timing?.sessionElapsedSeconds).toBe(83);
    expect(stabilized?.timing?.revision).toBe(340);
  });

  it('keeps ready session metadata when a later same-session payload is partial', () => {
    const readyPrompt = event({
      id: 'gold-band-user-prompt-1',
      seq: 1,
      kind: 'userTextDelta',
      content: 'Build this',
      raw: { source: 'goldBandPrompt' },
    });
    const ready = session({
      sessionId: 'session-1',
      systemPromptAppend: 'System instructions',
      config: {
        currentModelId: 'gpt-5',
        currentModeId: 'default',
        configOptions: [
          { category: 'model', options: [{ value: 'gpt-5', name: 'GPT-5' }] },
          { category: 'mode', options: [{ value: 'default', name: 'Default' }] },
        ],
      },
      events: [readyPrompt],
    });
    const partial = session({
      sessionId: 'session-1',
      timing: {
        sessionElapsedSeconds: 12,
        revision: 12,
        observedAt: '12Z',
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: false,
      },
      events: [event({ id: 'assistant-message-2', seq: 2, kind: 'textDelta', content: 'Working' })],
    });

    const reconciled = reconcileAcpSessionForDisplay(ready, partial);

    expect(reconciled?.systemPromptAppend).toBe('System instructions');
    expect(reconciled?.config?.configOptions).toHaveLength(2);
    expect(reconciled?.events.map((item) => item.id)).toEqual([
      'gold-band-user-prompt-1',
      'assistant-message-2',
    ]);
    expect(reconciled?.timing?.sessionElapsedSeconds).toBe(12);
  });

  it('does not carry ready metadata into a different ACP session', () => {
    const ready = session({
      sessionId: 'session-1',
      systemPromptAppend: 'System instructions',
      config: {
        currentModelId: 'gpt-5',
        currentModeId: 'default',
      },
      events: [
        event({
          id: 'gold-band-user-prompt-1',
          seq: 1,
          kind: 'userTextDelta',
          raw: { source: 'goldBandPrompt' },
        }),
      ],
    });
    const nextSession = session({
      sessionId: 'session-2',
      events: [],
    });

    const reconciled = reconcileAcpSessionForDisplay(ready, nextSession);

    expect(reconciled?.systemPromptAppend).toBeUndefined();
    expect(reconciled?.config).toBeUndefined();
    expect(reconciled?.events).toEqual([]);
  });

  it('does not carry timing across different ACP sessions', () => {
    const previous = session({
      sessionId: 'session-1',
      sessionElapsedSeconds: 61,
      timing: {
        sessionElapsedSeconds: 61,
        activeTurnStartedAt: null,
        activeTurnLastActivityAt: null,
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: true,
      },
    });
    const next = session({
      sessionId: 'session-2',
      sessionElapsedSeconds: 3,
      timing: {
        sessionElapsedSeconds: 3,
        activeTurnStartedAt: '1Z',
        activeTurnLastActivityAt: '4Z',
        permissionWaitStartedAt: null,
        userWaitStartedAt: null,
        waitReason: null,
        paused: false,
      },
    });

    expect(stabilizeAcpSessionTimingForDisplay(previous, next)?.timing?.sessionElapsedSeconds).toBe(3);
  });

  it('deduplicates repeated Gold Band user prompt snapshots with the same prompt id', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'gold-band-user-prompt-71',
        seq: 71,
        timestamp: '1782356175Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'acp-prompt-1' },
      }),
      event({
        id: 'gold-band-user-prompt-71-copy',
        seq: 72,
        timestamp: '1782356176Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'acp-prompt-1' },
      }),
    ]);

    expect(timeline).toHaveLength(1);
  });

  it('folds retry attempts by prompt identity instead of matching their text', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'gold-band-user-prompt-1', seq: 1, kind: 'userTextDelta',
        content: 'first payload', status: 'completed',
        raw: { source: 'goldBandPrompt', promptId: 'turn-1' },
      }),
      event({
        id: 'gold-band-user-prompt-2', seq: 2, kind: 'userTextDelta',
        content: 'payload normalized after reconnect', status: 'failed',
        raw: {
          source: 'goldBandPrompt', promptId: 'turn-1',
          retry: { attempt: 3, maxAttempts: 3 },
          terminalFailure: { code: 'provider.server-unavailable' },
        },
      }),
    ]);

    expect(timeline).toHaveLength(1);
    expect(timeline[0]).toMatchObject({
      content: 'first payload',
      status: 'failed',
      raw: { retry: { attempt: 3, maxAttempts: 3 } },
    });
  });

  it('keeps a stopped retry result bound to its own prompt turn', () => {
    const stopped = event({
      id: 'gold-band-user-prompt-1',
      kind: 'userTextDelta',
      status: 'cancelled',
      raw: { promptId: 'turn-1', retry: { attempt: 2, maxAttempts: 3 } },
    });
    const nextTurn = event({
      id: 'gold-band-user-prompt-2',
      seq: 2,
      kind: 'userTextDelta',
      status: 'processing',
      raw: { promptId: 'turn-2' },
    });

    expect(promptRetryFooterKind(stopped)).toBe('cancelled');
    expect(promptRetryFooterKind(nextTurn)).toBeNull();
  });

  it('keeps repeated external provider-history prompts as separate turns', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'provider-history-user-session-1-2',
        seq: 20,
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: {
          source: 'providerHistory',
          historyOrigin: 'external',
          sessionUpdate: 'user_message_chunk',
          historyTurnIndex: 2,
        },
      }),
      event({
        id: 'provider-history-user-session-1-3',
        seq: 30,
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: {
          source: 'providerHistory',
          historyOrigin: 'external',
          sessionUpdate: 'user_message_chunk',
          historyTurnIndex: 3,
        },
      }),
    ]);

    expect(timeline).toHaveLength(2);
    expect(timeline.map((item) => item.id)).toEqual([
      'provider-history-user-session-1-2',
      'provider-history-user-session-1-3',
    ]);
  });

  it('removes only pending optimistic prompts after a successful stop', () => {
    const settled = clearPendingOptimisticPromptsAfterStop([
      event({
        id: 'optimistic-pending',
        kind: 'userTextDelta',
        status: 'sending',
        content: 'not accepted',
        raw: { source: 'goldBandPrompt', optimistic: true, promptId: 'prompt-1' },
      }),
      event({
        id: 'optimistic-completed',
        kind: 'userTextDelta',
        status: 'completed',
        content: 'accepted locally',
        raw: { source: 'goldBandPrompt', optimistic: true, promptId: 'prompt-2' },
      }),
      event({
        id: 'gold-band-user-prompt-3',
        kind: 'userTextDelta',
        status: 'completed',
        content: 'accepted durably',
        raw: { source: 'goldBandPrompt', promptId: 'prompt-3' },
      }),
    ]);

    expect(settled.map((item) => item.id)).toEqual([
      'optimistic-completed',
      'gold-band-user-prompt-3',
    ]);
  });

  it('keeps historical Gold Band prompts without prompt ids as separate turns', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'gold-band-user-prompt-712',
        seq: 712,
        timestamp: '1782359019Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt' },
      }),
      event({
        id: 'assistant-thought-894',
        seq: 894,
        timestamp: '1782359024Z',
        kind: 'thoughtDelta',
        content: 'first resumed thought',
      }),
      event({
        id: 'gold-band-user-prompt-896',
        seq: 896,
        timestamp: '1782359028Z',
        kind: 'userTextDelta',
        content: '继续',
        status: 'completed',
        raw: { source: 'goldBandPrompt' },
      }),
      event({
        id: 'assistant-thought-901',
        seq: 901,
        timestamp: '1782359029Z',
        kind: 'thoughtDelta',
        content: 'second resumed thought',
      }),
    ]);

    expect(timeline).toHaveLength(4);
    expect(timeline.map((item) => item.kind === 'activityBatch' ? item.events[0]?.content : item.content)).toEqual([
      '继续',
      'first resumed thought',
      '继续',
      'second resumed thought',
    ]);
  });

  it('keeps top-level plan updates out of duplicate timeline rows', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'session-plan-1',
        seq: 1,
        kind: 'plan',
        content: 'draft',
        raw: { entries: [{ content: 'Step 1', status: 'in_progress' }] },
      }),
      event({
        id: 'session-plan-1',
        seq: 2,
        kind: 'plan',
        content: 'draft updated',
        raw: { entries: [{ content: 'Step 1', status: 'completed' }] },
      }),
    ]);

    expect(timeline).toHaveLength(0);
  });

  it('does not let older shorter text stream updates replace complete live content', () => {
    const merged = mergeAcpEvents(
      [
        event({
          id: 'assistant-message-m1',
          seq: 10,
          kind: 'textDelta',
          content: '我先建立验收清单并读取当前节点可见的报告文件。',
          endedSeq: 10,
        }),
      ],
      [
        event({
          id: 'assistant-message-m1',
          seq: 9,
          kind: 'textDelta',
          content: '我先建立验收清单',
          endedSeq: 9,
        }),
      ],
    );

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({
      seq: 10,
      content: '我先建立验收清单并读取当前节点可见的报告文件。',
    });
  });

  it('keeps context compaction as one typed timeline item across lifecycle revisions', () => {
    const started = event({
      id: 'context-compaction-10',
      seq: 10,
      kind: 'contextCompaction',
      status: 'running',
      startedSeq: 10,
      startedAt: '100Z',
      raw: {
        contextCompaction: {
          phase: 'started',
          contextUsedBefore: 169_052,
          contextSize: 200_000,
        },
      },
    });
    const completed = event({
      id: 'context-compaction-10',
      seq: 20,
      kind: 'contextCompaction',
      status: 'completed',
      startedSeq: 10,
      endedSeq: 20,
      startedAt: '100Z',
      endedAt: '420Z',
      raw: {
        contextCompaction: {
          phase: 'completed',
          contextUsedBefore: 169_052,
          contextSize: 200_000,
          contextUsedAfter: 23_825,
        },
      },
    });

    const timeline = buildAcpTimeline(mergeAcpEvents([started], [completed]));

    expect(timeline).toHaveLength(1);
    expect(timeline[0]).toMatchObject({
      id: 'context-compaction-10',
      kind: 'contextCompaction',
      status: 'completed',
      startedSeq: 10,
      endedSeq: 20,
      startedAt: '100Z',
      endedAt: '420Z',
    });
    expect(contextCompactionUsageBefore(timeline[0])).toEqual({
      used: '169.1K',
      size: '200.0K',
    });
  });

  it('does not expose the adapter-derived post-compaction usage in the UI display contract', () => {
    const completed = event({
      kind: 'contextCompaction',
      status: 'completed',
      raw: {
        contextCompaction: {
          phase: 'completed',
          contextUsedBefore: 47_583,
          contextSize: 1_000_000,
          contextUsedAfter: 41_462,
        },
      },
    });

    expect(contextCompactionUsageBefore(completed)).toEqual({
      used: '47.6K',
      size: '1.0M',
    });
  });

  it('does not let older shorter thought stream updates replace complete live content', () => {
    const merged = mergeAcpEvents(
      [
        event({
          id: 'assistant-thought-m1',
          seq: 10,
          kind: 'thoughtDelta',
          content: 'carefully and avoid vague references.',
          endedSeq: 10,
        }),
      ],
      [
        event({
          id: 'assistant-thought-m1',
          seq: 9,
          kind: 'thoughtDelta',
          content: 'carefully and',
          endedSeq: 9,
        }),
      ],
    );

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({
      seq: 10,
      content: 'carefully and avoid vague references.',
    });
  });

  it('keeps text and thought content when empty stream frames arrive in the timeline builder', () => {
    const timeline = buildAcpTimeline([
      event({
        id: 'assistant-message-m1',
        seq: 1,
        kind: 'textDelta',
        content: 'hello world',
        endedSeq: 1,
      }),
      event({
        id: 'assistant-message-m1',
        seq: 2,
        kind: 'textDelta',
        content: '',
        endedSeq: 2,
      }),
      event({
        id: 'assistant-thought-m1',
        seq: 3,
        kind: 'thoughtDelta',
        content: 'thinking done',
        endedSeq: 3,
      }),
      event({
        id: 'assistant-thought-m1',
        seq: 4,
        kind: 'thoughtDelta',
        content: '',
        endedSeq: 4,
      }),
    ]);

    expect(timeline).toHaveLength(2);
    expect(timeline[0]).toMatchObject({ kind: 'textDelta', content: 'hello world' });
    expect(timeline[1]).toMatchObject({ kind: 'activityBatch' });
    if (timeline[1]?.kind !== 'activityBatch') throw new Error('missing activity batch');
    expect(timeline[1].events[0]).toMatchObject({ kind: 'thoughtDelta', content: 'thinking done' });
  });

  it('replaces existing permission events during live/session merge', () => {
    const merged = mergeAcpEvents(
      [
        event({
          id: 'permission-0',
          seq: 10,
          kind: 'permissionRequest',
          status: 'pending',
          raw: { requestId: '0' },
        }),
      ],
      [
        event({
          id: 'permission-permission-0',
          seq: 11,
          kind: 'permissionRequest',
          status: 'selected',
          raw: { requestId: 'permission-0', optionId: 'allow' },
        }),
      ],
    );

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ status: 'selected' });
  });

  it('replaces pending permission when terminal update omits session id', () => {
    const merged = mergeAcpEvents(
      [
        event({
          id: 'permission-5',
          seq: 762,
          kind: 'permissionRequest',
          sessionId: 'session-live',
          status: 'pending',
          raw: { requestId: '5' },
        }),
      ],
      [
        event({
          id: 'permission-5',
          seq: 920,
          kind: 'permissionRequest',
          sessionId: null,
          status: 'cancelled',
          raw: { requestId: '5', cancelled: true },
        }),
      ],
    );

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ status: 'cancelled' });
    expect(pendingPermissionFromEvents(merged, new Set())).toBeNull();
  });

  it('calculates session elapsed from active prompt turns without idle resume gaps', () => {
    const events = [
      event({
        id: 'gold-band-user-prompt-3',
        seq: 3,
        timestamp: '1782903916Z',
        kind: 'userTextDelta',
        status: 'completed',
        raw: { source: 'goldBandPrompt' },
      }),
      event({ id: 'assistant-message-4', seq: 4, timestamp: '1782903917Z', kind: 'textDelta' }),
      event({
        id: 'acp-event-6',
        seq: 6,
        timestamp: '1782904743Z',
        kind: 'modeUpdate',
        raw: { sessionUpdate: 'current_mode_update' },
      }),
      event({
        id: 'gold-band-user-prompt-7',
        seq: 7,
        timestamp: '1782904743Z',
        kind: 'userTextDelta',
        status: 'completed',
        raw: { source: 'goldBandPrompt' },
      }),
      event({ id: 'assistant-message-8', seq: 8, timestamp: '1782904746Z', kind: 'textDelta' }),
      event({
        id: 'acp-event-10',
        seq: 10,
        timestamp: '1782905348Z',
        kind: 'modeUpdate',
        raw: { sessionUpdate: 'current_mode_update' },
      }),
      event({
        id: 'gold-band-user-prompt-11',
        seq: 11,
        timestamp: '1782905348Z',
        kind: 'userTextDelta',
        status: 'completed',
        raw: { source: 'goldBandPrompt' },
      }),
      event({ id: 'assistant-message-12', seq: 12, timestamp: '1782905355Z', kind: 'textDelta' }),
    ];

    expect(calculateSessionElapsedSeconds(events, 'failed')).toBe(11);
  });
});
