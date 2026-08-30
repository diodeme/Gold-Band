import { describe, expect, it } from 'vitest';
import { browserApi } from '../../src/api/browser';

describe('browserApi', () => {
  it('keeps queued authoring payload out of lifecycle summaries and restores one item on demand', async () => {
    const run = await browserApi.getConversationRun('default', 'mock-task', 'run-053');
    const item = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0]?.lifecycle?.promptQueue?.items[0];
    expect(item).toMatchObject({
      id: 'browser-queued-1',
      attachmentCount: 1,
      quoteCount: 0,
    });
    expect(item).not.toHaveProperty('attachmentPaths');
    expect(item).not.toHaveProperty('quotes');

    const restored = await browserApi.restoreConversationQueuedPrompt(
      'default',
      'mock-task',
      'run-053',
      'round-001',
      'dev',
      'attempt-001',
      'browser-queued-1',
    );
    expect(restored.draft).toEqual({
      content: '完成当前修改后，补充对应的回归测试。',
      quotes: [],
      attachmentPaths: ['C:/browser/mock.png'],
    });
  });

  it('serves the authoritative hidden-prompt fixture for right-workspace deep-link verification', async () => {
    const run = await browserApi.getConversationRun('default', 'mock-task', 'run-052');
    const session = await browserApi.getAcpSession(
      'default',
      'mock-task',
      'run-052',
      'round-001',
      'dev',
      'attempt-001',
    );

    expect(run.taskUuid).toBe('browser-mock-task-uuid');
    expect(session?.events[0]?.content).toContain('Gold Band stable system prompt');
    expect(session?.events[0]?.content).toContain('Gold Band runtime context');
    expect(session?.systemPromptAppend).toContain('**system prompt**');
  });

  it('provides deterministic multi-page Git history for browser interaction regression', async () => {
    const first = await browserApi.getGitHistory('project-1', 'D:/repo', { limit: 300 });
    expect(first.commits).toHaveLength(300);
    expect(first.nextCursor).toBe('browser-history:300');

    const second = await browserApi.getGitHistory('project-1', 'D:/repo', {
      cursor: first.nextCursor,
      limit: 300,
      revision: first.revision,
    });
    expect(second.commits).toHaveLength(3);
    expect(second.nextCursor).toBeNull();
    expect(new Set([...first.commits, ...second.commits].map((commit) => commit.oid)).size).toBe(303);
  });

  it('exposes a branch picker snapshot and applies a preview branch switch', async () => {
    const snapshot = await browserApi.getGitBranchPickerSnapshot('project-1', 'D:/repo');
    expect(snapshot.currentBranch).toBe('feature/source-control');

    await expect(browserApi.changeGitBranch('project-1', 'D:/repo', {
      kind: 'switch',
      name: 'main',
      expectedRevision: snapshot.revision,
    })).resolves.toMatchObject({ currentBranch: 'main' });
  });

  it('keeps the current preview workspace in the recent list', async () => {
    const bootstrap = await browserApi.getAppBootstrap();

    await expect(browserApi.removeRecentWorkspace(bootstrap.repoRoot)).rejects.toEqual({
      code: 'workspace.recent-current-locked',
      params: { workspace: bootstrap.repoRoot },
    });
  });

  it('preserves the requested identity in preview analytics range reports', async () => {
    const range = { start: '2026-08-01', end: '2026-08-18' };

    await expect(browserApi.queryPersonalAnalyticsReport(range)).resolves.toMatchObject({ range });
    await expect(browserApi.startPersonalAnalyticsInsights('agent-a', range)).resolves.toMatchObject({
      agentType: 'agent-a',
      range,
      generation: 1,
      schemaVersion: '2.2.0',
      indexRevision: 6,
    });
  });

  it('keeps built-in profiles readonly in preview mode', async () => {
    const builtIn = (await browserApi.getProfiles()).profiles.find((profile) => profile.isBuiltIn);

    expect(builtIn).toBeDefined();
    await expect(browserApi.deleteProfile(builtIn!.id)).rejects.toEqual({
      code: 'profile.readonly-built-in',
      params: {},
    });
  });

  it('requires explicit force before deleting confirmation-gated preview profiles', async () => {
    const created = await browserApi.createProfile({
      name: 'Needs confirmation',
      summary: 'preview role [requires-confirmation]',
      content: 'temp',
      dynamicTemplate: true,
    });
    expect(created.scope).toBe('user');
    expect(created.dynamicTemplate).toBe(true);

    await expect(browserApi.deleteProfile(created.id)).rejects.toEqual({
      code: 'profile.delete-confirmation-required',
      params: {
        templateCount: 1,
        taskCount: 1,
        runCount: 0,
      },
    });

    const list = await browserApi.deleteProfile(created.id, true);
    expect(list.profiles.some((profile) => profile.id === created.id)).toBe(false);
  });

  it('enforces and rotates single-file grants for external preview files', async () => {
    const resolved = await browserApi.resolveWorkspaceFileLink('default', 'D:/outside/external.txt');
    const grant = resolved.externalAccessGrant!;
    const snapshot = await browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      grant.token,
    );
    expect(snapshot.kind).toBe('text');

    await expect(browserApi.writeFileResource({
      projectId: 'default',
      canonicalPath: resolved.locator.canonicalPath,
      externalAccessToken: null,
      content: 'denied',
      encoding: 'utf-8',
      lineEnding: 'lf',
      expectedRevision: snapshot.revision,
      operationId: 'denied-write',
      force: false,
    })).rejects.toMatchObject({ code: 'workspace-file.external-access-denied' });

    const renewed = await browserApi.renewExternalFileAccess(grant.token);
    expect(renewed.token).not.toBe(grant.token);
    await expect(browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      grant.token,
    )).rejects.toMatchObject({ code: 'workspace-file.external-access-denied' });
    await expect(browserApi.readFileResource(
      'default',
      resolved.locator.canonicalPath,
      renewed.token,
    )).resolves.toMatchObject({ kind: 'text' });
  });

  it('preserves the line target when resolving a conversation file link', async () => {
    const resolved = await browserApi.resolveWorkspaceFileLink('default', 'README.md:47');

    expect(resolved.locator.relativePath).toBe('README.md');
    expect(resolved.target).toEqual({ line: 47, column: null, endLine: null });
  });

  it('derives materialized attachment size from the decoded snapshot bytes', async () => {
    const [attachment] = await browserApi.materializeConversationAttachments([{
      name: 'active.log',
      mime: 'text/plain',
      dataBase64: 'AQIDBAU=',
    }]);

    expect(attachment).toMatchObject({ name: 'active.log', size: 5 });
  });

  it('returns the shared comparison contract for GitHub pull request files', async () => {
    const comparison = await browserApi.getGitComparison('default', {
      kind: 'github-pr',
      workspacePath: '/preview/gold-band',
      host: 'github.com',
      repository: 'gold-band/desktop',
      prNumber: 42,
      baseOid: '1111111111111111111111111111111111111111',
      headOid: '2222222222222222222222222222222222222222',
      path: 'src/source-control.ts',
    });

    expect(comparison).toMatchObject({
      path: 'src/source-control.ts',
      stats: { addedLines: 4, deletedLines: 1 },
      limitationCode: null,
    });
    expect(comparison.after?.content).toContain('gitHubPullRequests');
  });
});
