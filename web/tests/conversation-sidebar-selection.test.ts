import { describe, expect, it, vi } from 'vitest';
import {
  canOpenConversationSidebarRunMenu,
  canPauseConversationSidebarRun,
  conversationSidebarIdentityKind,
  conversationSidebarNavigationKey,
  conversationSidebarRunKey,
  conversationSidebarTaskKey,
  isConversationSidebarRunListScopeActive,
  isConversationSidebarRunActive,
  prioritizeConversationSidebarWorkspace,
  selectConversationSidebarRunPauseAction,
  shouldShowConversationSidebarRunList,
  shouldShowConversationSidebarActivity,
  conversationSidebarActivityIconClass,
  conversationSidebarRunStatusClass,
  updateConversationSidebarExpandedTaskKeys,
} from '@/components/conversation/ConversationSidebar';
import {
  applyConversationSidebarRunLifecycle,
  applyConversationSidebarRunStateUpdate,
  applyConversationSidebarTaskActivity,
  conversationTaskActivityFromLifecycle,
  conversationTaskActivityFromUpdate,
} from '@/lib/conversation-sidebar-activity';

describe('ConversationSidebar run selection identity', () => {
  it('selects quick chat while authoring a new scheduled task', () => {
    expect(conversationSidebarNavigationKey({ kind: 'conversation-home' })).toBe('quick-chat');
    expect(conversationSidebarNavigationKey({ kind: 'scheduled-task-create' })).toBe('quick-chat');
    expect(conversationSidebarNavigationKey({ kind: 'scheduled-tasks' })).toBe('scheduled-tasks');
    expect(conversationSidebarNavigationKey({
      kind: 'scheduled-task-detail',
      projectId: 'project-a',
      scheduledTaskId: 'scheduled-a',
    })).toBe('scheduled-tasks');
  });

  it('uses a reduced-motion-safe breathing effect for active Direct Agent icons', () => {
    expect(conversationSidebarActivityIconClass).toContain('motion-safe:animate-pulse');
    expect(conversationSidebarActivityIconClass).not.toContain('animate-spin');
  });

  it('uses a blue breathing dot only for running workflow sessions', () => {
    expect(conversationSidebarRunStatusClass({ status: 'running', outcome: null })).toContain('bg-gold-running');
    expect(conversationSidebarRunStatusClass({ status: 'running', outcome: null })).toContain('motion-safe:animate-pulse');
    expect(conversationSidebarRunStatusClass({ status: 'paused', outcome: null })).toBe('bg-yellow-500/50');
    expect(conversationSidebarRunStatusClass({ status: 'completed', outcome: 'success' })).toBe('bg-emerald-500/50');
  });

  it('uses Agent identity for Direct tasks and runtime status for other modes', () => {
    expect(conversationSidebarIdentityKind({
      runMode: 'direct',
      agentIdentity: { agentType: 'codex-acp', displayName: 'Codex', iconKey: 'codex' },
    })).toBe('agent-icon');
    expect(conversationSidebarIdentityKind({ runMode: 'workflow', agentIdentity: null })).toBe('runtime-status');
    expect(conversationSidebarIdentityKind({ runMode: 'auto', agentIdentity: null })).toBe('runtime-status');
  });

  it('shows activity around the Direct Agent identity only while a canonical task activity exists', () => {
    const direct = {
      runMode: 'direct' as const,
      agentIdentity: { agentType: 'codex-acp', displayName: 'Codex', iconKey: 'codex' },
    };
    expect(shouldShowConversationSidebarActivity({ ...direct, activity: { phase: 'running', stopping: false } })).toBe(true);
    expect(shouldShowConversationSidebarActivity({ ...direct, activity: null })).toBe(false);
    expect(shouldShowConversationSidebarActivity({
      runMode: 'workflow',
      agentIdentity: null,
      activity: { phase: 'runtime-active', stopping: false },
    })).toBe(false);
  });

  it('maps canonical lifecycle into both workspace and pinned sidebar copies', () => {
    const task = {
      projectId: 'project-a',
      taskId: 'task-a',
      title: 'Direct task',
      autoTitle: false,
      runMode: 'direct' as const,
      runs: [],
      pinned: true,
    };
    const lifecycle = {
      runtime: { status: 'completed', resumable: false, current: true, active: false, continuable: false, phase: 'terminal' },
      control: { mode: 'non-runtime-controlled' as const },
      acp: { sessionAvailability: 'established' as const, liveTurnActivity: 'running' as const, latestTurnStatus: 'none' as const, stopping: false },
      displayStatus: 'running',
      runtimeDisplay: { code: 'running', tone: 'running', icon: 'dot', terminal: false, resumable: false, reasonCode: null, blockingError: false },
      continueKind: null,
      composer: { mode: 'runtime-active', submitTarget: 'none', processingKind: 'processing', statusKey: null, canStop: true, lockInput: true },
    };
    const activity = conversationTaskActivityFromLifecycle(lifecycle);
    const sidebar = applyConversationSidebarTaskActivity({
      workspaces: [],
      pinnedTasks: [task],
      tasksByWorkspace: { 'project-a': [task] },
    }, 'project-a', 'task-a', activity);

    expect(sidebar.pinnedTasks[0].activity).toEqual({ phase: 'running', stopping: false });
    expect(sidebar.tasksByWorkspace['project-a'][0].activity).toEqual({ phase: 'running', stopping: false });
  });

  it('projects a continued runtime into both task and run sidebar dots immediately', () => {
    const run = {
      runId: 'run-001',
      status: 'paused',
      outcome: null,
      startedAt: '2026-08-12T00:00:00Z',
      updatedAt: '2026-08-12T00:01:00Z',
      resumable: true,
    };
    const task = {
      projectId: 'project-a',
      taskId: 'task-a',
      title: 'Workflow task',
      autoTitle: false,
      runMode: 'workflow' as const,
      latestRun: run,
      runs: [run],
      pinned: true,
    };
    const lifecycle = {
      runtime: { status: 'running', outcome: null, pauseReason: null, resumable: false, current: true, active: true, continuable: false, phase: 'runtime-active' },
      control: { mode: 'runtime-controlled' as const },
      acp: { sessionAvailability: 'established' as const, liveTurnActivity: 'starting' as const, latestTurnStatus: 'none' as const, stopping: false },
      displayStatus: 'running',
      runtimeDisplay: { code: 'running', tone: 'running', icon: 'dot', terminal: false, resumable: false, reasonCode: null, blockingError: false },
      continueKind: null,
      composer: { mode: 'runtime-active', submitTarget: 'none', processingKind: 'launching', statusKey: null, canStop: true, lockInput: true },
    };

    const sidebar = applyConversationSidebarRunLifecycle({
      workspaces: [],
      pinnedTasks: [task],
      tasksByWorkspace: { 'project-a': [task] },
    }, 'project-a', 'task-a', 'run-001', lifecycle);

    expect(sidebar.pinnedTasks[0].latestRun?.status).toBe('running');
    expect(sidebar.pinnedTasks[0].runs[0].status).toBe('running');
    expect(sidebar.tasksByWorkspace['project-a'][0].latestRun?.status).toBe('running');
    expect(sidebar.tasksByWorkspace['project-a'][0].runs[0].resumable).toBe(false);
  });

  it('projects a background terminal run across workspaces without replacing unrelated sidebar data', () => {
    const workspaceA = { projectId: 'project-a', workspacePath: '/a', name: 'A' };
    const workspaceB = { projectId: 'project-b', workspacePath: '/b', name: 'B' };
    const runA = {
      runId: 'run-001',
      status: 'running',
      outcome: null,
      startedAt: '2026-08-17T00:00:00Z',
      updatedAt: '2026-08-17T00:01:00Z',
      resumable: false,
    };
    const runB = { ...runA };
    const taskA = {
      projectId: 'project-a',
      taskId: 'task-001',
      title: 'Workspace A task',
      autoTitle: false,
      runMode: 'workflow' as const,
      latestRun: runA,
      runs: [runA],
      pinned: false,
    };
    const taskB = {
      projectId: 'project-b',
      taskId: 'task-001',
      title: 'Workspace B task',
      autoTitle: false,
      runMode: 'workflow' as const,
      latestRun: runB,
      runs: [runB],
      pinned: true,
    };
    const sidebar = {
      workspaces: [workspaceA, workspaceB],
      pinnedTasks: [taskB],
      tasksByWorkspace: {
        'project-a': [taskA],
        'project-b': [taskB],
      },
      preferences: { density: 'compact' },
    };

    const next = applyConversationSidebarRunStateUpdate(sidebar, {
      projectId: 'project-b',
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'accept',
      attemptId: 'attempt-001',
      status: 'completed',
      outcome: 'success',
    });

    expect(next).not.toBe(sidebar);
    expect(next.workspaces).toBe(sidebar.workspaces);
    expect(next.preferences).toBe(sidebar.preferences);
    expect(next.tasksByWorkspace['project-a']).toBe(sidebar.tasksByWorkspace['project-a']);
    expect(next.tasksByWorkspace['project-a'][0]).toBe(taskA);
    expect(next.tasksByWorkspace['project-b'][0].latestRun).toMatchObject({
      status: 'completed',
      outcome: 'success',
      updatedAt: runB.updatedAt,
    });
    expect(next.pinnedTasks[0].runs[0]).toMatchObject({ status: 'completed', outcome: 'success' });

    const stale = applyConversationSidebarRunStateUpdate(next, {
      projectId: 'project-b',
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'accept',
      attemptId: 'attempt-001',
      status: 'running',
      outcome: null,
    });
    expect(stale).toBe(next);
  });


  it('projects lightweight ACP activity without requiring a lifecycle snapshot and clears it explicitly', () => {
    const event = {
      taskId: 'task-a',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'direct-agent',
      attemptId: 'attempt-001',
    };

    expect(conversationTaskActivityFromUpdate({
      ...event,
      activity: { phase: 'running', stopping: false },
    })).toEqual({ phase: 'running', stopping: false });
    expect(conversationTaskActivityFromUpdate({
      ...event,
      activity: null,
    })).toBeNull();
    expect(conversationTaskActivityFromUpdate(event)).toBeUndefined();
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
