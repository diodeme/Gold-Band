import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
} from '@/types';

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
