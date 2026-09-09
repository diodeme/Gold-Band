import type { TurnFileChangeSetVm, TurnFileLocatorVm } from '@/types';
import { DEMO_WORKFLOW_ROUNDS, demoRunIds } from './scenarios';

export const DEMO_REPORT_PATH = '/default/demo-report.md';
export const demoDevelopmentFiles: TurnFileChangeSetVm = {
  id: 'browser-change-set-052', turnId: 'demo-development-turn', promptEventId: 'dev-test-1', branchId: 'root',
  status: 'finalized', startedAt: '2026-09-01T08:00:00Z', finishedAt: '2026-09-01T08:01:00Z',
  summary: { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0, addedLines: 8, deletedLines: 2 },
  changes: [
    { id: 'browser-added-readme', changeKind: 'added', logicalPath: 'docs/workspace-notes.md', text: true, addedLines: 4, deletedLines: 0 },
    { id: 'browser-modified-config', changeKind: 'modified', logicalPath: 'src/config.json', text: true, addedLines: 4, deletedLines: 2 },
  ],
  attachments: [{ id: 'demo-report', relativePath: 'demo-report.md', name: 'demo-report.md', byteLength: 0 }],
  limitationCodes: [],
};

export function validateDemoFileLocator(locator: TurnFileLocatorVm, changeSetId: string) {
  if (locator.projectId !== 'default' || locator.taskId !== 'demo-review' || !demoRunIds(locator.taskId).includes(locator.runId)
    || !DEMO_WORKFLOW_ROUNDS.some((roundId) => roundId === locator.roundId) || locator.nodeId !== 'dev-test' || locator.attemptId !== 'attempt-001'
    || locator.branchId !== 'root' || locator.outerNodeId || locator.outerAttemptId || changeSetId !== demoDevelopmentFiles.id) {
    throw { code: 'demo.resource-not-found', params: { changeSetId } };
  }
}

export function demoReportContent(language: string) {
  return language === 'en'
    ? '# Development report\n\n## Completed\n\n- Reviewed module boundaries.\n- Updated configuration defaults and workspace notes.\n- Verified the configuration checks.\n\n## Result\n\nAll checks passed. Ready for acceptance.\n'
    : '# 开发报告\n\n## 已完成\n\n- 检查模块边界。\n- 更新配置默认值与工作区说明。\n- 验证配置检查项。\n\n## 结果\n\n检查通过，可以进入验收。\n';
}
