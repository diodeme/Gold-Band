import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
  ConversationTerminalResultVm,
} from '@/types';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm, ConversationTerminalResultUpdatedEventVm } from '@/api/client';
import { parseTimestamp } from '@/lib/datetime';

function isTerminalConversationRunStatus(status: string) {
  return ['completed', 'failed', 'cancelled', 'killed'].includes(status.trim().toLowerCase());
}

export function conversationTaskActivityFromLifecycle(
  lifecycle: ConversationAttemptLifecycleVm,
): ConversationTaskActivityVm | null {
  if (!lifecycle.runtime.active && lifecycle.acp.liveTurnActivity === 'idle' && !lifecycle.acp.stopping) {
    return null;
  }
  return {
    phase: lifecycle.acp.stopping
      ? 'cancel-requested'
      : lifecycle.acp.liveTurnActivity !== 'idle'
        ? lifecycle.acp.liveTurnActivity
        : lifecycle.runtime.phase ?? lifecycle.composer.processingKind,
    stopping: lifecycle.acp.stopping,
  };
}

export function conversationTaskActivityFromUpdate(
  event: AcpSessionUpdatedEventVm,
): ConversationTaskActivityVm | null | undefined {
  const lifecycleActivity = event.lifecycle
    ? conversationTaskActivityFromLifecycle(event.lifecycle)
    : undefined;
  // A quiescent canonical lifecycle is terminal for the task activity
  // projection. A task-wide lightweight activity sample can be captured
  // before the attempt control is released, so it must not resurrect the
  // breathing indicator after the same update has settled the lifecycle.
  if (lifecycleActivity === null) {
    return null;
  }
  if (event.activity !== undefined) {
    return event.activity ?? null;
  }
  return lifecycleActivity;
}

export function applyConversationSidebarTaskActivity(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  activity: ConversationTaskActivityVm | null,
  taskActivityAt?: string | null,
): ConversationSidebarVm {
  const activityTimeAdvanced = (task: ConversationTaskRowVm) => {
    if (!taskActivityAt || task.projectId !== projectId || task.taskId !== taskId) return false;
    const current = parseTimestamp(task.lastActivityAt)?.getTime() ?? Number.NEGATIVE_INFINITY;
    const incoming = parseTimestamp(taskActivityAt)?.getTime();
    return incoming !== undefined && incoming !== null && incoming > current;
  };
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== projectId || task.taskId !== taskId) return task;
    const currentActivity = task.activity ?? null;
    const timestampAdvanced = activityTimeAdvanced(task);
    if (
      !timestampAdvanced
      && (
        currentActivity === activity
        || (
          currentActivity !== null
          && activity !== null
          && currentActivity.phase === activity.phase
          && currentActivity.stopping === activity.stopping
        )
      )
    ) {
      return task;
    }
    return {
      ...task,
      activity,
      lastActivityAt: timestampAdvanced ? taskActivityAt : task.lastActivityAt,
    };
  };

  const pinnedTasks = sidebar.pinnedTasks.map(updateTask);
  const workspaceTasks = sidebar.tasksByWorkspace[projectId] ?? [];
  const shouldMoveToFront = workspaceTasks.some(activityTimeAdvanced);
  const updatedWorkspaceTasks = workspaceTasks.map(updateTask);
  const nextWorkspaceTasks = shouldMoveToFront
    ? [
        ...updatedWorkspaceTasks.filter((task) => task.projectId === projectId && task.taskId === taskId),
        ...updatedWorkspaceTasks.filter((task) => task.projectId !== projectId || task.taskId !== taskId),
      ]
    : updatedWorkspaceTasks;
  const pinnedChanged = pinnedTasks.some((task, index) => task !== sidebar.pinnedTasks[index]);
  const workspaceChanged = nextWorkspaceTasks.some((task, index) => task !== workspaceTasks[index]);
  if (!pinnedChanged && !workspaceChanged) return sidebar;

  return {
    ...sidebar,
    pinnedTasks: pinnedChanged ? pinnedTasks : sidebar.pinnedTasks,
    tasksByWorkspace: workspaceChanged
      ? {
          ...sidebar.tasksByWorkspace,
          [projectId]: nextWorkspaceTasks,
        }
      : sidebar.tasksByWorkspace,
  };
}

function sameTerminalResult(
  left: ConversationTerminalResultVm | null | undefined,
  right: ConversationTerminalResultVm | null | undefined,
) {
  if (!left || !right) return !left && !right;
  return left.eventId === right.eventId
    && left.runId === right.runId
    && left.kind === right.kind
    && left.occurredAt === right.occurredAt;
}

