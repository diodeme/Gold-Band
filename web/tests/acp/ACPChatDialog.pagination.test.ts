import { describe, expect, it } from 'vitest';
import {
  ACP_RAW_SESSION_KIND_I18N_KEYS,
  acpPaginationSeqBounds,
  createVisibleAcpSession,
  limitAcpEvents,
  loadedEventBufferLimit,
  mergeAcpEvents,
  reconcileAcpEventPageForUpdate,
  resolveAcpHasOlderEvents,
} from '../../src/components/acp/ACPChatDialog';
import { normalizeAcpEventForAttempt } from '../../src/lib/acp-event-normalization';
import {
  DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE,
  DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT,
} from '../../src/lib/acp-chat-pagination';
import type { AcpSessionVm, AcpUiEventVm } from '../../src/types';

function event(
  partial: Partial<AcpUiEventVm> &
    Pick<AcpUiEventVm, 'id' | 'seq' | 'timestamp' | 'kind'>,
): AcpUiEventVm {
  return {
    id: partial.id,
    seq: partial.seq,
    timestamp: partial.timestamp,
    kind: partial.kind,
    ...partial,
  } as AcpUiEventVm;
}

describe('ACPChatDialog pagination buffer', () => {
  it('does not turn an after-seq delta into a client-side history gap', () => {
    const current = {
      loadedCount: 4,
      total: 4,
      oldestSeq: 3,
      newestSeq: 104,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: 'seq:3',
      newestCursor: 'seq:104',
    };
    const delta = {
      loadedCount: 2,
      total: 6,
      oldestSeq: 109,
      newestSeq: 188,
      hasOlder: true,
      hasNewer: false,
      oldestCursor: 'seq:109',
      newestCursor: 'seq:188',
    };

    expect(reconcileAcpEventPageForUpdate(current, delta, 'append-newer')).toEqual({
      ...delta,
      loadedCount: 6,
      oldestSeq: 3,
      oldestCursor: 'seq:3',
      hasOlder: false,
    });
  });

  it('preserves a real older-page gap while appending live deltas', () => {
    const current = {
      loadedCount: 200,
      total: 240,
      oldestSeq: 41,
      newestSeq: 240,
      hasOlder: true,
      hasNewer: false,
      oldestCursor: 'seq:41',
      newestCursor: 'seq:240',
    };
    const delta = {
      loadedCount: 1,
      total: 241,
      oldestSeq: 241,
      newestSeq: 241,
      hasOlder: true,
      hasNewer: false,
      oldestCursor: 'seq:241',
      newestCursor: 'seq:241',
    };

    const merged = reconcileAcpEventPageForUpdate(current, delta, 'append-newer');
    expect(merged.hasOlder).toBe(true);
    expect(merged.oldestSeq).toBe(41);
    expect(merged.newestSeq).toBe(241);
  });

  it('exposes distinct raw-frame labels for resume and load lifecycle calls', () => {
    expect(ACP_RAW_SESSION_KIND_I18N_KEYS).toMatchObject({
      'session/resume': 'acp.rawKindSessionResume',
      'session/load': 'acp.rawKindSessionLoad',
    });
  });

  it('lets an authoritative session snapshot clear stale history availability', () => {
    expect(resolveAcpHasOlderEvents(false, 2, 2)).toBe(false);
  });

  it('keeps history available when the local event buffer actually truncates events', () => {
    expect(resolveAcpHasOlderEvents(false, 91, 90)).toBe(true);
  });

  it('derives continuation cursors only from the active attempt window', () => {
    const previous = normalizeAcpEventForAttempt(
      event({ id: 'previous', seq: 1, timestamp: '1Z', kind: 'textDelta' }),
      'attempt-001',
      1,
    );
    const activeOldest = normalizeAcpEventForAttempt(
      event({ id: 'active-oldest', seq: 40, timestamp: '2Z', kind: 'textDelta' }),
      'attempt-002',
      2,
    );
    const activeNewest = normalizeAcpEventForAttempt(
      event({ id: 'active-newest', seq: 80, timestamp: '3Z', kind: 'textDelta' }),
      'attempt-002',
      3,
    );

    expect(acpPaginationSeqBounds(
      [previous, activeOldest, activeNewest],
      'attempt-002',
    )).toEqual({ oldestSeq: 40, newestSeq: 80 });
  });

  it('keeps three configured pages in the sliding event buffer', () => {
    expect(DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE).toBe(192);
    expect(DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT).toBe(576);
    expect(loadedEventBufferLimit(DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE)).toBe(576);
    expect(loadedEventBufferLimit(240)).toBe(720);
    expect(loadedEventBufferLimit(30)).toBe(90);
    expect(loadedEventBufferLimit(10)).toBe(30);
    expect(loadedEventBufferLimit(2000)).toBe(2000);
  });

  it('keeps the current page when the next page is merged', () => {
    const pageSize = DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE;
    const current = Array.from({ length: pageSize }, (_, index) =>
      event({
        id: `current-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `current ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: pageSize }, (_, index) => {
      const seq = index + pageSize + 1;
      return event({
        id: `newer-${seq}`,
        seq,
        timestamp: `${seq}Z`,
        kind: 'textDelta',
        content: `newer ${seq}`,
      });
    });

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE),
    );

    expect(merged).toHaveLength(pageSize * 2);
    expect(merged[0]!.id).toBe('current-1');
    expect(merged[pageSize - 1]!.id).toBe(`current-${pageSize}`);
    expect(merged[pageSize]!.id).toBe(`newer-${pageSize + 1}`);
    expect(merged[pageSize * 2 - 1]!.id).toBe(`newer-${pageSize * 2}`);
  });

  it('slides a full three-page window without breaking the page boundary', () => {
    const pageSize = DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE;
    const windowSize = DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT;
    const current = Array.from({ length: windowSize }, (_, index) =>
      event({
        id: `event-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `event ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: pageSize }, (_, index) => {
      const seq = index + windowSize + 1;
      return event({
        id: `event-${seq}`,
        seq,
        timestamp: `${seq}Z`,
        kind: 'textDelta',
        content: `event ${seq}`,
      });
    });

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE),
    );

    expect(merged).toHaveLength(windowSize);
    expect(merged[0]!.seq).toBe(pageSize + 1);
    expect(merged[windowSize - pageSize - 1]!.seq).toBe(windowSize);
    expect(merged[windowSize - pageSize]!.seq).toBe(windowSize + 1);
    expect(merged[windowSize - 1]!.seq).toBe(windowSize + pageSize);
  });

  it('does not turn live activity audit overflow into conversation history', () => {
    const rootMessage = event({ id: 'user', seq: 1, timestamp: '1Z', kind: 'userTextDelta', content: 'delegate' });
    const session = {
      branchId: 'root',
      parentBranchId: null,
      readOnly: false,
      sessionId: 'session-1',
      provider: 'acp',
      status: 'running',
      restored: false,
      events: [rootMessage],
      timelineProjection: null,
      eventPage: {
        loadedCount: 1,
        total: 1,
        oldestSeq: 1,
        newestSeq: 1,
        hasOlder: false,
        hasNewer: false,
        oldestCursor: 'seq:1',
        newestCursor: 'seq:1',
      },
      pendingInteractions: [],
      diagnostics: { rawFrameCount: 0, eventCount: 1, errorCount: 0 },
    } as AcpSessionVm;
    const activity = Array.from({ length: 500 }, (_, index) => event({
      id: `tool-${index}`,
      seq: index + 2,
      timestamp: `${index + 2}Z`,
      kind: 'toolCall',
      toolCallId: `call-${index}`,
      status: 'completed',
    }));

    const visible = createVisibleAcpSession(session, activity, 90);

    expect(visible.events).toHaveLength(90);
    expect(visible.eventPage).toEqual(session.eventPage);
    expect(visible.eventPage.hasOlder).toBe(false);
    expect(visible.eventPage.total).toBe(1);
  });
});
