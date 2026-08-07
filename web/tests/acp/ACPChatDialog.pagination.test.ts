import { describe, expect, it } from 'vitest';
import {
  acpPaginationSeqBounds,
  createVisibleAcpSession,
  limitAcpEvents,
  loadedEventBufferLimit,
  mergeAcpEvents,
  reconcileAcpEventPageForUpdate,
  resolveAcpHasOlderEvents,
} from '../../src/components/acp/ACPChatDialog';
import { normalizeAcpEventForAttempt } from '../../src/lib/acp-event-normalization';
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
    expect(loadedEventBufferLimit(360)).toBe(1080);
    expect(loadedEventBufferLimit(30)).toBe(90);
    expect(loadedEventBufferLimit(10)).toBe(30);
    expect(loadedEventBufferLimit(2000)).toBe(2000);
  });

  it('keeps the current page when the next page is merged', () => {
    const current = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `current-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `current ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `newer-${index + 361}`,
        seq: index + 361,
        timestamp: `${index + 361}Z`,
        kind: 'textDelta',
        content: `newer ${index + 361}`,
      }),
    );

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(360),
    );

    expect(merged).toHaveLength(720);
    expect(merged[0]!.id).toBe('current-1');
    expect(merged[359]!.id).toBe('current-360');
    expect(merged[360]!.id).toBe('newer-361');
    expect(merged[719]!.id).toBe('newer-720');
  });

  it('slides a full three-page window without breaking the page boundary', () => {
    const current = Array.from({ length: 1080 }, (_, index) =>
      event({
        id: `event-${index + 1}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'textDelta',
        content: `event ${index + 1}`,
      }),
    );
    const newer = Array.from({ length: 360 }, (_, index) =>
      event({
        id: `event-${index + 1081}`,
        seq: index + 1081,
        timestamp: `${index + 1081}Z`,
        kind: 'textDelta',
        content: `event ${index + 1081}`,
      }),
    );

    const merged = limitAcpEvents(
      mergeAcpEvents(current, newer),
      'start',
      loadedEventBufferLimit(360),
    );

    expect(merged).toHaveLength(1080);
    expect(merged[0]!.seq).toBe(361);
    expect(merged[719]!.seq).toBe(1080);
    expect(merged[720]!.seq).toBe(1081);
    expect(merged[1079]!.seq).toBe(1440);
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
      pendingPermissions: [],
      pendingElicitations: [],
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
