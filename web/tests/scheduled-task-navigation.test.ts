import { describe, expect, it } from 'vitest';

import { scheduledHistoryTarget, scheduledOccurrenceTarget } from '@/lib/scheduled-task-navigation';
import { pathFromRoute, routeFromPath } from '@/routes';
import { formatScheduledSchedule, scheduledScheduleTimezone } from '@/lib/scheduled-task-formatting';
import i18n from '@/i18n';
import type { ScheduledOccurrenceVm, ScheduledScheduleSpec } from '@/types';

const occurrence = (overrides: Partial<ScheduledOccurrenceVm> = {}): ScheduledOccurrenceVm => ({
  id: 'occurrence-1',
  scheduledTaskId: 'scheduled-1',
  scheduledAt: '2026-08-07T09:00:00Z',
  triggerKind: 'scheduled',
  status: 'succeeded',
  attempt: 1,
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  attemptId: 'attempt-1',
  ...overrides,
});

describe('scheduled task navigation', () => {
  it('keeps task, run, round, and attempt links in the conversation target', () => {
    expect(scheduledOccurrenceTarget('project-1', occurrence())).toEqual({
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      attemptId: 'attempt-1',
    });
  });

  it('does not create a partial target without both task and run', () => {
    expect(scheduledOccurrenceTarget('project-1', occurrence({ runId: null }))).toBeNull();
  });

  it('round-trips the complete Run and occurrence locator through a deep link', () => {
    const target = scheduledHistoryTarget({ projectId: 'project-1', scheduledTaskId: 'scheduled-1', taskId: 'task-1', runId: 'run-1', latestOccurrenceId: 'occurrence-1' } as never);
    const path = pathFromRoute('task-orchestration', { kind: 'task-list' }, target);
    expect(path).toBe('/chat/projects/project-1/scheduled-tasks/scheduled-1/history/task-1/run-1/occurrences/occurrence-1');
    expect(routeFromPath(path).conversationPage).toEqual({ kind: 'scheduled-task-detail', projectId: 'project-1', scheduledTaskId: 'scheduled-1', taskId: 'task-1', runId: 'run-1', occurrenceId: 'occurrence-1' });
  });

  it('keeps the project scope in normal scheduled task detail routes', () => {
    const target = { kind: 'scheduled-task-detail', projectId: 'project-1', scheduledTaskId: 'scheduled-1' } as const;
    const path = pathFromRoute('task-orchestration', { kind: 'task-list' }, target);
    expect(path).toBe('/chat/projects/project-1/scheduled-tasks/scheduled-1');
    expect(routeFromPath(path).conversationPage).toEqual(target);
  });

  it('does not accept the removed unscoped scheduled task detail route', () => {
    expect(routeFromPath('/chat/scheduled-tasks/scheduled-1').conversationPage).toEqual({ kind: 'scheduled-tasks' });
  });
});

describe('scheduled task formatting', () => {
  it('derives localized schedule labels and timezone from the typed schedule', async () => {
    const schedule: ScheduledScheduleSpec = {
      kind: 'Repeat',
      preset: 'Daily',
      hour: 9,
      minute: 5,
      timezone: 'Asia/Shanghai',
    };

    await i18n.changeLanguage('zh-CN');
    const chinese = formatScheduledSchedule(i18n.t, schedule);
    await i18n.changeLanguage('en');
    const english = formatScheduledSchedule(i18n.t, schedule);

    expect(chinese).toBe('每天 09:05');
    expect(english).toBe('Daily at 09:05');
    expect(scheduledScheduleTimezone(schedule)).toBe('Asia/Shanghai');
  });
});
