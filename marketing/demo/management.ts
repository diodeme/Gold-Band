import type { RuntimeApi } from '@/api/client';
import type { DesktopLanguage, RemoteTaskVm, ScheduledOccurrenceVm, ScheduledTaskVm } from '@/types';
import { DEMO_PROJECT_ID, DEMO_RUN_ID } from './fixtures';

const workspace = { id: 'demo-team', name: 'Gold Band', slug: 'gold-band', provider: 'multica' };
const timestamp = '2026-09-09T01:00:00Z';
const missing = () => { throw { code: 'demo.resource-not-found', params: {} }; };

export function demoManagementApi(language: () => DesktopLanguage): Partial<RuntimeApi> {
  function tasks(): ScheduledTaskVm[] {
    const en = language() === 'en';
    return [
      { id: 'demo-daily', title: en ? 'Daily project review' : '每日项目检查', mode: 'direct', enabled: true, sessionPolicy: 'continuous',
        schedule: { kind: 'Repeat', preset: 'Weekdays', hour: 9, minute: 0, timezone: 'Asia/Shanghai' }, status: 'enabled', nextAt: '2026-09-10T01:00:00Z' },
      { id: 'demo-weekly', title: en ? 'Weekly workflow review' : '每周工作流审阅', mode: 'workflow', enabled: false, sessionPolicy: 'new',
        schedule: { kind: 'Repeat', preset: { Weekly: { weekdays: ['Mon'] } }, hour: 10, minute: 0, timezone: 'Asia/Shanghai' }, status: 'paused', nextAt: null },
    ].map((task) => ({ ...task, projectId: DEMO_PROJECT_ID, workspaceName: workspace.name, createdAt: timestamp, updatedAt: timestamp, lastTriggerAt: timestamp, lastTriggerStatus: 'succeeded' } as ScheduledTaskVm));
  }
  function task(projectId: string, id: string) {
    return tasks().find((item) => item.projectId === projectId && item.id === id) ?? missing();
  }
  function occurrences(projectId: string, id: string): ScheduledOccurrenceVm[] {
    const item = task(projectId, id);
    return [0, 1].map((index) => ({ id: `${id}-${index}`, scheduledTaskId: id, scheduledAt: index ? '2026-09-08T01:00:00Z' : timestamp,
      triggerKind: index ? 'manual' : 'scheduled', status: 'succeeded', attempt: 1,
      taskId: item.mode === 'direct' ? 'mock-task' : 'demo-review', runId: item.mode === 'workflow' && index ? 'run-051' : DEMO_RUN_ID,
      startedAt: index ? '2026-09-08T01:00:00Z' : timestamp, finishedAt: index ? '2026-09-08T01:03:00Z' : '2026-09-09T01:03:00Z' }));
  }
  function requirements(): RemoteTaskVm[] {
    const titles = language() === 'en'
      ? ['Add export options', 'Review configuration defaults', 'Document project structure', 'Investigate acceptance failure']
      : ['补充导出选项', '检查配置默认值', '整理项目结构说明', '排查验收失败原因'];
    return ['queued', 'running', 'completed', 'failed'].map((status, index) => ({ id: `demo-requirement-${index}`, issueId: `GB-${101 + index}`, status,
      workspaceId: workspace.id, title: titles[index], requirement: null, lastActivityAt: timestamp,
      localTaskId: index > 1 ? 'demo-review' : null, runId: index > 1 ? index === 2 ? DEMO_RUN_ID : 'run-051' : null, projectId: index > 1 ? DEMO_PROJECT_ID : null }));
  }
  return {
    async listScheduledTasks(projectId) { return tasks().filter((item) => !projectId || item.projectId === projectId); },
    async getScheduledTask(projectId, id) {
      const item = task(projectId, id);
      return { scheduledTaskId: id, projectId, content: item.title, attachmentNames: [], runMode: item.mode,
        workflowTemplateId: item.mode === 'workflow' ? 'default-lightweight' : null, directConfig: { agentType: 'claude-acp' },
        schedule: item.schedule, overlapPolicy: 'skip_when_running', sessionPolicy: item.mode === 'direct' ? 'continuous' : 'new', expectedUpdatedAt: item.updatedAt };
    },
    async listScheduledTaskOccurrences(projectId, id, cursor, status) {
      const all = occurrences(projectId, id).filter((item) => !status || item.status === status);
      const start = cursor ? all.findIndex((item) => item.id === cursor) + 1 : 0;
      if (cursor && !start) return missing();
      return { items: all.slice(start), nextCursor: null };
    },
    async getScheduledTaskDiagnostics(projectId, id) {
      return { scheduledTaskId: id, projectId, nextAt: task(projectId, id).nextAt, lastStatus: 'succeeded', runCount: 2, retryCount: 0, occurrences: occurrences(projectId, id) };
    },
    async getMulticaSettings() {
      return { enabled: true, toggleLocked: true, multicaBaseUrl: null, multicaAppUrl: null, patSet: true, daemonIdSet: true,
        workspaces: [{ ...workspace }], activeWorkspaceId: workspace.id, defaultProvider: 'multica', connected: true,
        connectedAccount: { name: 'Demo', email: 'demo@example.com' }, addressOverrideSet: false };
    },
    async getMulticaTasks() { return { connected: true, workspaces: [{ ...workspace }], lastActiveWorkspaceId: workspace.id, tasksByWorkspace: { [workspace.id]: requirements() } }; },
    async getMulticaTaskRequirement(id, workspaceId) {
      const item = requirements().find((item) => item.id === id && item.workspaceId === workspaceId) ?? missing();
      return { ...item, requirement: item.title };
    },
  };
}
