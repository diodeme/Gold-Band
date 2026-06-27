import { describe, expect, it } from 'vitest';
import {
  buildAcpTimeline,
  mergeAcpEvents,
  pendingElicitationFromEvents,
  pendingPermissionFromEvents,
} from '../src/components/acp/ACPChatDialog';
import type { AcpUiEventVm } from '../src/types';

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
  };
}

describe('ACP chat event handling', () => {
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
    expect(timeline[0]).toMatchObject({
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
    expect(timeline[1]).toMatchObject({ kind: 'thoughtDelta', content: 'thinking done' });
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
    expect(timeline.map((item) => 'content' in item ? item.content : null)).toEqual([
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
    expect(timeline[1]).toMatchObject({ kind: 'thoughtDelta', content: 'thinking done' });
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
});
