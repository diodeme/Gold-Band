import { describe, expect, it, vi } from 'vitest';
import {
  canOpenConversationSidebarRunMenu,
  canPauseConversationSidebarRun,
  conversationSidebarRunKey,
  conversationSidebarTaskKey,
  isConversationSidebarRunActive,
  prioritizeConversationSidebarWorkspace,
  selectConversationSidebarRunPauseAction,
} from '@/components/conversation/ConversationSidebar';

describe('ConversationSidebar run selection identity', () => {
  it('binds an active run to its parent project and task', () => {
    const activeRunKey = conversationSidebarRunKey('project-a', 'task-a', 'run-003');

    expect(isConversationSidebarRunActive(activeRunKey, 'project-a', 'task-a', 'run-003')).toBe(true);
    expect(isConversationSidebarRunActive(activeRunKey, 'project-a', 'task-b', 'run-003')).toBe(false);
    expect(isConversationSidebarRunActive(activeRunKey, 'project-b', 'task-a', 'run-003')).toBe(false);
  });

  it('uses distinct task keys for the single-expanded sidebar task state', () => {
    expect(conversationSidebarTaskKey('project-a', 'task-1')).not.toBe(conversationSidebarTaskKey('project-a', 'task-2'));
    expect(conversationSidebarTaskKey('project-a', 'task-1')).not.toBe(conversationSidebarTaskKey('project-b', 'task-1'));
  });

  it('moves the active workspace to the top of the sidebar immediately', () => {
    const sidebar = prioritizeConversationSidebarWorkspace({
      workspaces: [
        { projectId: 'project-a', workspacePath: '/a', name: 'A' },
        { projectId: 'project-b', workspacePath: '/b', name: 'B' },
      ],
      pinnedTasks: [],
      tasksByWorkspace: {},
      lastActiveWorkspaceId: 'project-a',
    }, 'project-b');

    expect(sidebar.lastActiveWorkspaceId).toBe('project-b');
    expect(sidebar.workspaces.map((workspace) => workspace.projectId)).toEqual(['project-b', 'project-a']);
  });

  it('enables run stop only for running runs', () => {
    expect(canPauseConversationSidebarRun({ status: 'running' })).toBe(true);
    expect(canPauseConversationSidebarRun({ status: 'paused' })).toBe(false);
    expect(canPauseConversationSidebarRun({ status: 'completed' })).toBe(false);
  });

  it('opens stop context menu only for concrete run rows', () => {
    expect(canOpenConversationSidebarRunMenu('run')).toBe(true);
    expect(canOpenConversationSidebarRunMenu('task')).toBe(false);
  });

  it('routes run stop menu selection to pause callback only when running', () => {
    const onPauseRun = vi.fn();

    expect(selectConversationSidebarRunPauseAction({ runId: 'run-001', status: 'running' }, onPauseRun)).toBe(true);
    expect(selectConversationSidebarRunPauseAction({ runId: 'run-002', status: 'paused' }, onPauseRun)).toBe(false);

    expect(onPauseRun).toHaveBeenCalledTimes(1);
    expect(onPauseRun).toHaveBeenCalledWith('run-001');
  });
});
