import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRecordingApi } from '../../marketing/recording/runtime';

describe('after recording source', () => {
  afterEach(() => vi.unstubAllGlobals());
  it('uses the recording repository in the branch picker', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => ({ result: { id: 'test', head: 'actual-head', revision: 'actual-revision', changes: [{ path: 'src/config.json' }] } }) })));
    const api = createRecordingApi({ scene: 'after', language: 'en' });
    const picker = await api.getGitBranchPickerSnapshot('default');
    expect(picker.currentBranch).toBe('review/workspace');
    expect(picker.headOid).toBe('actual-head');
    expect(picker.dirtyFileCount).toBe(1);
  });
  it('continues the same completed workflow with a coherent graph and file session', async () => {
    const api = createRecordingApi({ scene: 'after', language: 'en' });
    const run = await api.getConversationRun('default', 'mock-task', 'run-052');
    expect(run.workflowGraph.nodes.map(node => node.id)).toEqual(['review', 'repair', 'continue']);
    expect(run.workflowGraph.nodes.map(node => node.outcome)).toEqual(['failure', 'success', 'success']);
    expect(run.runStatus).toBe('completed');
    expect(run.runOutcome).toBe('success');
    expect(run.activeSessions).toEqual([]);
    expect(run.selectedSession?.nodeId).toBe('continue');
    expect(run.selectedSession?.events.some(event => event.kind === 'fileChangeSet')).toBe(true);
    for (const node of run.sessionTree.rounds[0].nodes) {
      const session = await api.getAcpSession('default', 'mock-task', 'run-052', 'round-001', node.nodeId, 'attempt-001');
      expect(session?.nodeId).toBe(node.nodeId);
      expect(session?.status).toBe('completed');
    }
    await expect(api.getAcpSession('default', 'mock-task', 'run-052', 'round-001', 'continue', 'attempt-001', undefined, undefined, 'foreign', 'attempt-001')).rejects.toMatchObject({ code: 'recording.after.locator-invalid' });
  });
});
