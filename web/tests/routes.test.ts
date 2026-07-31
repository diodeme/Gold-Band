import { describe, expect, it } from 'vitest';
import { routeFromPath } from '../src/routes';

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
});
