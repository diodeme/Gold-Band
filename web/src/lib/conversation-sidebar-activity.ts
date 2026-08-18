import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
  ConversationTerminalResultVm,
} from '@/types';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm, ConversationTerminalResultUpdatedEventVm } from '@/api/client';

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
  if (event.activity !== undefined) {
    return event.activity ?? null;
  }
  return event.lifecycle
    ? conversationTaskActivityFromLifecycle(event.lifecycle)
    : undefined;
}

export function applyConversationSidebarTaskActivity(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  activity: ConversationTaskActivityVm | null,
): ConversationSidebarVm {
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== projectId || task.taskId !== taskId) return task;
    const currentActivity = task.activity ?? null;
    if (
      currentActivity === activity
      || (
        currentActivity !== null
        && activity !== null
        && currentActivity.phase === activity.phase
        && currentActivity.stopping === activity.stopping
      )
    ) {
      return task;
    }
    return { ...task, activity };
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
