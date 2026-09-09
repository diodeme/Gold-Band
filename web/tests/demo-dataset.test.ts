import { describe, expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { createDatasetReader, REAL_PROJECT_ID } from '../../marketing/demo/dataset';
import { createDemoApi } from '../../marketing/demo/api';
import { demoHashForPage, demoPageFromHash, demoLinkParameters } from '../../marketing/demo/routes';
import { entryPreferences } from '../../marketing/demo/entry';

describe('demo entry and immutable source', () => {
  it('round-trips complete navigation identity', () => {
    const page = { kind: 'conversation-run' as const, projectId: REAL_PROJECT_ID, taskId: 'task-015', runId: 'run-001' };
    const leaf = { roundId: 'round-001', nodeId: 'nested', attemptId: 'attempt-002', outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001', pathLabel: '' };
    const hash = demoHashForPage(page, leaf);
    expect(demoPageFromHash(hash)).toEqual(page);
    expect(Object.fromEntries(demoLinkParameters(hash))).toMatchObject({ project: REAL_PROJECT_ID, attempt: 'attempt-002', outerNode: 'ai-dynamic', outerAttempt: 'attempt-001' });
  });
  it('applies only supported incoming preferences', async () => {
    const preferences = (await createDemoApi().getAppBootstrap()).preferences;
    expect(entryPreferences(preferences, '?language=en&theme=light')).toMatchObject({ language: 'en', appearance: { colorScheme: 'light' } });
    expect(entryPreferences(preferences, '?language=bad&theme=bad')).toEqual(preferences);
  });
});

describe.skipIf(!process.env.DEMO_DATASET_PATH)('exported source reader', () => {
  const root = process.env.DEMO_DATASET_PATH!;
  const requested: string[] = [];
  const reader = createDatasetReader('/data/', async (input) => {
    const path = String(input).replace('/data/', ''); requested.push(path);
    return new Response(await readFile(resolve(root, path)));
  });
  it('keeps actual run state and all 58 nodes', async () => {
    const data = await reader.dataset();
    expect(data.source).toMatchObject({ revision: 118, status: 'paused', outcome: null, pauseReason: 'process-interrupted' });
    const run = await reader.api.getConversationRun(data.projectId, data.taskId, data.runId);
    expect(run.workflowGraph.nodes).toHaveLength(58);
    expect(run.selectedSession).toBeNull();
    expect(requested).toHaveLength(2);
  });
  it('keeps the resource catalog lightweight instead of loading every filename', async () => {
    const data = await reader.dataset();
    expect(data.resources!.byteLength).toBeLessThan(64 * 1024);
  });
  it('covers every root/child history exactly once across cursor boundaries', async () => {
    const data = await reader.dataset();
    for (const ref of data.sessions) {
      let cursor: string | undefined;
      const seen = new Set<string>(); let pages = 0;
      while (true) {
        const page = (await reader.api.getAcpSession(data.projectId, data.taskId, data.runId, ref.roundId, ref.nodeId, ref.attemptId, { branchId: ref.branchId, beforeCursor: cursor, pageSize: 50 }, undefined, ref.outerNodeId, ref.outerAttemptId))!;
        expect(page.readOnly).toBe(true);
        expect(page.sessionId).toBe(ref.sessionId);
        for (const event of page.events) { expect(seen.has(event.id)).toBe(false); seen.add(event.id); }
        if (!page.eventPage.hasOlder) break;
        expect(page.eventPage.oldestCursor).not.toBe(cursor);
        cursor = page.eventPage.oldestCursor!;
        if (++pages > 10000) throw new Error('Unbounded pagination');
      }
      const index = JSON.parse(await readFile(resolve(root, ref.path), 'utf8'));
      const expected = index.blocks.flatMap((block: { summary?: { id: string }; itemIds: string[] }) => block.summary ? [block.summary.id] : block.itemIds);
      expect([...seen].sort()).toEqual(expected.sort());
    }
  }, 120000);
  it('reads the selected attachment and diff through scoped immutable resources', async () => {
    const data = await reader.dataset();
    const ref = data.sessions.find((item) => item.nodeId === 'phase-2-parallel-capabilities-merge-2' && item.branchId === 'root')!;
    const locator = { ...ref, projectId: data.projectId, taskId: data.taskId, runId: data.runId };
    const resources = JSON.parse(await readFile(resolve(root, ref.path.replace('index.json', 'resources.json')), 'utf8'));
    const id = Object.keys(resources.changes)[0];
    const changes = await reader.api.getTurnFileChangeSet(locator, id);
    const resolved = await reader.api.resolveTurnAttachmentFile(locator, id, changes.attachments[0].id);
    const file = await reader.api.readFileResource(data.projectId, resolved.locator.canonicalPath);
    expect(file).toMatchObject({ kind: 'text', editable: false });
    const diff = await reader.api.getFileComparison(locator, id, changes.changes[0].id);
    expect(diff.changeId).toBe(changes.changes[0].id);
    expect(diff.after?.content).toBeTruthy();
    await expect(reader.api.getFileComparison({ ...locator, branchId: 'wrong' }, id, changes.changes[0].id)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(reader.api.readFileResource('wrong', resolved.locator.canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(reader.api.readConversationDirectoryFile({ ...locator, relativePath: '../run.json', kind: 'attachments' } as any)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('rejects wrong outer/branch and returns the requested tool rather than the first', async () => {
    const data = await reader.dataset(); const ref = data.sessions[0];
    const args = [data.projectId, data.taskId, data.runId, ref.roundId, ref.nodeId, ref.attemptId] as const;
    await expect(reader.api.getAcpSession(...args, undefined, undefined, 'wrong', ref.outerAttemptId)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(reader.api.getAcpSession(...args, { branchId: 'wrong' }, undefined, ref.outerNodeId, ref.outerAttemptId)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    const index = JSON.parse(await readFile(resolve(root, ref.path), 'utf8'));
    const tools = Object.values(index.eventRefs).filter((event: any) => event.kind === 'toolCall') as any[];
    for (const tool of tools.slice(-2)) {
      const result = await reader.api.getAcpToolDetail(...args, { branchId: ref.branchId, sessionId: ref.sessionId, eventId: tool.id, toolCallId: tool.toolCallId }, ref.outerNodeId, ref.outerAttemptId);
      expect(result.event?.id).toBe(tool.id);
    }
  });
});
