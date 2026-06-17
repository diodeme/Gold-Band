import { describe, expect, it } from 'vitest';
import {
  buildAcpTimeline,
  mergeAcpEvents,
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
