import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import type { ConversationSidebarVm } from '@/types';

import {
  CONVERSATION_PINNED_TASK_PAGE_SIZE,
  CONVERSATION_SIDEBAR_RECENT_WINDOW,
  CONVERSATION_TASK_PAGE_SIZE,
  applyConversationRecentTaskPage,
  applyConversationSidebarBootstrap,
  beginConversationRecentTaskLoad,
  conversationSidebarBootstrapInvalidationKeys,
  createEmptyConversationSidebar,
  failConversationRecentTaskLoad,
} from '@/lib/conversation-sidebar-loading';
import { applyConversationTaskSnapshot } from '@/lib/conversation-task-state';
import { applyConversationSidebarRunLifecycle, applyConversationSidebarRunStateUpdate, applyConversationSidebarTaskActivity } from '@/lib/conversation-sidebar-activity';
import { removeConversationSidebarTask } from '@/lib/conversation-sidebar-loading';

function recentRow(overrides: Partial<Parameters<typeof applyConversationRecentTaskPage>[1]['tasks'][number]> = {}) {
  return {
    projectId: 'a', taskId: 'task-1', taskUuid: 'uuid-1', title: 'Task A', autoTitle: false,
    runMode: 'direct' as const, runs: [], runHistoryStatus: 'not-loaded' as const, pinned: false,
    workspaceName: 'Workspace A', ...overrides,
  };
}

function sidebarWithRecent(recentTasks: ReturnType<typeof recentRow>[]) {
  return {
    ...createEmptyConversationSidebar(),
    workspaces: [
      { projectId: 'a', name: 'Workspace A', workspacePath: '/a' },
      { projectId: 'b', name: 'Workspace B', workspacePath: '/b' },
    ],
    recentTasks,
    recentTaskPage: { status: 'ready' as const, nextCursor: null },
  };
}

