import { describe, expect, it } from 'vitest';

import {
  scheduledNotificationCopy,
  scheduledNotificationNavigation,
} from '../src/lib/scheduled-task-notifications';
import type { ScheduledNotificationEventVm, ScheduledViewActionPayload } from '../src/types';

const translate = (key: string, params?: Record<string, unknown>) =>
  params ? `${key}:${JSON.stringify(params)}` : key;

function event(kind: ScheduledNotificationEventVm['kind']): ScheduledNotificationEventVm {
  return {
    eventId: `event-${kind}`,
    kind,
    projectId: 'project-a',
    scheduledTaskId: 'scheduled-a',
    occurrenceId: 'occurrence-a',
    errorCode: kind === 'failed' ? 'SCHEDULED_EXECUTION_FAILED' : null,
    errorParams: null,
    links: { taskId: 'task-a', runId: 'run-a', roundId: 'round-a', attemptId: 'attempt-a' },
    missedCount: kind === 'missed' ? 3 : null,
  };
}

describe('scheduled task notifications', () => {
  it('localizes each structured notification kind without backend copy', () => {
    expect(scheduledNotificationCopy(event('completion'), translate)).toEqual({
      title: 'scheduled.notifications.completion.title',
      body: 'scheduled.notifications.completion.body',
    });
    expect(scheduledNotificationCopy(event('failed'), translate).body).toContain(
      'SCHEDULED_EXECUTION_FAILED',
    );
    expect(scheduledNotificationCopy(event('missed'), translate).body).toContain('"count":3');
  });

  it('routes failed notifications to detail and resumable outcomes to linked runs', () => {
    const payload: ScheduledViewActionPayload = {
      kind: 'failed',
      projectId: 'project-a',
      scheduledTaskId: 'scheduled-a',
      occurrenceId: 'occurrence-a',
      taskId: 'task-a',
      runId: 'run-a',
      roundId: 'round-a',
      attemptId: 'attempt-a',
      dedupKey: 'scheduled:occurrence-a:failed',
    };

    expect(scheduledNotificationNavigation(payload)).toEqual({
      kind: 'scheduled-detail',
      projectId: 'project-a',
      scheduledTaskId: 'scheduled-a',
    });
    expect(scheduledNotificationNavigation({ ...payload, kind: 'attentionRequired' })).toEqual({
      kind: 'run',
      projectId: 'project-a',
      taskId: 'task-a',
      runId: 'run-a',
      roundId: 'round-a',
      attemptId: 'attempt-a',
    });
  });
});
