import { describe, expect, it } from 'vitest';
import { archiveSessionVm, archiveTimestamp } from '../../marketing/demo/archive-session';
import type { ArchiveSession } from '../../marketing/demo/archive';

const session: ArchiveSession = { projectId: 'p', taskId: 't', runId: 'r', roundId: 'round', nodeId: 'node', attemptId: 'a', outerNodeId: 'outer', outerAttemptId: 'outer-a',
  node: { id: 'node', title: 'Original node', status: 'paused', outcome: null, provider: 'codex-acp' }, detail: `assets/${'a'.repeat(64)}.json`, itemCount: 10, recordCount: 15 };
const diagnostics = { rawFrameCount: 27, eventCount: 8, errorCount: 2, lastError: 'Original error', lastErrorTimestamp: null };
const page = { diagnostics, workerRef: { provider: 'codex-acp' }, snapshot: { latestTurnStatus: 'completed', stopReason: 'end_turn', sessionId: 'source-session', systemPromptAppend: 'Original system context', createdAt: '1788431047Z' },
  branches: [{ id: 'root', count: 8 }, { id: 'agent-a', count: 2 }], events: [{ id: 'source-event', seq: 7, kind: 'textDelta', timestamp: '1788431047Z', content: 'Original body\nExact second line', raw: { preserved: true } }],
  eventPage: { loadedCount: 1, total: 8, oldestSeq: 7, newestSeq: 7, hasOlder: true, hasNewer: true, oldestCursor: '7', newestCursor: '7' } };

describe('archive client session projection', () => {
  it('normalizes persisted epoch timestamps without altering original body, sequence or locator', () => {
    const projected = archiveSessionVm(session, 'root', page);
    expect(projected.diagnostics).toEqual(diagnostics);
    expect(projected.events[0]).toMatchObject({ id: 'source-event', seq: 7, content: page.events[0].content, raw: { preserved: true }, timestamp: new Date(1788431047000).toISOString() });
    expect(projected).toMatchObject({ readOnly: true, outerNodeId: 'outer', outerAttemptId: 'outer-a', nodeId: 'node', attemptId: 'a' });
    expect(projected.eventPage).toEqual(page.eventPage);
    expect(page.events[0].timestamp).toBe('1788431047Z');
  });
  it('keeps ACP turn status distinct from paused node runtime and does not fabricate a child terminal outcome', () => {
    expect(archiveSessionVm(session, 'root', page).status).toBe('completed');
    expect(session.node.status).toBe('paused');
    const child = archiveSessionVm(session, 'agent-a', page);
    expect(child.status).toBe('unknown');
    expect(child.stopReason).toBeNull();
    expect(child.systemPromptAppend).toBeNull();
    expect(child.config).toBeNull();
    expect(child.sessionId).toBe('source-session');
    expect(child.branchId).toBe('agent-a');
    expect(child.sessionStartedAt).toBeNull();
    expect(child.sessionUpdatedAt).toBeNull();
    expect(child.timelineProjection).toBeNull();
  });
  it('retains valid ISO timestamps and handles missing or invalid source times explicitly', () => {
    expect(archiveTimestamp('2026-09-01T00:00:00Z')).toBe('2026-09-01T00:00:00.000Z');
    expect(archiveTimestamp('invalid')).toBeNull();
    expect(archiveTimestamp(undefined)).toBeNull();
  });
  it('rejects missing diagnostic evidence instead of treating timeline records as raw frames', () => {
    expect(() => archiveSessionVm(session, 'root', { ...page, diagnostics: undefined } as unknown as typeof page)).toThrow('demo.archive-diagnostics-unavailable');
  });
});
