import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
  ConversationTerminalResultVm,
} from '@/types';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm, ConversationTerminalResultUpdatedEventVm } from '@/api/client';
import { parseTimestamp } from '@/lib/datetime';
import { findConversationTask } from '@/lib/conversation-task-state';
import { isDirectBackgroundActive, useDirectBackgroundActive } from '@/lib/acp-background-control';

export type ConversationSidebarRunStateRefreshTarget =
  | { kind: 'workspace-tasks'; projectId: string }
  | { kind: 'task-runs'; task: ConversationTaskRowVm };

export function conversationSidebarRunStateRefreshTarget(
  sidebar: ConversationSidebarVm,
  event: ConversationRunStateUpdatedEventVm,
  activeProjectId: string | null | undefined,
): ConversationSidebarRunStateRefreshTarget | null {
  if (!event.taskUuid) return null;
  const task = findConversationTask(sidebar, event.projectId, event.taskId);
  if (!task) {
    return event.projectId === activeProjectId
      ? { kind: 'workspace-tasks', projectId: event.projectId }
      : null;
  }
  if (task.taskUuid !== event.taskUuid) return null;
  const runIsLoaded = task.latestRun?.runId === event.runId
    || task.runs.some((run) => run.runId === event.runId);
  return runIsLoaded ? null : { kind: 'task-runs', task };
}

function isTerminalConversationRunStatus(status: string) {
  return ['completed', 'failed', 'cancelled', 'killed'].includes(status.trim().toLowerCase());
}

/** Background facts are kept as-is; whether they are still busy is decided at render time. */
const BACKGROUND_TASK_ACTIVITY_PHASE = 'background';

export function conversationTaskActivityFromLifecycle(
  lifecycle: ConversationAttemptLifecycleVm,
): ConversationTaskActivityVm | null {
  if (!lifecycle.runtime.active && lifecycle.acp.liveTurnActivity === 'idle' && !lifecycle.acp.stopping) {
    const backgroundControl = lifecycle.composer.backgroundControl;
    return backgroundControl
      ? { phase: BACKGROUND_TASK_ACTIVITY_PHASE, stopping: false, backgroundControl }
      : null;
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

/** A task is active while its prompt turn runs or its retained background is busy. */
export function isConversationTaskActive(activity: ConversationTaskActivityVm | null | undefined, now: number) {
  if (!activity) return false;
  return activity.phase !== BACKGROUND_TASK_ACTIVITY_PHASE
    || isDirectBackgroundActive(activity.backgroundControl, now);
}

/** {@link isConversationTaskActive} that re-renders when a background grace window expires. */
export function useConversationTaskActive(activity: ConversationTaskActivityVm | null | undefined, scope: string) {
  useDirectBackgroundActive(
    activity?.phase === BACKGROUND_TASK_ACTIVITY_PHASE ? activity.backgroundControl : null,
    scope,
  );
  return isConversationTaskActive(activity, Date.now());
}

function sameBackgroundControl(left: ConversationTaskActivityVm, right: ConversationTaskActivityVm) {
  const a = left.backgroundControl ?? null;
  const b = right.backgroundControl ?? null;
  return a === b || Boolean(
    a && b
    && a.sessionId === b.sessionId
    && a.connectionGeneration === b.connectionGeneration
    && a.activeTools === b.activeTools
    && a.expiresAtMs === b.expiresAtMs,
  );
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
  return projectConversationSidebarTask(sidebar, projectId, taskId, taskActivityAt, (task) => {
    const currentActivity = task.activity ?? null;
    const sameActivity = currentActivity === activity
      || (
        currentActivity !== null
        && activity !== null
        && currentActivity.phase === activity.phase
        && currentActivity.stopping === activity.stopping
        && sameBackgroundControl(currentActivity, activity)
      );
    return sameActivity ? task : { ...task, activity };
  });
}

// Projects one task-row change into pinned and workspace lists. A newer
// task activity time is the only reorder signal: the row's lastActivityAt
// advances and it moves to the front of its workspace list.
function projectConversationSidebarTask(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
  taskActivityAt: string | null | undefined,
  transform: (task: ConversationTaskRowVm) => ConversationTaskRowVm,
): ConversationSidebarVm {
  const activityTimeAdvanced = (task: ConversationTaskRowVm) => {
    if (!taskActivityAt || task.projectId !== projectId || task.taskId !== taskId) return false;
    const current = parseTimestamp(task.lastActivityAt)?.getTime() ?? Number.NEGATIVE_INFINITY;
    const incoming = parseTimestamp(taskActivityAt)?.getTime();
    return incoming !== undefined && incoming !== null && incoming > current;
  };
  const updateTask = (task: ConversationTaskRowVm) => {
    if (task.projectId !== projectId || task.taskId !== taskId) return task;
    const next = transform(task);
    return activityTimeAdvanced(task) ? { ...next, lastActivityAt: taskActivityAt } : next;
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
  taskActivityAt?: string | null,
): ConversationSidebarVm {
  return projectConversationSidebarTask(sidebar, projectId, taskId, taskActivityAt, (task) => {
    const current = task.unreadTerminalResult ?? null;
    const next = update(current);
    return sameTerminalResult(current, next) ? task : { ...task, unreadTerminalResult: next };
  });
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
    event.taskActivityAt,
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
  // An active runtime proves work is ongoing; an inactive attempt says nothing
  // about its siblings. Only run-state events may settle the aggregate.
  if (!lifecycle.runtime.active) return sidebar;
  const status = 'running';
  const updateRun = (run: ConversationTaskRowVm['runs'][number]) => {
    if (run.runId !== runId || isTerminalConversationRunStatus(run.status)) return run;
    const outcome = null;
    if (
      run.status === status
      && (run.outcome ?? null) === outcome
      && run.resumable === false
    ) {
      return run;
    }
    return {
      ...run,
      status,
      outcome,
      resumable: false,
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
  const pinnedTasks = sidebar.pinnedTasks.map(updateTask);
  const currentWorkspaceTasks = sidebar.tasksByWorkspace[projectId] ?? [];
  const workspaceTasks = currentWorkspaceTasks.map(updateTask);
  const pinnedTasksChanged = pinnedTasks.some(
    (task, index) => task !== sidebar.pinnedTasks[index],
  );
  const workspaceTasksChanged = workspaceTasks.some(
    (task, index) => task !== currentWorkspaceTasks[index],
  );
  if (!pinnedTasksChanged && !workspaceTasksChanged) return sidebar;
  return {
    ...sidebar,
    pinnedTasks: pinnedTasksChanged ? pinnedTasks : sidebar.pinnedTasks,
    tasksByWorkspace: workspaceTasksChanged
      ? {
          ...sidebar.tasksByWorkspace,
          [projectId]: workspaceTasks,
        }
      : sidebar.tasksByWorkspace,
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
