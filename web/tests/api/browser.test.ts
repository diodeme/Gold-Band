/** @vitest-environment jsdom */

import { describe, expect, it, vi } from 'vitest';
import type { AcpSessionUpdatedEventVm } from '../../src/api/client';
import { browserApi } from '../../src/api/browser';

describe('browserApi', () => {
  it('keeps paused Workflow errors independent from manual ACP follow-up lifecycle', async () => {
    vi.resetModules();
    vi.useFakeTimers();
    const api = await import('../../src/api');
    const { subscribeConversationAttemptEvents, readConversationBranchLiveSnapshot } = await import('../../src/lib/conversation-event-router');
    const { applyConversationSelectedSessionSnapshot } = await import('../../src/lib/conversation-run-snapshot');
    const locator = ['default', 'mock-task', 'run-054', 'round-001', 'dev', 'attempt-001'] as const;
    const initial = await api.getConversationRun('default', 'mock-task', 'run-054');
    const eventLocator = {
      projectId: initial.projectId, taskId: initial.taskId, taskUuid: initial.taskUuid,
      runId: initial.runId, roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-001',
    };
    const updates: AcpSessionUpdatedEventVm[] = [];
    const unsubscribe = subscribeConversationAttemptEvents(eventLocator, (event) => updates.push(event));
    try {
      expect(initial).toMatchObject({ runMode: 'workflow', runStatus: 'paused', pauseReason: 'runtime-abnormal' });
      expect(initial.runtimeError?.diagnostic).toBeTruthy();
      const initialLifecycle = initial.sessionTree.rounds[0].nodes[0].attempts[0].lifecycle!;
      expect(initialLifecycle).toMatchObject({
        runtime: { status: 'paused', active: false, revision: 1 },
        control: { mode: 'non-runtime-controlled' },
        acp: { latestTurnStatus: 'failed', liveTurnActivity: 'idle' },
        composer: { submitTarget: 'acp-prompt', lockInput: false },
      });
      const session = await api.getAcpSession(...locator);
      expect(session?.events.length).toBeGreaterThan(0);
      expect(session?.turnError).toEqual(initialLifecycle.acp.turnError);
      expect(session?.status).toBe('failed');
      await expect(api.getAcpSession('other-project', 'mock-task', 'run-054', 'round-001', 'dev', 'attempt-001')).resolves.toBeNull();
      await expect(api.getAcpSession(...locator, { branchId: 'other-branch' })).resolves.toBeNull();
      await api.submitConversationPrompt('other-project', 'mock-task', 'run-054', 'round-001', 'dev', 'attempt-001', { displayText: 'Other project', quotes: [] });
      expect(updates).toHaveLength(0);

      const admitted = await api.submitConversationPrompt(...locator, { displayText: '继续分析这个问题', quotes: [] }, 'follow-up-1', session);
      expect(admitted).toMatchObject({ kind: 'acp-session-started', lifecycle: {
        runtime: initialLifecycle.runtime,
        acp: { liveTurnActivity: 'accepted', latestTurnStatus: 'none', turnError: null, promptEventId: 'follow-up-1' },
      } });
      expect.soft(admitted.session?.events.find((event) => event.id === 'follow-up-1')).toMatchObject({
        kind: 'userTextDelta', raw: { source: 'goldBandPrompt', promptId: 'follow-up-1' },
      });
      expect(admitted.turnId).not.toBe(initialLifecycle.acp.turnId);
      expect(admitted.operationId).not.toBe(initialLifecycle.acp.operationId);
      expect(admitted.revision).toBeGreaterThan(initialLifecycle.acp.revision!);
      await expect(api.submitConversationPrompt(...locator, { displayText: 'Concurrent submission', quotes: [] }, 'concurrent-prompt'))
        .resolves.toMatchObject({ kind: 'rejected' });
      await vi.advanceTimersToNextTimerAsync();
      expect(updates.at(-1)).toMatchObject({
        projectId: 'default', taskId: 'mock-task', runId: 'run-054',
        roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-001',
        session: { status: 'running', turnError: null },
        lifecycle: { runtime: initialLifecycle.runtime, acp: { liveTurnActivity: 'running', turnId: admitted.turnId } },
      });
      expect.soft(updates.at(-1)?.lifecycle).toMatchObject({
        displayStatus: 'running', runtimeDisplay: { code: 'running', tone: 'running', blockingError: false },
      });
      const projected = applyConversationSelectedSessionSnapshot(initial, {
        ...eventLocator, taskUuid: initial.taskUuid!, lifecycle: updates.at(-1)!.lifecycle,
      });
      expect.soft(projected?.sessionTree.rounds[0].nodes[0].attempts[0].runtimeDisplay.tone).toBe('running');
      const runningRun = await api.getConversationRun('default', 'mock-task', 'run-054');
      expect.soft(runningRun.activeSessions).toHaveLength(1);
      expect.soft(runningRun.sessionTree.rounds[0].nodes[0].attempts[0].runtimeDisplay.tone).toBe('running');
      expect(readConversationBranchLiveSnapshot(eventLocator, 'root')).toMatchObject({
        status: 'running', acp: { liveTurnActivity: 'running', turnError: null, turnId: admitted.turnId },
      });
      await vi.runAllTimersAsync();
      expect(updates.map((event) => event.lifecycle?.acp.liveTurnActivity)).toEqual(['accepted', 'running', 'idle']);
      expect(updates.at(-1)).toMatchObject({
        session: { status: 'completed', turnError: null },
        lifecycle: { runtime: initialLifecycle.runtime, acp: { latestTurnStatus: 'completed', turnError: null } },
      });
      const completed = await api.getAcpSession(...locator);
      expect(completed?.events.some((event) => event.content === '继续分析这个问题')).toBe(true);
      const run = await api.getConversationRun('default', 'mock-task', 'run-054');
      expect(run).toMatchObject({ runStatus: 'paused', pauseReason: 'runtime-abnormal', runtimeError: initial.runtimeError, activeSessions: [] });
      expect(run.sessionTree.rounds[0].nodes[0].attempts[0].lifecycle?.acp.latestTurnStatus).toBe('completed');
      const second = await api.submitConversationPrompt(...locator, { displayText: '补充说明', quotes: [] }, 'follow-up-2', completed);
      expect(second.turnId).not.toBe(admitted.turnId);
      expect(second.operationId).not.toBe(admitted.operationId);
      expect(second.revision).toBeGreaterThan(updates.at(-2)!.lifecycle!.acp.revision!);
      await vi.runAllTimersAsync();
      expect((await api.getConversationRun('default', 'mock-task', 'run-051')).pauseReason).toBe('error-blocked');
      expect((await api.getAcpSession('default', 'mock-task', 'run-052', 'round-001', 'dev', 'attempt-001'))?.sessionId).toBe('browser-session-052');
      const count = updates.length;
      unsubscribe();
      await api.submitConversationPrompt(...locator, { displayText: '确认', quotes: [] }, 'follow-up-3');
      await vi.runAllTimersAsync();
      expect(updates).toHaveLength(count);
    } finally {
      unsubscribe();
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });

  it.each(['run-051', 'run-052', 'run-053', 'run-054'])('keeps %s session content out of run summaries', async (runId) => {
    const run = await browserApi.getConversationRun('default', 'mock-task', runId);
    expect(run).not.toHaveProperty('selectedSession');

    const session = await browserApi.getAcpSession(
      'default', 'mock-task', runId, 'round-001', 'dev', 'attempt-001',
    );
    expect(session).not.toBeNull();
    expect(session).toHaveProperty('pendingInteractions');
    if (runId !== 'run-051') {
      expect(session?.events.length).toBeGreaterThan(0);
      expect(session?.sessionId).toBe(`browser-session-${runId.slice(-3)}`);
    }
  });

  it('keeps queued authoring payload out of lifecycle summaries and restores one item on demand', async () => {
    const run = await browserApi.getConversationRun('default', 'mock-task', 'run-053');
    const item = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0]?.lifecycle?.promptQueue?.items[0];
    expect(item).toMatchObject({
      id: 'browser-queued-1',
      attachmentCount: 1,
      quoteCount: 0,
      workspaceFileCount: 0,
    });
    const fileItem = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0]?.lifecycle?.promptQueue?.items[2];
    expect(fileItem).toMatchObject({
      id: 'browser-queued-3',
      workspaceFileCount: 1,
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
    await expect(browserApi.startPersonalAnalyticsInsights(
      'agent-a',
      range,
      'model-a',
      'reasoning_effort',
      'high',
    )).resolves.toMatchObject({
      agentType: 'agent-a',
      modelId: 'model-a',
      thoughtLevelOptionId: 'reasoning_effort',
      thoughtLevelValue: 'high',
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

  it('mirrors Windows slash-prefixed drive pathname normalization', async () => {
    const resolved = await browserApi.resolveWorkspaceFileLink(
      'default',
      '/D:/outside/roadmap.md:12',
    );

    expect(resolved.locator.canonicalPath).toBe('D:/outside/roadmap.md');
    expect(resolved.target).toEqual({ line: 12, column: null, endLine: null });
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
