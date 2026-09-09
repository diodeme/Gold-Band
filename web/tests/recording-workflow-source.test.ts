import { describe, expect, it } from 'vitest';
import { createRecordingApi } from '../../marketing/recording/runtime';

describe('workflow recording source', () => {
  it('exposes the actual workflow instead of a direct preview with an empty graph', async () => {
    const api = createRecordingApi({ scene: 'during', language: 'zh' });
    const run = await api.getConversationRun('default', 'mock-task', 'run-052');
    expect(run.runMode).toBe('workflow');
    expect(run.selectedSession?.branchId).toBe('root');
    expect(run.workflowGraph.nodes.map(node => node.id)).toEqual(['review', 'repair', 'continue']);
    expect(JSON.parse(run.workflowJson!).nodes[0].success_condition.expression).toBe('$.result == true');
  });

  it('requires real replies and output before advancing the shared run and graph', async () => {
    const api = createRecordingApi({ scene: 'during', language: 'zh' });
    const advance = api.scenario!.advance;
    const locator = ['default', 'mock-task', 'run-052', 'round-001', 'review', 'attempt-001'] as const;
    await expect(advance('branch')).rejects.toMatchObject({ code: 'recording.output-required' });
    await advance('streaming'); await advance('streaming');
    await advance('question-wait');
    await expect(advance('permission-wait')).rejects.toMatchObject({ code: 'recording.interaction-pending' });
    await api.respondElicitation(...locator, 'snapshot-question', 'accept', { answer: '每轮保留' });
    await expect(api.respondElicitation(...locator, 'snapshot-question', 'accept', { answer: '迟到回答' })).rejects.toMatchObject({ code: 'recording.interaction-stale' });
    await advance('permission-wait');
    await api.respondAcpPermission(...locator, 'write-config', 'allow-once');
    await advance('result-false');
    const failed = await api.scenario!.snapshot();
    expect(failed.activeSessions[0].nodeId).toBe('review');
    expect(JSON.parse((await api.showArtifact(...locator, 'result.json')).content).result).toBe(false);
    await advance('branch');
    const repair = await api.scenario!.snapshot();
    expect(repair.selectedSession?.nodeId).toBe('repair');
    expect(repair.workflowGraph.nodes.find(node => node.current)?.id).toBe('repair');
    expect(repair.workflowGraph.nodes[0].outcome).toBe('failure');
    await expect(api.respondAcpPermission(...locator, 'write-config', 'allow-once')).rejects.toMatchObject({ code: 'recording.interaction-stale' });
    await advance('result-true');
    expect((await api.scenario!.snapshot()).activeSessions[0].nodeId).toBe('repair');
    await advance('branch');
    expect((await api.scenario!.snapshot()).activeSessions[0].nodeId).toBe('continue');
  });

  it('rejects a foreign outer locator even when the leaf IDs match', async () => {
    const api = createRecordingApi({ scene: 'during', language: 'zh' });
    await expect(api.getAcpSession('default', 'mock-task', 'run-052', 'round-001', 'dev', 'attempt-001', undefined, undefined, 'foreign', 'attempt-001'))
      .rejects.toMatchObject({ code: 'recording.locator-invalid' });
  });

  it('publishes the canonical task identity after the matching snapshot is readable', async () => {
    const api = createRecordingApi({ scene: 'during', language: 'zh' });
    const reads: Promise<void>[] = [];
    const initial = await api.scenario!.snapshot();
    const unsubscribe = await api.subscribeAcpSessionUpdates!(event => {
      expect(event.taskUuid).toBe(initial.taskUuid);
      expect(event.session?.branchId).toBe('root');
      reads.push(api.getAcpSession('default', event.taskId, event.runId, event.roundId, event.nodeId, event.attemptId)
        .then(session => { expect(session?.eventPage.coveredRevision).toBe(event.session?.eventPage.coveredRevision); expect(session?.events).toEqual(event.session?.events); }));
    });
    await api.scenario!.advance('streaming');
    await Promise.all(reads);
    const partial = await api.scenario!.snapshot();
    unsubscribe();
    await api.scenario!.advance('streaming');
    const complete = await api.scenario!.snapshot();
    expect(complete.selectedSession!.events[1].id).toBe(partial.selectedSession!.events[1].id);
    expect(complete.selectedSession!.events[1].content).toContain(partial.selectedSession!.events[1].content);
    expect(complete.selectedSession!.eventPage.coveredRevision).toBeGreaterThan(partial.selectedSession!.eventPage.coveredRevision!);
    await expect(api.getAcpSession('default','mock-task','run-052','round-001','review','attempt-001',{branchId:'foreign'})).rejects.toMatchObject({code:'recording.locator-invalid'});
  });
});
