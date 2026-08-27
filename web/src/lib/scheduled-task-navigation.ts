import type { ConversationPage, ScheduledExecutionHistoryVm, ScheduledOccurrenceVm, ScheduledTriggerPayloadVm } from '@/types';

export function scheduledOccurrenceTarget(
  projectId: string,
  occurrence: ScheduledOccurrenceVm,
): ConversationPage | null {
  if (!occurrence.taskId || !occurrence.runId) return null;
  return {
    kind: 'conversation-run',
    projectId,
    taskId: occurrence.taskId,
    runId: occurrence.runId,
    roundId: occurrence.roundId ?? undefined,
    attemptId: occurrence.attemptId ?? undefined,
  };
}

export function scheduledHistoryTarget(history: ScheduledExecutionHistoryVm, occurrenceId = history.latestOccurrenceId): ConversationPage {
  return { kind: 'scheduled-task-detail', projectId: history.projectId, scheduledTaskId: history.scheduledTaskId, taskId: history.taskId, runId: history.runId, occurrenceId };
}

export function scheduledTriggerTarget(payload: ScheduledTriggerPayloadVm): ConversationPage | null {
  const taskId = payload.links.taskId;
  const runId = payload.links.runId;
  if (!taskId || !runId) return null;
  return { kind: 'scheduled-task-detail', projectId: payload.projectId, scheduledTaskId: payload.scheduledTaskId, taskId, runId, occurrenceId: payload.occurrenceId };
}
