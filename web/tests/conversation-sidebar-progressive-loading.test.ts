import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import {
  CONVERSATION_SIDEBAR_TASK_WINDOW,
  applyConversationSidebarBootstrap,
  applyConversationRunSummaryPage,
  applyConversationTaskPage,
  beginConversationSidebarBootstrap,
  createEmptyConversationSidebar,
  clearConversationLocatedTask,
  locateConversationSidebarTask,
} from '@/lib/conversation-sidebar-loading';
import { RefreshSingleFlight } from '@/lib/refresh-single-flight';
import { applyConversationTaskSnapshot } from '@/lib/conversation-task-state';
import { applyConversationSidebarTaskActivity, applyConversationSidebarTerminalResultUpdate } from '@/lib/conversation-sidebar-activity';

describe('conversation sidebar progressive loading', () => {
  it.each(['activity', 'terminal', 'snapshot'] as const)('retains a located conversation after renewed %s and navigation away', (update) => {
    const tasks = Array.from({ length: CONVERSATION_SIDEBAR_TASK_WINDOW }, (_, index) => ({
      projectId: 'a', taskId: `task-${200 - index}`, taskUuid: `uuid-${index}`, title: 'Recent',
      autoTitle: false, runMode: 'direct' as const, runs: [], runHistoryStatus: 'not-loaded' as const,
      pinned: false, lastActivityAt: '2026-10-27T00:00:00Z',
    }));
    const target = { ...tasks[0], taskId: 'task-001', taskUuid: 'uuid-old', lastActivityAt: '2026-01-01T00:00:00Z' };
    const located = locateConversationSidebarTask({
      ...createEmptyConversationSidebar(), workspaces: [{ projectId: 'a', name: 'A', workspacePath: 'D:/A' }],
      tasksByWorkspace: { a: tasks },
    }, target);
    const taskActivityAt = '2026-10-28T00:00:00Z';
    const active = update === 'activity'
      ? applyConversationSidebarTaskActivity(located, 'a', target.taskId, { phase: 'prompt', stopping: false }, taskActivityAt)
      : update === 'terminal'
        ? applyConversationSidebarTerminalResultUpdate(located, { projectId: 'a', taskId: target.taskId, unreadTerminalResult: null, taskActivityAt })
        : applyConversationTaskSnapshot(located, { ...target, lastActivityAt: taskActivityAt });
    const left = clearConversationLocatedTask(active, tasks[0]);
    expect(left.tasksByWorkspace.a.some((task) => task.taskUuid === target.taskUuid)).toBe(true);
    expect(left.tasksByWorkspace.a[0].taskUuid).toBe(target.taskUuid);
    expect(left.locatedTask).toBeNull();
    expect(left.tasksByWorkspace.a).toHaveLength(CONVERSATION_SIDEBAR_TASK_WINDOW);
  });

  it.each([undefined, '2025-01-01T00:00:00Z', '2026-01-01T00:00:00Z'])('keeps temporary placement without newer activity (%s)', (activityAt) => {
    const target = { projectId: 'a', taskId: 'old', taskUuid: 'uuid-old', title: 'Old', autoTitle: false,
      runMode: 'direct' as const, pinned: false, runs: [], lastActivityAt: '2026-01-01T00:00:00Z' };
    const located = locateConversationSidebarTask({ ...createEmptyConversationSidebar(),
      workspaces: [{ projectId: 'a', name: 'A', workspacePath: 'D:/A' }],
    }, target);
    const next = applyConversationSidebarTaskActivity(located, 'a', 'old', { phase: 'prompt', stopping: false }, activityAt);
    expect(next.locatedTask).toEqual(located.locatedTask);
    expect(clearConversationLocatedTask(next).tasksByWorkspace.a).toEqual([]);
  });

  it('keeps a located old row outside the regular 120-row paging window', () => {
    const tasks = Array.from({ length: 120 }, (_, index) => ({
      projectId: 'a', taskId: `task-${200 - index}`, taskUuid: `uuid-${index}`, title: 'Recent',
      autoTitle: false, runMode: 'direct' as const, runs: [], runHistoryStatus: 'not-loaded' as const, pinned: false,
    }));
    const target = { ...tasks[0], taskId: 'task-001', taskUuid: 'uuid-old', title: 'Old' };
    const sidebar = {
      ...createEmptyConversationSidebar(),
      workspaces: [{ projectId: 'a', name: 'A', workspacePath: 'D:/A' }],
      tasksByWorkspace: { a: [...tasks, target] },
      locatedTask: { projectId: 'a', taskId: target.taskId, taskUuid: target.taskUuid },
    };
    const next = applyConversationTaskPage(sidebar, { projectId: 'a', tasks: tasks.slice(0, 24), nextCursor: 'cursor', errors: [] }, false);
    expect(next.tasksByWorkspace.a.some((task) => task.taskUuid === target.taskUuid)).toBe(true);
    expect(next.tasksByWorkspace.a).toHaveLength(121);
    expect(next.workspaceTaskPages.a.nextCursor).toBeNull();
    expect(clearConversationLocatedTask(next).tasksByWorkspace.a).toHaveLength(120);
    expect(clearConversationLocatedTask(next, target)).toBe(next);
    expect(clearConversationLocatedTask(next, tasks[0]).tasksByWorkspace.a).toHaveLength(120);
    const updated = applyConversationTaskSnapshot(next, { ...target, title: 'Renamed target' });
    expect(updated.tasksByWorkspace.a.find((task) => task.taskId === target.taskId)?.title).toBe('Renamed target');
    const withNewTask = applyConversationTaskSnapshot(updated, { ...tasks[0], taskId: 'task-999', taskUuid: 'uuid-new' });
    expect(withNewTask.tasksByWorkspace.a).toHaveLength(121);
    expect(withNewTask.tasksByWorkspace.a.some((task) => task.taskUuid === target.taskUuid)).toBe(true);
    const secondTarget = { ...target, taskId: 'task-002', taskUuid: 'uuid-second' };
    const replaced = locateConversationSidebarTask(updated, secondTarget);
    expect(replaced.tasksByWorkspace.a).toHaveLength(121);
    expect(replaced.tasksByWorkspace.a.some((task) => task.taskId === target.taskId)).toBe(false);
    const normalPage = applyConversationTaskPage(replaced, { projectId: 'a', tasks: [secondTarget], nextCursor: 'next', errors: [] }, false);
    expect(normalPage.locatedTask).toBeNull();
    expect(normalPage.tasksByWorkspace.a.filter((task) => task.taskId === secondTarget.taskId)).toHaveLength(1);
    expect(clearConversationLocatedTask(normalPage)).toBe(normalPage);
  });
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
    const flights = new RefreshSingleFlight();
    const request = vi.fn(async () => 'done');

    const first = flights.run('workspace:project-a', request);
    const second = flights.run('workspace:project-a', request);

    await expect(first).resolves.toBe('done');
    await expect(second).resolves.toBe('done');
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('fences a late page after its entity scope is invalidated', async () => {
    const flights = new RefreshSingleFlight();
    const generation = flights.generation('workspace:project-a');

    flights.invalidate('workspace:project-a');

    expect(flights.isCurrent('workspace:project-a', generation)).toBe(false);
    expect(flights.generation('workspace:project-a')).toBe(generation + 1);
  });

  it('rereads a workspace after background creation arrives during an older page request', async () => {
    const flights = new RefreshSingleFlight();
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
    const flights = new RefreshSingleFlight();
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
    const flights = new RefreshSingleFlight();
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
