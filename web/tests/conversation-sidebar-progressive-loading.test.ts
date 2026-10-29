import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import {
  ConversationSidebarSingleFlight,
  CONVERSATION_SIDEBAR_TASK_WINDOW,
  applyConversationSidebarBootstrap,
  applyConversationRunSummaryPage,
  applyConversationTaskPage,
  beginConversationSidebarBootstrap,
  createEmptyConversationSidebar,
} from '@/lib/conversation-sidebar-loading';

describe('conversation sidebar progressive loading', () => {
  it('distinguishes not-loaded from ready and publishes workspace identity first', () => {
    const initial = createEmptyConversationSidebar();
    expect(initial.loadStatus).toBe('not-loaded');

    const loading = beginConversationSidebarBootstrap(initial);
    expect(loading.loadStatus).toBe('loading');

    const bootstrapped = applyConversationSidebarBootstrap(loading, {
      workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
      pinRefs: [],
      lastActiveWorkspaceId: 'project-a',
      preferences: {},
    });
    expect(bootstrapped.loadStatus).toBe('ready');
    expect(bootstrapped.workspaceTaskPages['project-a']?.status).toBe('not-loaded');
    expect(bootstrapped.tasksByWorkspace['project-a']).toEqual([]);
  });

  it('does not demote a ready sidebar to loading during a background bootstrap refresh', () => {
    const ready = applyConversationSidebarBootstrap(createEmptyConversationSidebar(), {
      workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
      pinRefs: [],
      lastActiveWorkspaceId: 'project-a',
      preferences: {},
    });
    expect(ready.loadStatus).toBe('ready');
    const refreshing = beginConversationSidebarBootstrap(ready);
    expect(refreshing.loadStatus).toBe('ready');
    expect(refreshing).toBe(ready);
  });

  it('does not reload the conversation sidebar when only app bootstrap preferences change', () => {
    const source = fs.readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(source).toContain('[conversationShellReady, loadConversationSidebarBootstrap]');
    expect(source).not.toContain('[bootstrap, loadConversationSidebarBootstrap, uiMode]');
  });

  it('merges a bounded task page by stable identity without treating it as full history', () => {
    const sidebar = applyConversationSidebarBootstrap(createEmptyConversationSidebar(), {
      workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
      pinRefs: [],
      lastActiveWorkspaceId: 'project-a',
      preferences: {},
    });
    const next = applyConversationTaskPage(sidebar, {
      projectId: 'project-a',
      tasks: [{
        projectId: 'project-a', taskId: 'task-002', taskUuid: 'uuid-2', title: 'Task 2', autoTitle: false,
        runMode: 'workflow', lastActivityAt: '2026-08-01T00:00:00Z', latestRun: null, runs: [],
        pinned: false, pinnedOrder: null,
      }],
      nextCursor: 'task-001',
      errors: [],
    }, false);

    expect(next.tasksByWorkspace['project-a']).toHaveLength(1);
    expect(next.workspaceTaskPages['project-a']).toEqual({ status: 'ready', nextCursor: 'task-001' });
    expect(next.tasksByWorkspace['project-a'][0].runs).toEqual([]);

    const capped = applyConversationTaskPage(sidebar, {
      projectId: 'project-a',
      tasks: Array.from({ length: CONVERSATION_SIDEBAR_TASK_WINDOW + 1 }, (_, index) => ({
        projectId: 'project-a', taskId: `task-${index}`, taskUuid: `uuid-${index}`, title: `Task ${index}`,
        autoTitle: false, runMode: 'workflow', lastActivityAt: null, latestRun: null, runs: [],
        pinned: false, pinnedOrder: null,
      })),
      nextCursor: 'task-older',
      errors: [],
    }, false);
    expect(capped.tasksByWorkspace['project-a']).toHaveLength(CONVERSATION_SIDEBAR_TASK_WINDOW);
    expect(capped.workspaceTaskPages['project-a']?.nextCursor).toBeNull();
  });

  it('coalesces duplicate loads for the same scope', async () => {
    const flights = new ConversationSidebarSingleFlight();
    const request = vi.fn(async () => 'done');

    const first = flights.run('workspace:project-a', request);
    const second = flights.run('workspace:project-a', request);

    await expect(first).resolves.toBe('done');
    await expect(second).resolves.toBe('done');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('fences a late page after its entity scope is invalidated', async () => {
    const flights = new ConversationSidebarSingleFlight();
    const generation = flights.generation('workspace:project-a');

    flights.invalidate('workspace:project-a');

    expect(flights.isCurrent('workspace:project-a', generation)).toBe(false);
    expect(flights.generation('workspace:project-a')).toBe(generation + 1);
  });

  it('rereads a workspace after background creation arrives during an older page request', async () => {
    const flights = new ConversationSidebarSingleFlight();
    let resolveOldPage!: (tasks: string[]) => void;
    const oldPage = new Promise<string[]>((resolve) => { resolveOldPage = resolve; });
    const published: string[][] = [];
    const first = flights.run('workspace:project-a', async () => {
      const tasks = await oldPage;
      published.push(tasks);
      return tasks;
    });
    const refresh = vi.fn(async () => {
      const tasks = ['scheduled-task'];
      published.push(tasks);
      return tasks;
    });
    const refreshed = flights.run('workspace:project-a', refresh, true);
    flights.run('workspace:project-a', refresh, true);
    expect(refresh).not.toHaveBeenCalled();
    resolveOldPage([]);
    await first;
    await refreshed;
    expect(published.at(-1)).toEqual(['scheduled-task']);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('keeps only the latest pending refresh and waits for it after a failed load', async () => {
    const flights = new ConversationSidebarSingleFlight();
    let rejectOldPage!: (error: Error) => void;
    const oldPage = new Promise<string>((_resolve, reject) => { rejectOldPage = reject; });
    const first = flights.run('runs:project-a:uuid-1', () => oldPage);
    const superseded = vi.fn(async () => 'run-001');
    const latest = vi.fn(async () => 'run-002');
    flights.run('runs:project-a:uuid-1', superseded, true);
    const refreshed = flights.run('runs:project-a:uuid-1', latest, true);
    rejectOldPage(new Error('old page failed'));
    await expect(first).resolves.toBe('run-002');
    await expect(refreshed).resolves.toBe('run-002');
    expect(superseded).not.toHaveBeenCalled();
    expect(latest).toHaveBeenCalledTimes(1);
  });

  it('discards pending refreshes when their scope is invalidated', async () => {
    const flights = new ConversationSidebarSingleFlight();
    let resolveOldPage!: (value: string) => void;
    const oldPage = new Promise<string>((resolve) => { resolveOldPage = resolve; });
    const first = flights.run('workspace:project-a', () => oldPage);
    const obsolete = vi.fn(async () => 'deleted-task');
    flights.run('workspace:project-a', obsolete, true);
    flights.invalidate('workspace:project-a');
    const replacement = flights.run('workspace:project-a', async () => 'new-task');
    resolveOldPage('old-task');
    await first;
    await expect(replacement).resolves.toBe('new-task');
    expect(obsolete).not.toHaveBeenCalled();
  });

  it('does not regress task activity metadata when run history is hydrated', () => {
    const sidebar = applyConversationTaskPage(
      applyConversationSidebarBootstrap(createEmptyConversationSidebar(), {
        workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
        pinRefs: [],
        lastActiveWorkspaceId: 'project-a',
        preferences: {},
      }),
      {
        projectId: 'project-a',
        tasks: [{
          projectId: 'project-a', taskId: 'task-001', taskUuid: 'uuid-1', title: 'Task 1', autoTitle: false,
          runMode: 'direct', lastActivityAt: '2026-08-29T12:00:00Z', latestRun: null, runs: [],
          pinned: false, pinnedOrder: null,
        }],
        nextCursor: null,
        errors: [],
      },
      false,
    );

    const next = applyConversationRunSummaryPage(sidebar, {
      projectId: 'project-a',
      taskId: 'task-001',
      taskUuid: 'uuid-1',
      runs: [{
        runId: 'run-001', status: 'completed', outcome: 'success',
        startedAt: '2026-08-28T10:00:00Z', updatedAt: '2026-08-28T11:00:00Z',
        currentRound: null, currentNode: null, resumable: false,
      }],
      nextCursor: null,
      errors: [],
    }, false);

    expect(next.tasksByWorkspace['project-a'][0].lastActivityAt).toBe('2026-08-29T12:00:00Z');
  });
});
