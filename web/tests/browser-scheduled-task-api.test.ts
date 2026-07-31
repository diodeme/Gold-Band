import { describe, expect, it } from 'vitest';
import { browserApi } from '@/api/browser';

const input = (content: string) => ({
  projectId: 'default',
  content,
  runMode: 'direct' as const,
  directConfig: { agentType: 'claude-acp' },
  schedule: { kind: 'At' as const, at: '2026-08-01T01:00:00.000Z', timezone: 'Asia/Shanghai' },
  overlapPolicy: 'skip_when_running' as const,
});

describe('browser scheduled task API', () => {
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
      schedule: edit.schedule,
      overlapPolicy: edit.overlapPolicy,
      sessionPolicy: edit.sessionPolicy,
    });

    expect(updated.content).toBe('edited content');
    await browserApi.deleteScheduledTask('default', first.id);
    expect((await browserApi.listScheduledTasks('default')).some((task) => task.id === first.id)).toBe(false);
  });
});
