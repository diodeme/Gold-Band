import { describe, expect, it } from 'vitest';
import { createDemoApi } from '../../marketing/demo/api';
import { demoPageFromHash } from '../../marketing/demo/routes';

describe('demo management read contracts', () => {
  it('routes management pages and schedule creation without a server rewrite', () => {
    for (const kind of ['multica-tasks', 'scheduled-tasks', 'scheduled-task-create']) expect(demoPageFromHash(`#${kind}`)).toEqual({ kind });
    expect(demoPageFromHash('#scheduled-task-detail?id=demo-weekly')).toEqual({ kind: 'scheduled-task-detail', projectId: 'default', scheduledTaskId: 'demo-weekly' });
  });
  it('provides isolated schedule definitions, filtered history and valid conversation links', async () => {
    const api = createDemoApi();
    const tasks = await api.listScheduledTasks(null);
    expect(tasks.map((item) => item.mode)).toEqual(['direct', 'workflow']);
    for (const task of tasks) {
      expect(await api.getScheduledTask('default', task.id)).toMatchObject({ schedule: task.schedule, runMode: task.mode });
      const page = await api.listScheduledTaskOccurrences('default', task.id, null, 'succeeded');
      expect(page.items).toHaveLength(2);
      for (const occurrence of page.items) expect(await api.getConversationRun('default', occurrence.taskId!, occurrence.runId!)).toBeTruthy();
      expect((await api.listScheduledTaskOccurrences('default', task.id, null, 'failed')).items).toEqual([]);
      expect((await api.listScheduledTaskOccurrences('default', task.id, page.items.at(-1)!.id, null)).items).toEqual([]);
      expect(await api.getScheduledTaskDiagnostics('default', task.id)).toMatchObject({ runCount: 2 });
    }
    tasks[0].title = 'changed';
    expect((await api.listScheduledTasks(null))[0].title).not.toBe('changed');
    await expect(api.getScheduledTask('other', tasks[0].id)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('exposes four requirement states and reads only the selected requirement', async () => {
    const api = createDemoApi();
    const board = await api.getMulticaTasks();
    const tasks = board.tasksByWorkspace[board.workspaces[0].id];
    expect(tasks.map((item) => item.status)).toEqual(['queued', 'running', 'completed', 'failed']);
    expect(tasks.every((item) => item.requirement === null)).toBe(true);
    expect((await api.getMulticaTaskRequirement(tasks[0].id, tasks[0].workspaceId)).requirement).toBe(tasks[0].title);
    await expect(api.getMulticaTaskRequirement(tasks[0].id, 'other')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('reads fake source control and run files while rejecting arbitrary paths and writes', async () => {
    const api = createDemoApi();
    expect(await api.getGitCapability('default')).toMatchObject({ status: 'ready' });
    expect((await api.getSourceControlSnapshot('default', '/default')).status.staged.length).toBeGreaterThan(0);
    const locator = { projectId: 'default', taskId: 'demo-review', runId: 'run-052', roundId: 'round-002', nodeId: 'dev-test', attemptId: 'attempt-001' };
    const root = await api.listConversationDirectory(locator);
    const files = await api.listConversationDirectory({ ...locator, relativePath: root[0].relativePath });
    expect(await api.readConversationDirectoryFile({ ...locator, relativePath: files[0].relativePath })).toMatchObject({ kind: 'text', editable: false });
    await expect(api.readConversationDirectoryFile({ ...locator, relativePath: '../../secret' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.listConversationDirectory({ ...locator, runId: 'other' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    for (const method of ['createScheduledTask', 'updateScheduledTask', 'deleteScheduledTask', 'setScheduledTaskEnabled', 'runScheduledTaskNow', 'cancelMulticaTask', 'disconnectMultica', 'setActiveMulticaWorkspace', 'executeGitMutation', 'startGitOperation', 'openConversationDirectoryPathInFileManager'] as const) {
      await expect(Reflect.apply(api[method], api, [])).rejects.toMatchObject({ code: 'demo.operation-unavailable' });
    }
  });
});
