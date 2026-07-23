import { describe, expect, it, vi } from 'vitest';
import {
  canOpenConversationSidebarRunMenu,
  canPauseConversationSidebarRun,
  conversationSidebarIdentityKind,
  conversationSidebarRunKey,
  conversationSidebarTaskKey,
  isConversationSidebarRunListScopeActive,
  isConversationSidebarRunActive,
  prioritizeConversationSidebarWorkspace,
  selectConversationSidebarRunPauseAction,
  shouldShowConversationSidebarRunList,
  updateConversationSidebarExpandedTaskKeys,
} from '@/components/conversation/ConversationSidebar';

describe('ConversationSidebar run selection identity', () => {
  it('uses Agent identity for Direct tasks and runtime status for other modes', () => {
    expect(conversationSidebarIdentityKind({
      runMode: 'direct',
      agentIdentity: { agentType: 'codex-acp', displayName: 'Codex', iconKey: 'codex' },
    })).toBe('agent-icon');
    expect(conversationSidebarIdentityKind({ runMode: 'workflow', agentIdentity: null })).toBe('runtime-status');
    expect(conversationSidebarIdentityKind({ runMode: 'auto', agentIdentity: null })).toBe('runtime-status');
  });
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

  it('shows the run list for a task as soon as it has one run', () => {
    expect(shouldShowConversationSidebarRunList({ runMode: 'workflow', runs: [] })).toBe(false);
    expect(shouldShowConversationSidebarRunList({ runMode: 'workflow', runs: [{ runId: 'run-001' }] })).toBe(true);
    expect(shouldShowConversationSidebarRunList({ runMode: 'auto', runs: [{ runId: 'run-002' }, { runId: 'run-001' }] })).toBe(true);
  });

  it('keeps Direct as one continuous conversation without run rows', () => {
    expect(shouldShowConversationSidebarRunList({ runMode: 'direct', runs: [{ runId: 'run-001' }] })).toBe(false);
    expect(shouldShowConversationSidebarRunList({ runMode: 'direct', runs: [{ runId: 'run-002' }, { runId: 'run-001' }] })).toBe(false);
  });

  it('keeps pinned and workspace run-list expansion independent', () => {
    const taskA = conversationSidebarTaskKey('project-a', 'task-a');
    const taskB = conversationSidebarTaskKey('project-a', 'task-b');
    const taskC = conversationSidebarTaskKey('project-a', 'task-c');

    const pinnedExpanded = updateConversationSidebarExpandedTaskKeys(
      { pinned: null, workspace: null },
      'pinned',
      taskA,
      'expand',
    );
    expect(pinnedExpanded).toEqual({ pinned: taskA, workspace: null });

    const workspaceExpanded = updateConversationSidebarExpandedTaskKeys(
      pinnedExpanded,
      'workspace',
      taskB,
      'expand',
    );
    expect(workspaceExpanded).toEqual({ pinned: taskA, workspace: taskB });

    const workspaceReplaced = updateConversationSidebarExpandedTaskKeys(
      workspaceExpanded,
      'workspace',
      taskC,
      'expand',
    );
    expect(workspaceReplaced).toEqual({ pinned: taskA, workspace: taskC });

    expect(updateConversationSidebarExpandedTaskKeys(workspaceReplaced, 'pinned', taskA, 'toggle')).toEqual({
      pinned: null,
      workspace: taskC,
    });
  });

  it('keeps selected run-list highlight scoped to the interaction area', () => {
    expect(isConversationSidebarRunListScopeActive('pinned', 'pinned')).toBe(true);
    expect(isConversationSidebarRunListScopeActive('workspace', 'workspace')).toBe(true);
    expect(isConversationSidebarRunListScopeActive('pinned', 'workspace')).toBe(false);
    expect(isConversationSidebarRunListScopeActive('workspace', 'pinned')).toBe(false);
  });

  it('routes run stop menu selection to pause callback only when running', () => {
    const onPauseRun = vi.fn();

    expect(selectConversationSidebarRunPauseAction({ runId: 'run-001', status: 'running' }, onPauseRun)).toBe(true);
    expect(selectConversationSidebarRunPauseAction({ runId: 'run-002', status: 'paused' }, onPauseRun)).toBe(false);

    expect(onPauseRun).toHaveBeenCalledTimes(1);
    expect(onPauseRun).toHaveBeenCalledWith('run-001');
  });
});
