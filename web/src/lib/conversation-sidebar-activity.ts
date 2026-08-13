import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
} from '@/types';
import type { AcpSessionUpdatedEventVm } from '@/api/client';

export function conversationTaskActivityFromLifecycle(
  lifecycle: ConversationAttemptLifecycleVm,
): ConversationTaskActivityVm | null {
  if (!lifecycle.runtime.active && !lifecycle.acp.active && !lifecycle.acp.stopping) {
    return null;
  }
  return {
    phase: lifecycle.acp.stopping
      ? 'cancel-requested'
      : lifecycle.acp.phase ?? lifecycle.runtime.phase ?? lifecycle.composer.processingKind,
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
  const updateTask = (task: ConversationTaskRowVm) => (
    task.projectId === projectId && task.taskId === taskId
      ? { ...task, activity }
      : task
  );
  return {
    ...sidebar,
    pinnedTasks: sidebar.pinnedTasks.map(updateTask),
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [projectId]: (sidebar.tasksByWorkspace[projectId] ?? []).map(updateTask),
    },
  };
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
