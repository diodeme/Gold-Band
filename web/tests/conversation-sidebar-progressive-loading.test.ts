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
  it('distinguishes not-loaded from ready-empty and publishes workspace identity first', () => {
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
