import type {
  ScheduledNotificationEventVm,
  ScheduledViewActionPayload,
} from '../types';

type Translate = (key: string, params?: Record<string, unknown>) => string;

export function scheduledNotificationCopy(
  event: ScheduledNotificationEventVm,
  translate: Translate,
): { title: string; body: string } {
  const prefix = `scheduled.notifications.${event.kind}`;
  const bodyParams = event.kind === 'failed'
    ? { code: event.errorCode ?? 'SCHEDULED_EXECUTION_FAILED' }
    : event.kind === 'missed'
      ? { count: event.missedCount ?? 0 }
      : undefined;
  return {
    title: translate(`${prefix}.title`),
    body: translate(`${prefix}.body`, bodyParams),
  };
}

export type ScheduledNotificationNavigation =
  | { kind: 'scheduled-detail'; projectId: string; scheduledTaskId: string }
  | {
      kind: 'run';
      projectId: string;
      taskId: string;
      runId: string;
      roundId?: string;
      attemptId?: string;
    };

export function scheduledNotificationNavigation(
  payload: ScheduledViewActionPayload,
): ScheduledNotificationNavigation {
  if (
    (payload.kind === 'attentionRequired' || payload.kind === 'completion')
    && payload.taskId
    && payload.runId
  ) {
    return {
      kind: 'run',
      projectId: payload.projectId,
      taskId: payload.taskId,
      runId: payload.runId,
      roundId: payload.roundId ?? undefined,
      attemptId: payload.attemptId ?? undefined,
    };
  }
  return {
    kind: 'scheduled-detail',
    projectId: payload.projectId,
    scheduledTaskId: payload.scheduledTaskId,
  };
}
