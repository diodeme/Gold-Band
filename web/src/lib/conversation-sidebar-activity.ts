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
