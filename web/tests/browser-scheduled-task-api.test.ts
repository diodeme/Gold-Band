import { describe, expect, it } from 'vitest';
import { browserApi } from '@/api/browser';

const scheduleInput = {
  kind: 'At' as const,
  localDate: '2099-08-01',
  localTime: '09:00',
  timezone: 'Asia/Shanghai',
  disambiguation: 'earlier' as const,
};

const input = (content: string) => ({
  projectId: 'default',
  content,
  runMode: 'direct' as const,
  directConfig: { agentType: 'claude-acp' },
  schedule: scheduleInput,
  overlapPolicy: 'skip_when_running' as const,
});

describe('browser scheduled task API', () => {
  it('freezes the selected workflow optional entry default in the definition', async () => {
    const projectId = `browser-scheduled-workflow-${Date.now()}`;
    const task = await browserApi.createScheduledTask({
      ...input('workflow task'),
      projectId,
      runMode: 'workflow',
      directConfig: undefined,
      workflowTemplateId: 'default-lightweight',
    });

    const edit = await browserApi.getScheduledTask(projectId, task.id);
    expect(edit.includeOptionalEntry).toBe(true);

    const updated = await browserApi.updateScheduledTask({
      scheduledTaskId: task.id,
      projectId,
      expectedUpdatedAt: edit.expectedUpdatedAt,
      content: edit.content,
      runMode: 'workflow',
      workflowTemplateId: 'default-lightweight',
      includeOptionalEntry: false,
      schedule: scheduleInput,
      overlapPolicy: edit.overlapPolicy,
      sessionPolicy: edit.sessionPolicy,
    });
    expect(updated.includeOptionalEntry).toBe(false);
  });

  it('keeps multiple scheduled task definitions instead of replacing the previous one', async () => {
    const first = await browserApi.createScheduledTask(input('first task'));
    const second = await browserApi.createScheduledTask(input('second task'));

    const tasks = await browserApi.listScheduledTasks('default');

    expect(tasks.map((task) => task.id)).toEqual([first.id, second.id]);
    expect(new Set(tasks.map((task) => task.id)).size).toBe(2);

    const paused = await browserApi.setScheduledTaskEnabled('default', first.id, false);
    expect(paused.enabled).toBe(false);
    expect((await browserApi.listScheduledTasks('default')).find((task) => task.id === second.id)?.enabled).toBe(true);
  });

  it('supports reading, updating, and deleting one definition without affecting another', async () => {
    const first = await browserApi.createScheduledTask(input('edit me'));
    const edit = await browserApi.getScheduledTask('default', first.id);
    const updated = await browserApi.updateScheduledTask({
      scheduledTaskId: first.id,
      projectId: 'default',
      expectedUpdatedAt: edit.expectedUpdatedAt,
      content: 'edited content',
      runMode: edit.runMode,
      directConfig: { agentType: 'claude-acp' },
      schedule: scheduleInput,
      overlapPolicy: edit.overlapPolicy,
      sessionPolicy: edit.sessionPolicy,
    });

    expect(updated.content).toBe('edited content');
    await browserApi.deleteScheduledTask('default', first.id);
    expect((await browserApi.listScheduledTasks('default')).some((task) => task.id === first.id)).toBe(false);
  });

  it('keeps manual run occurrences and exposes diagnostics', async () => {
    const task = await browserApi.createScheduledTask(input('run me now'));

    const updates: string[] = [];
    const unlisten = await browserApi.subscribeScheduledOccurrenceUpdates?.((event) => {
      updates.push(event.status);
    });
    const result = await browserApi.runScheduledTaskNow('default', task.id);
    const occurrences = await browserApi.listScheduledTaskOccurrences('default', task.id);
    const diagnostics = await browserApi.getScheduledTaskDiagnostics('default', task.id);
    unlisten?.();

    expect(result.occurrence.triggerKind).toBe('manual');
    expect(result.occurrence.status).toBe('succeeded');
    expect(occurrences[0]?.id).toBe(result.occurrence.id);
    expect(diagnostics.runCount).toBe(1);
    expect(diagnostics.occurrences[0]?.status).toBe('succeeded');
    expect(updates).toContain('running');
    expect(updates).toContain('succeeded');
  });
});
