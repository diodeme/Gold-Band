import { describe, expect, it } from 'vitest';
import { pathFromRoute, routeFromPath, taskListPage } from '../src/routes';

describe('desktop entry routing', () => {
  it('uses the conversation home as the default root entry', () => {
    expect(routeFromPath('/')).toMatchObject({
      uiMode: 'conversation',
      module: 'task-orchestration',
      conversationPage: { kind: 'conversation-home' },
    });
  });

  it('keeps the explicit conversation home route stable', () => {
    expect(routeFromPath('/chat')).toMatchObject({
      uiMode: 'conversation',
      conversationPage: { kind: 'conversation-home' },
    });
  });

  it('retains explicit workbench deep links while the visible entry is hidden', () => {
    expect(routeFromPath('/tasks')).toMatchObject({
      uiMode: 'workbench',
      taskPage: { kind: 'task-list' },
    });
  });

  it('routes scheduled task management inside the conversation shell', () => {
    expect(routeFromPath('/chat/scheduled-tasks')).toMatchObject({
      uiMode: 'conversation',
      module: 'task-orchestration',
      conversationPage: { kind: 'scheduled-tasks' },
    });
  });

  it('round-trips the dedicated scheduled task creation route', () => {
    const path = '/chat/scheduled-tasks/new';
    const page = routeFromPath(path).conversationPage;

    expect(page).toEqual({ kind: 'scheduled-task-create' });
    expect(pathFromRoute('task-orchestration', taskListPage, page)).toBe(path);
  });

  it('round-trips scheduled occurrence run links with round and attempt selection', () => {
    const path = '/chat/projects/project-a/tasks/task-a/runs/run-a/rounds/round-a/attempts/attempt-a';
    const page = routeFromPath(path).conversationPage;

    expect(page).toEqual({
      kind: 'conversation-run',
      projectId: 'project-a',
      taskId: 'task-a',
      runId: 'run-a',
      roundId: 'round-a',
      attemptId: 'attempt-a',
    });
    expect(pathFromRoute('task-orchestration', taskListPage, page)).toBe(path);
  });
});
