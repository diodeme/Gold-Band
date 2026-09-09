export const DEMO_RUN_ID = 'run-052';
export const DEMO_WORKFLOW_ROUNDS = ['round-001', 'round-002'] as const;
export const demoWorkflowRuns = [
  { runId: DEMO_RUN_ID, startedAt: '2026-09-01T08:00:00Z', updatedAt: '2026-09-01T08:12:00Z',
    goals: { en: ['Review configuration defaults', 'Add missing configuration boundary checks'], 'zh-cn': ['检查配置默认值', '补齐配置边界检查'] } },
  { runId: 'run-051', startedAt: '2026-08-31T08:00:00Z', updatedAt: '2026-08-31T08:12:00Z',
    goals: { en: ['Document module responsibilities', 'Clarify module dependencies and ownership'], 'zh-cn': ['整理模块职责', '补充模块依赖与职责边界'] } },
];

export function demoRunIds(taskId: string): string[] {
  return taskId === 'demo-review' ? demoWorkflowRuns.map((run) => run.runId) : taskId === 'mock-task' ? [DEMO_RUN_ID] : [];
}
