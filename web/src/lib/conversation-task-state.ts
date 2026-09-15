import type { ConversationSidebarVm, ConversationTaskRowVm } from '@/types';

function sameTask(
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId'>,
  projectId: string,
  taskId: string,
) {
  return task.projectId === projectId && task.taskId === taskId;
}

export function findConversationTask(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
) {
  return sidebar.tasksByWorkspace[projectId]?.find((task) => sameTask(task, projectId, taskId))
    ?? sidebar.pinnedTasks.find((task) => sameTask(task, projectId, taskId))
    ?? null;
}

export function applyConversationTaskSnapshot(
  sidebar: ConversationSidebarVm,
  task: ConversationTaskRowVm,
): ConversationSidebarVm {
  const workspaceTasks = sidebar.tasksByWorkspace[task.projectId] ?? [];
  const taskIndex = workspaceTasks.findIndex((candidate) => sameTask(candidate, task.projectId, task.taskId));
  const existing = taskIndex >= 0
    ? workspaceTasks[taskIndex]
    : sidebar.pinnedTasks.find((candidate) => sameTask(candidate, task.projectId, task.taskId));
  const mergedTask = existing && task.runHistoryStatus === 'not-loaded'
    ? {
        ...task,
        runs: existing.runs,
        runHistoryStatus: existing.runHistoryStatus,
        runsNextCursor: existing.runsNextCursor,
      }
    : task;
  const nextWorkspaceTasks = taskIndex >= 0
    ? workspaceTasks.map((candidate, index) => index === taskIndex ? mergedTask : candidate)
    : [mergedTask, ...workspaceTasks];

  const remainingPinnedTasks = sidebar.pinnedTasks
    .filter((candidate) => !sameTask(candidate, task.projectId, task.taskId));
  const nextPinnedTasks = mergedTask.pinned
    ? [...remainingPinnedTasks, mergedTask].sort((left, right) =>
        (left.pinnedOrder ?? Number.MAX_SAFE_INTEGER) - (right.pinnedOrder ?? Number.MAX_SAFE_INTEGER))
    : remainingPinnedTasks;

  return {
    ...sidebar,
    pinnedTasks: nextPinnedTasks,
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [task.projectId]: nextWorkspaceTasks,
    },
  };
}