describe('conversation sidebar recent tasks', () => {
  it('pages the merged list with cursor pagination and a bounded window', () => {
    let sidebar = createEmptyConversationSidebar();
    sidebar = beginConversationRecentTaskLoad(sidebar);
    expect(sidebar.recentTaskPage.status).toBe('loading');

    const first = Array.from({ length: 10 }, (_, index) => recentRow({ taskId: `task-${index}`, taskUuid: `uuid-${index}` }));
    sidebar = applyConversationRecentTaskPage(sidebar, { tasks: first, nextCursor: 'cursor-1', errors: [] }, false);
    expect(sidebar.recentTasks).toHaveLength(10);
    expect(sidebar.recentTaskPage).toEqual({ status: 'ready', nextCursor: 'cursor-1' });

    // Append dedupes by stable task identity and keeps the window bound.
    const appended = [first[0], ...Array.from({ length: 10 }, (_, index) => recentRow({ projectId: 'b', taskId: `task-b-${index}`, taskUuid: `uuid-b-${index}`, workspaceName: 'Workspace B' }))];
    const full = Array.from({ length: CONVERSATION_SIDEBAR_RECENT_WINDOW - 19 }, (_, index) => recentRow({ taskId: `task-fill-${index}`, taskUuid: `uuid-fill-${index}` }));
    sidebar = applyConversationRecentTaskPage(sidebar, { tasks: [...appended, ...full], nextCursor: 'more', errors: [] }, true);
    expect(sidebar.recentTasks).toHaveLength(CONVERSATION_SIDEBAR_RECENT_WINDOW);
    expect(new Set(sidebar.recentTasks.map((task) => task.taskUuid)).size).toBe(CONVERSATION_SIDEBAR_RECENT_WINDOW);
    expect(sidebar.recentTaskPage.nextCursor).toBeNull();

    sidebar = failConversationRecentTaskLoad(sidebar);
    expect(sidebar.recentTaskPage.status).toBe('error');
  });

  it('refreshes without duplicating rows and keeps loaded history like the workspace pages', () => {
    const first = [recentRow({ taskId: 'task-1', taskUuid: 'uuid-1' }), recentRow({ taskId: 'task-2', taskUuid: 'uuid-2' })];
    let sidebar = applyConversationRecentTaskPage(createEmptyConversationSidebar(), { tasks: first, nextCursor: null, errors: [] }, false);
    // A refresh (append=false) puts the incoming page first and keeps rows from
    // earlier pages that the fresh page no longer covers — same contract as
    // applyConversationTaskPage, so a refresh never clears loaded history.
    sidebar = applyConversationRecentTaskPage(sidebar, { tasks: [recentRow({ taskId: 'task-1', taskUuid: 'uuid-1' })], nextCursor: null, errors: [] }, false);
    expect(sidebar.recentTasks.map((task) => task.taskUuid)).toEqual(['uuid-1', 'uuid-2']);
    expect(sidebar.recentTasks).toHaveLength(2);
  });

  it('restarts the recent page when the workspace set changes', () => {
    let sidebar: ConversationSidebarVm = sidebarWithRecent([recentRow()]);
    sidebar = applyConversationSidebarBootstrap(sidebar, {
      workspaces: [{ projectId: 'b', name: 'Workspace B', workspacePath: '/b' }],
      pinRefs: [],
      lastActiveWorkspaceId: 'b',
      preferences: {},
    });
    expect(sidebar.recentTasks).toHaveLength(0);
    expect(sidebar.recentTaskPage).toEqual({ status: 'not-loaded', nextCursor: null });
  });

  it('projects activity updates into recent rows and moves advanced rows to the front', () => {
    const older = recentRow({ taskId: 'task-old', taskUuid: 'uuid-old', lastActivityAt: '2026-01-01T00:00:00Z' });
    const newer = recentRow({ projectId: 'b', taskId: 'task-new', taskUuid: 'uuid-new', workspaceName: 'Workspace B', lastActivityAt: '2026-06-01T00:00:00Z' });
    const sidebar = sidebarWithRecent([newer, older]);

    const updated = applyConversationSidebarTaskActivity(sidebar, 'a', 'task-old', { phase: 'prompt', stopping: false }, '2026-07-01T00:00:00Z');
    expect(updated.recentTasks[0].taskId).toBe('task-old');
    expect(updated.recentTasks[0].lastActivityAt).toBe('2026-07-01T00:00:00Z');
    expect(updated.recentTasks[0].workspaceName).toBe('Workspace A');
  });

  it('projects run-state updates into recent rows without reordering', () => {
    const row = recentRow({ runMode: 'workflow', taskId: 'task-1', latestRun: { runId: 'run-1', status: 'running', updatedAt: '2026-01-01T00:00:00Z' } as never });
    const sidebar = sidebarWithRecent([row]);
    const updated = applyConversationSidebarRunStateUpdate(sidebar, {
      projectId: 'a', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1', status: 'completed', outcome: 'success',
    } as never);
    expect(updated.recentTasks[0].latestRun?.status).toBe('completed');
    expect(updated.recentTasks[0].workspaceName).toBe('Workspace A');
  });

  it('settles a stale live-activity sample when the run reaches a terminal status', () => {
    // The last session update raced the runner: the row still pins a running
    // activity sample while the run has already completed. The terminal run
    // state must stop the spinner in the same projection.
    const row = recentRow({
      runMode: 'workflow',
      taskId: 'task-1',
      latestRun: { runId: 'run-1', status: 'running', updatedAt: '2026-01-01T00:00:00Z' } as never,
      activity: { phase: 'running', stopping: false },
    });
    const sidebar = sidebarWithRecent([row]);
    const updated = applyConversationSidebarRunStateUpdate(sidebar, {
      projectId: 'a', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1', status: 'completed', outcome: 'success',
    } as never);
    expect(updated.recentTasks[0].latestRun?.status).toBe('completed');
    expect(updated.recentTasks[0].activity ?? null).toBeNull();
  });

  it('keeps background facts alive across a terminal run state', () => {
    const row = recentRow({
      runMode: 'workflow',
      taskId: 'task-1',
      latestRun: { runId: 'run-1', status: 'running', updatedAt: '2026-01-01T00:00:00Z' } as never,
      activity: { phase: 'background', stopping: false },
    });
    const sidebar = sidebarWithRecent([row]);
    const updated = applyConversationSidebarRunStateUpdate(sidebar, {
      projectId: 'a', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1', status: 'completed', outcome: 'success',
    } as never);
    expect(updated.recentTasks[0].activity?.phase).toBe('background');
  });

  it('projects an active run lifecycle into recent rows of unloaded workspaces', () => {
    // The task lives only in the cross-workspace recent list: its workspace page
    // is not loaded, so the lifecycle event is the only projection available.
    const row = recentRow({
      runMode: 'workflow',
      taskId: 'task-1',
      latestRun: { runId: 'run-1', status: 'queued', outcome: null, updatedAt: '2026-01-01T00:00:00Z', resumable: true } as never,
      runs: [{ runId: 'run-1', status: 'queued', outcome: null, updatedAt: '2026-01-01T00:00:00Z', resumable: true } as never],
    });
    const sidebar = sidebarWithRecent([row]);
    const updated = applyConversationSidebarRunLifecycle(sidebar, 'a', 'task-1', 'run-1', {
      runtime: { active: true, phase: 'runtime-active' },
      acp: { liveTurnActivity: 'idle', stopping: false },
    } as never);
    expect(updated.recentTasks[0].latestRun?.status).toBe('running');
    expect(updated.recentTasks[0].latestRun?.resumable).toBe(false);
    expect(updated.recentTasks[0].workspaceName).toBe('Workspace A');
  });

  it('upserts task snapshots into recent rows and adds new tasks to the front', () => {
    const existing = recentRow({ taskId: 'task-1', lastActivityAt: '2026-01-01T00:00:00Z' });
    let sidebar = sidebarWithRecent([existing]);

    // Existing row is updated in place, keeping the workspace label.
    const updated = applyConversationTaskSnapshot(sidebar, { ...existing, title: 'Renamed', workspaceName: undefined } as never);
    expect(updated.recentTasks).toHaveLength(1);
    expect(updated.recentTasks[0].title).toBe('Renamed');
    expect(updated.recentTasks[0].workspaceName).toBe('Workspace A');

    // A brand-new task joins the merged front with its workspace name resolved.
    const created = applyConversationTaskSnapshot(updated, {
      projectId: 'b', taskId: 'task-9', taskUuid: 'uuid-9', title: 'New B', autoTitle: true, runMode: 'direct',
      runs: [], runHistoryStatus: 'not-loaded', pinned: false,
    } as never);
    expect(created.recentTasks[0].taskId).toBe('task-9');
    expect(created.recentTasks[0].workspaceName).toBe('Workspace B');
  });

  it('keeps recent row identity when a snapshot carries no visible change', () => {
    const existing = recentRow({ taskId: 'task-1', taskUuid: 'uuid-1', lastActivityAt: '2026-01-01T00:00:00Z' });
    const sidebar = sidebarWithRecent([existing]);

    // Identical authoritative snapshots arrive as fresh objects on every task
    // refresh; the memoized RecentTaskRow must not re-render for them.
    const unchanged = applyConversationTaskSnapshot(sidebar, { ...existing, workspaceName: undefined } as never);
    expect(unchanged.recentTasks).toBe(sidebar.recentTasks);

    // A visible change still rebuilds the row in place.
    const changed = applyConversationTaskSnapshot(sidebar, { ...existing, title: 'Renamed', workspaceName: undefined } as never);
    expect(changed.recentTasks).not.toBe(sidebar.recentTasks);
    expect(changed.recentTasks[0].title).toBe('Renamed');
  });

  it('inserts the very first conversation into a loaded-but-empty recent page', () => {
    const empty = sidebarWithRecent([]);
    expect(empty.recentTaskPage.status).toBe('ready');
    const created = applyConversationTaskSnapshot(empty, {
      projectId: 'a', taskId: 'task-1', taskUuid: 'uuid-1', title: 'First', autoTitle: true, runMode: 'direct',
      runs: [], runHistoryStatus: 'not-loaded', pinned: false,
    } as never);
    expect(created.recentTasks).toHaveLength(1);
    expect(created.recentTasks[0].taskId).toBe('task-1');
    expect(created.recentTasks[0].workspaceName).toBe('Workspace A');
  });

  it('drops a deleted task from the recent projection', () => {
    const sidebar = sidebarWithRecent([
      recentRow({ taskId: 'task-1', taskUuid: 'uuid-1' }),
      recentRow({ projectId: 'b', taskId: 'task-2', taskUuid: 'uuid-2', workspaceName: 'Workspace B' }),
    ]);
    const next = removeConversationSidebarTask(sidebar, 'a', 'task-1');
    expect(next.recentTasks.map((task) => task.taskId)).toEqual(['task-2']);
  });

  it('invalidates every page flight when the bootstrap identity set lands', () => {
    // Workspaces known only to the old sidebar state, only to the incoming
    // bootstrap, and to both — every page flight paginates a workspace set
    // that just changed, so all of their premises are void.
    let sidebar = sidebarWithRecent([recentRow()]);
    sidebar = {
      ...sidebar,
      tasksByWorkspace: { ...sidebar.tasksByWorkspace, 'gone-ws': [], 'kept-ws': [] },
    };
    const keys = conversationSidebarBootstrapInvalidationKeys(sidebar, {
      workspaces: [
        { projectId: 'kept-ws', name: 'Kept', workspacePath: '/kept' },
        { projectId: 'new-ws', name: 'New', workspacePath: '/new' },
      ],
      pinRefs: [],
      lastActiveWorkspaceId: 'new-ws',
      preferences: {},
    });
    expect(new Set(keys)).toEqual(new Set([
      'workspace:gone-ws',
      'workspace:kept-ws',
      'workspace:new-ws',
      'workspace:a',
      'workspace:b',
      'pinned',
      'recent',
    ]));
  });

  it('keeps every sidebar section at the documented 7-row page size', () => {
    expect(CONVERSATION_TASK_PAGE_SIZE).toBe(7);
    expect(CONVERSATION_PINNED_TASK_PAGE_SIZE).toBe(7);
    const app = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(app).toContain('getConversationPinnedTaskPage(cursor, CONVERSATION_PINNED_TASK_PAGE_SIZE)');
    expect(app).toContain('getConversationTaskPage(projectId, cursor, CONVERSATION_TASK_PAGE_SIZE)');
    // The backend fallbacks must not disagree with the documented defaults.
    const viewModels = readFileSync(path.resolve(__dirname, '../../src-tauri/src/view_models_conversation.rs'), 'utf8');
    expect(viewModels).toContain('pub const CONVERSATION_RUN_PAGE_DEFAULT_LIMIT: usize = 7;');
    expect(viewModels).toContain('pub const CONVERSATION_PINNED_TASK_PAGE_DEFAULT_LIMIT: usize = 7;');
  });
});
