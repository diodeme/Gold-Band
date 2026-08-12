import type { ConversationPage, ScheduledOccurrenceVm } from '@/types';

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
