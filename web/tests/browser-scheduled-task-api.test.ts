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
    await expect(browserApi.deleteScheduledTask('another-project', first.id)).rejects.toMatchObject({ code: 'scheduled-task.not-found' });
    expect((await browserApi.listScheduledTasks('default')).some((task) => task.id === first.id)).toBe(true);
    await browserApi.deleteScheduledTask('default', first.id);
    expect((await browserApi.listScheduledTasks('default')).some((task) => task.id === first.id)).toBe(false);
  });

  it('keeps manual run occurrences and exposes diagnostics', async () => {
    const task = await browserApi.createScheduledTask(input('run me now'));

    const updates: string[] = [];
    let runningHistory: ReturnType<typeof browserApi.listScheduledExecutionHistory> | null = null;
    const unlisten = await browserApi.subscribeScheduledOccurrenceUpdates?.((event) => {
      updates.push(event.status);
      if (event.status === 'running') {
        runningHistory = browserApi.listScheduledExecutionHistory('default', task.id);
      }
    });
    const result = await browserApi.runScheduledTaskNow('default', task.id);
    const occurrencePage = await browserApi.listScheduledTaskOccurrences('default', task.id);
    const diagnostics = await browserApi.getScheduledTaskDiagnostics('default', task.id);
    unlisten?.();

    expect(result.occurrence.triggerKind).toBe('manual');
    expect(result.occurrence.status).toBe('succeeded');
    expect(occurrencePage.items[0]?.id).toBe(result.occurrence.id);
    expect(diagnostics.runCount).toBe(1);
    expect(diagnostics.occurrences[0]?.status).toBe('succeeded');
    expect(updates).toContain('running');
    expect(updates).toContain('succeeded');
    expect(runningHistory).not.toBeNull();
    expect((await runningHistory!).items[0]).toMatchObject({ taskId: result.taskId, runId: result.runId, run: { status: 'running', outcome: null } });
  });

  it('groups accepted occurrences by Run and retains history after definition deletion', async () => {
    const task = await browserApi.createScheduledTask(input('accepted history'));
    const first = await browserApi.runScheduledTaskNow('default', task.id);
    const history = await browserApi.listScheduledExecutionHistory('default', task.id);
    expect(history.items).toHaveLength(1);
    expect(history.items[0]).toMatchObject({ taskId: first.taskId, runId: first.runId, occurrenceCount: 1, latestOccurrenceId: first.occurrence.id });
    await browserApi.deleteScheduledTask('default', task.id);
    expect((await browserApi.listScheduledExecutionHistory('default', task.id)).items).toHaveLength(1);
    const rejected = await browserApi.deleteScheduledExecutionHistory([{ projectId: 'another-project', scheduledTaskId: task.id, taskId: first.taskId!, runId: first.runId!, throughOccurrenceId: first.occurrence.id }]);
    expect(rejected[0]).toMatchObject({ status: 'failed', code: 'SCHEDULED_NOT_FOUND' });
    expect((await browserApi.listScheduledExecutionHistory('default', task.id)).items).toHaveLength(1);
    const removal = { projectId: 'default', scheduledTaskId: task.id, taskId: first.taskId!, runId: first.runId!, throughOccurrenceId: first.occurrence.id };
    await browserApi.deleteScheduledExecutionHistory([removal]);
    expect((await browserApi.listScheduledExecutionHistory('default', task.id)).items).toHaveLength(0);
    expect((await browserApi.deleteScheduledExecutionHistory([removal]))[0]).toMatchObject({ status: 'completed' });
    expect(await browserApi.getConversationRun('default', first.taskId!, first.runId!)).toMatchObject({ projectId: 'default', taskId: first.taskId, runId: first.runId, runStatus: 'completed' });
  });

  it('keeps accepted history immutable when later executions use edited content', async () => {
    const task = await browserApi.createScheduledTask(input('accepted original'));
    await browserApi.runScheduledTaskNow('default', task.id);
    const originalHistory = await browserApi.listScheduledExecutionHistory('default', task.id);
    const originalFingerprint = originalHistory.items[0]?.latestContentFingerprint;

    const edit = await browserApi.getScheduledTask('default', task.id);
    await browserApi.updateScheduledTask({
      scheduledTaskId: task.id,
      projectId: 'default',
      expectedUpdatedAt: edit.expectedUpdatedAt,
      content: 'edited future execution',
      runMode: edit.runMode,
      directConfig: { agentType: 'claude-acp' },
      schedule: scheduleInput,
      overlapPolicy: edit.overlapPolicy,
      sessionPolicy: edit.sessionPolicy,
    });

    const afterEdit = await browserApi.listScheduledExecutionHistory('default', task.id);
    expect(afterEdit.items[0]).toMatchObject({ latestSummary: 'accepted original', latestContentFingerprint: originalFingerprint });

    await browserApi.runScheduledTaskNow('default', task.id);
    const afterNextRun = await browserApi.listScheduledExecutionHistory('default', task.id);
    expect(afterNextRun.items.map((item) => item.latestSummary)).toEqual(['edited future execution', 'accepted original']);
    expect(afterNextRun.items[0]?.latestContentFingerprint).not.toBe(originalFingerprint);
  });

  it('loads the page anchored at an older Run without scanning pages in the client', async () => {
    const task = await browserApi.createScheduledTask(input('anchored history'));
    const runs = [];
    for (let index = 0; index < 21; index += 1) runs.push(await browserApi.runScheduledTaskNow('default', task.id));
    const oldest = runs[0];

    const anchored = await browserApi.listScheduledExecutionHistory('default', task.id, null, { taskId: oldest.taskId!, runId: oldest.runId! });
    expect(anchored.items[0]).toMatchObject({ taskId: oldest.taskId, runId: oldest.runId });
    await expect(browserApi.listScheduledExecutionHistory('default', task.id, null, { taskId: 'missing-task', runId: 'missing-run' })).rejects.toMatchObject({ code: 'scheduled-task.not-found' });

    const deletion = await browserApi.deleteScheduledExecutionHistory([{
      projectId: 'default', scheduledTaskId: task.id, taskId: 'missing-task', runId: 'missing-run', throughOccurrenceId: 'missing-occurrence',
    }]);
    expect(deletion[0]).toMatchObject({ status: 'failed', code: 'SCHEDULED_NOT_FOUND' });
  });

  it('keeps cursor pages stable when a newer occurrence is inserted', async () => {
    const task = await browserApi.createScheduledTask(input('paged history'));
    for (let index = 0; index < 21; index += 1) {
      await browserApi.runScheduledTaskNow('default', task.id);
    }

    const first = await browserApi.listScheduledTaskOccurrences('default', task.id);
    await browserApi.runScheduledTaskNow('default', task.id);
    const second = await browserApi.listScheduledTaskOccurrences('default', task.id, first.nextCursor);

    expect(first.items).toHaveLength(20);
    expect(second.items).toHaveLength(1);
    expect(second.items.some((item) => first.items.some((firstItem) => firstItem.id === item.id))).toBe(false);
  });
});