function applyConversationSidebarTaskTerminalResult(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  update: (current: ConversationTerminalResultVm | null) => ConversationTerminalResultVm | null,
): ConversationSidebarVm {
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== projectId || task.taskId !== taskId) return task;
    const current = task.unreadTerminalResult ?? null;
    const next = update(current);
    return sameTerminalResult(current, next) ? task : { ...task, unreadTerminalResult: next };
  };
  const pinnedTasks = sidebar.pinnedTasks.map(updateTask);
  const workspaceTasks = sidebar.tasksByWorkspace[projectId] ?? [];
  const nextWorkspaceTasks = workspaceTasks.map(updateTask);
  const pinnedChanged = pinnedTasks.some((task, index) => task !== sidebar.pinnedTasks[index]);
  const workspaceChanged = nextWorkspaceTasks.some((task, index) => task !== workspaceTasks[index]);
  if (!pinnedChanged && !workspaceChanged) return sidebar;
  return {
    ...sidebar,
    pinnedTasks: pinnedChanged ? pinnedTasks : sidebar.pinnedTasks,
    tasksByWorkspace: workspaceChanged
      ? { ...sidebar.tasksByWorkspace, [projectId]: nextWorkspaceTasks }
      : sidebar.tasksByWorkspace,
  };
}

export function applyConversationSidebarTerminalResultUpdate(
  sidebar: ConversationSidebarVm,
  event: ConversationTerminalResultUpdatedEventVm,
): ConversationSidebarVm {
  return applyConversationSidebarTaskTerminalResult(
    sidebar,
    event.projectId,
    event.taskId,
    () => event.unreadTerminalResult,
  );
}

export function applyConversationSidebarTerminalResultAcknowledgement(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  requestedEventId: string,
  unreadTerminalResult: ConversationTerminalResultVm | null | undefined,
): ConversationSidebarVm {
  return applyConversationSidebarTaskTerminalResult(sidebar, projectId, taskId, (current) => {
    if (current?.eventId !== requestedEventId) return current;
    return unreadTerminalResult ?? null;
  });
}

export function applyConversationSidebarRunLifecycle(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  runId: string,
  lifecycle: ConversationAttemptLifecycleVm,
): ConversationSidebarVm {
  const status = lifecycle.runtime.active ? 'running' : lifecycle.runtime.status;
  const updateRun = (run: ConversationTaskRowVm['runs'][number]) => {
    if (run.runId !== runId) return run;
    const outcome = lifecycle.runtime.outcome ?? null;
    if (
      run.status === status
      && (run.outcome ?? null) === outcome
      && run.resumable === lifecycle.runtime.resumable
    ) {
      return run;
    }
    return {
      ...run,
      status,
      outcome,
      resumable: lifecycle.runtime.resumable,
    };
  };
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== projectId || task.taskId !== taskId) return task;
    const runs = task.runs.map(updateRun);
    const latestRun = task.latestRun ? updateRun(task.latestRun) : task.latestRun;
    const runsChanged = runs.some((run, index) => run !== task.runs[index]);
    if (!runsChanged && latestRun === task.latestRun) return task;
    return { ...task, runs, latestRun };
  };
  return {
    ...sidebar,
    pinnedTasks: sidebar.pinnedTasks.map(updateTask),
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [projectId]: (sidebar.tasksByWorkspace[projectId] ?? []).map(updateTask),
    },
  };
}

export function applyConversationSidebarRunStateUpdate(
  sidebar: ConversationSidebarVm,
  event: ConversationRunStateUpdatedEventVm,
): ConversationSidebarVm {
  const nextOutcome = event.outcome ?? null;
  const updateRun = (run: ConversationTaskRowVm['runs'][number]) => {
    if (run.runId !== event.runId) return run;
    if (isTerminalConversationRunStatus(run.status) && !isTerminalConversationRunStatus(event.status)) {
      return run;
    }
    if (run.status === event.status && (run.outcome ?? null) === nextOutcome) return run;
    return {
      ...run,
      status: event.status,
      outcome: nextOutcome,
    };
  };
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== event.projectId || task.taskId !== event.taskId) return task;
    const runs = task.runs.map(updateRun);
    const latestRun = task.latestRun ? updateRun(task.latestRun) : task.latestRun;
    const runsChanged = runs.some((run, index) => run !== task.runs[index]);
    if (!runsChanged && latestRun === task.latestRun) return task;
    return { ...task, runs, latestRun };
  };

  const pinnedTasks = sidebar.pinnedTasks.map(updateTask);
  const workspaceTasks = sidebar.tasksByWorkspace[event.projectId] ?? [];
  const nextWorkspaceTasks = workspaceTasks.map(updateTask);
  const pinnedChanged = pinnedTasks.some((task, index) => task !== sidebar.pinnedTasks[index]);
  const workspaceChanged = nextWorkspaceTasks.some((task, index) => task !== workspaceTasks[index]);
  if (!pinnedChanged && !workspaceChanged) return sidebar;

  return {
    ...sidebar,
    pinnedTasks: pinnedChanged ? pinnedTasks : sidebar.pinnedTasks,
    tasksByWorkspace: workspaceChanged
      ? {
          ...sidebar.tasksByWorkspace,
          [event.projectId]: nextWorkspaceTasks,
        }
      : sidebar.tasksByWorkspace,
  };
}
