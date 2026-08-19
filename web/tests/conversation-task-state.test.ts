import { describe, expect, it } from 'vitest';
import {
  applyConversationTaskSnapshot,
  findConversationTask,
} from '@/lib/conversation-task-state';
import type { ConversationSidebarVm, ConversationTaskRowVm } from '@/types';

function task(
  projectId: string,
  taskId: string,
  title: string,
  overrides: Partial<ConversationTaskRowVm> = {},
): ConversationTaskRowVm {
  return {
    projectId,
    taskId,
    title,
    autoTitle: false,
    runMode: 'direct',
    runs: [],
    pinned: false,
    pinnedOrder: null,
    ...overrides,
  };
}

function sidebar(tasks: ConversationTaskRowVm[]): ConversationSidebarVm {
  return {
    workspaces: [
      { projectId: 'project-a', workspacePath: '/a', name: 'A' },
      { projectId: 'project-b', workspacePath: '/b', name: 'B' },
    ],
    pinnedTasks: tasks.filter((candidate) => candidate.pinned),
    tasksByWorkspace: {
      'project-a': tasks.filter((candidate) => candidate.projectId === 'project-a'),
      'project-b': tasks.filter((candidate) => candidate.projectId === 'project-b'),
    },
  };
}

describe('conversation task canonical state', () => {
  it('resolves titles by project and task identity without crossing workspaces', () => {
    const state = sidebar([
      task('project-a', 'task-1', 'Title A'),
      task('project-b', 'task-1', 'Title B'),
    ]);

    expect(findConversationTask(state, 'project-a', 'task-1')?.title).toBe('Title A');
    expect(findConversationTask(state, 'project-b', 'task-1')?.title).toBe('Title B');
  });

  it('applies one authoritative snapshot to workspace and pinned projections', () => {
    const original = task('project-a', 'task-1', 'h', { pinned: true, pinnedOrder: 0 });
    const state = sidebar([original]);
    const authoritative = { ...original, title: 'hi' };
    const next = applyConversationTaskSnapshot(state, authoritative);

    expect(next.tasksByWorkspace['project-a'][0]?.title).toBe('hi');
    expect(next.pinnedTasks[0]?.title).toBe('hi');
  });

  it('inserts a newly created task at the front of its workspace', () => {
    const state = sidebar([task('project-a', 'task-old', 'Old')]);
    const created = task('project-a', 'task-new', 'hi');
    const next = applyConversationTaskSnapshot(state, created);

    expect(next.tasksByWorkspace['project-a'].map((candidate) => candidate.taskId))
      .toEqual(['task-new', 'task-old']);
  });

  it('keeps the current title when no authoritative snapshot is applied', () => {
    const state = sidebar([task('project-a', 'task-1', 'hi')]);

    expect(findConversationTask(state, 'project-a', 'task-1')?.title).toBe('hi');
  });
});
