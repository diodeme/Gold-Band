import { record } from 'rrweb';
import { browserApi } from './runtime';
import { browserApi as seedApi } from '@/api/browser';
import { applyAppearance, applyPersonalization } from '@/theme';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm } from '@/api/client';
import { createRecordingBuffer, RECORDING_LIMITS, type StopReason } from '../../web/rrweb-demo/recording';
import { createPreviewRun, PREVIEW_ROUTE, taskTitle } from '../shared/fixture';
import type { Language } from '../site/content';
import type { WorkflowCue } from './workflow-source';

const options = new URLSearchParams(location.search);
const language: Language = options.get('language') === 'en' ? 'en' : 'zh';
const scene = options.get('scene') || 'after';
const preferences = (await browserApi.getAppBootstrap()).preferences;
let currentPreferences = await browserApi.saveDesktopPreferences({ ...preferences.appearance, colorScheme: options.get('theme') === 'light' ? 'light' : 'dark' }, preferences.personalization, language === 'en' ? 'en' : 'zh-cn', preferences.useLocalClaude, preferences.verboseLogging);
let advance: (step: number | WorkflowCue) => void | Promise<void> = step => browserApi.scenario!.advance(step as WorkflowCue);
if (scene !== 'during' && scene !== 'after') {
const base = await browserApi.getConversationRun('default', 'mock-task', 'run-052');
let run = createPreviewRun(base, language, scene === 'during' ? 1 : 5);
const sessions = new Set<(event: AcpSessionUpdatedEventVm) => void>();
const runs = new Set<(event: ConversationRunStateUpdatedEventVm) => void>();
browserApi.getConversationRun = async () => structuredClone(run);
browserApi.getAcpSession = async () => structuredClone(run.selectedSession!);
browserApi.getAcpActivityDetail = async () => ({ items: structuredClone(run.selectedSession!.events.filter(event => event.kind === 'toolCall')), hasMoreEarlier: false, earlierCursor: null });
browserApi.getAcpToolDetail = async () => ({ event: structuredClone(run.selectedSession!.events.find(event => event.kind === 'toolCall') ?? null) });
browserApi.getConversationSidebarBootstrap = async () => ({ workspaces: [{ projectId: 'default', workspacePath: '/default', name: 'Gold Band' }], pinRefs: [], lastActiveWorkspaceId: 'default', preferences: {} });
browserApi.getConversationTaskPage = async (projectId) => ({ projectId, tasks: [{ projectId, taskId: run.taskId, taskUuid: run.taskUuid, title: taskTitle(language), autoTitle: false, runMode: 'direct', lastActivityAt: run.selectedSession!.sessionStartedAt!, runs: [], runHistoryStatus: 'ready-empty', runsNextCursor: null, pinned: false, pinnedOrder: null }], nextCursor: null, errors: [] });
browserApi.subscribeAcpSessionUpdates = async (listener) => { sessions.add(listener); return () => { sessions.delete(listener); }; };
browserApi.subscribeConversationRunStateUpdates = async (listener) => { runs.add(listener); return () => { runs.delete(listener); }; };
const originalChanges = seedApi.getTurnFileChangeSet.bind(seedApi);
browserApi.getTurnFileChangeSet = async (...args) => {
  const changes = await originalChanges(...args);
  return { ...changes, changes: changes.changes.slice(0, 2), attachments: [], summary: { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0, addedLines: 8, deletedLines: 2 } };
};
const originalComparison = seedApi.getFileComparison.bind(seedApi);
browserApi.getFileComparison = async (...args) => {
  const comparison = await originalComparison(...args);
  if (comparison.path.endsWith('.md') && comparison.after) {
    comparison.after.content = language === 'zh'
      ? '# 工作区说明\n\n## 会话与文件\n\n每轮会话保留文件变更快照，方便对照修改前后的内容。\n\n## 审阅流程\n\n1. 在会话中打开文件变更。\n2. 在右侧工作区预览文档与 Diff。\n3. 确认修改后，再提交到 Git。\n'
      : '# Workspace notes\n\n## Sessions and files\n\nEach turn keeps a snapshot of changed files for comparison.\n\n## Review workflow\n\n1. Open the file changes in the conversation.\n2. Preview documents and diffs in the workspace.\n3. Review the result before committing to Git.\n';
    comparison.after.version.byteLength = new TextEncoder().encode(comparison.after.content).length;
  }
  return comparison;
};

advance = (value: number | WorkflowCue) => {
  const step = Number(value);
  run = createPreviewRun(base, language, Math.max(1, Math.min(5, step)));
  const locator = { projectId: run.projectId, taskId: run.taskId, taskUuid: run.taskUuid, runId: run.runId, roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-001' };
  for (const listener of sessions) listener({ ...locator, session: structuredClone(run.selectedSession), lifecycle: run.sessionTree.rounds[0].nodes[0].attempts[0].lifecycle });
  for (const listener of runs) listener({ ...locator, eventKind: step >= 5 ? 'run-completed' : 'node-started', status: run.runStatus, outcome: run.runOutcome });
};
}
async function appearance(scheme: 'dark' | 'light', font: 'default' | 'mono') {
  const current = currentPreferences;
  const updated = structuredClone(current);
  updated.appearance.colorScheme = scheme;
  updated.personalization.typography.ui.fontStack = font === 'default' ? { source: 'theme' } : { source: 'custom', families: ['Consolas', 'Courier New'] };
  currentPreferences = await browserApi.saveDesktopPreferences(updated.appearance, updated.personalization, updated.language, updated.useLocalClaude, updated.verboseLogging);
  applyAppearance(updated.appearance);
  applyPersonalization(updated.personalization);
}
let buffer = createRecordingBuffer();
let dispose: (() => void) | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
function stop(reason: StopReason = 'manual') {
  if (dispose && reason === 'manual') record.addCustomEvent('recording-end', {});
  dispose?.(); dispose = undefined; clearTimeout(timer);
  return buffer.stop(reason);
}
const api = {
  advance, appearance,
  snapshot: () => browserApi.scenario?.snapshot(),
  marker(stepId: string) { if (dispose) record.addCustomEvent('semantic-checkpoint', { stepId, viewport: { width: innerWidth, height: innerHeight } }); },
  start() {
    if (dispose) return;
    buffer = createRecordingBuffer();
    dispose = record({ emit(event) { const reason = buffer.append(event); if (reason) queueMicrotask(() => stop(reason)); }, inlineStylesheet: true, inlineImages: true, collectFonts: true, maskInputOptions: { password: true }, sampling: { mousemove: 80, scroll: 100 } });
    timer = setTimeout(() => stop('duration'), RECORDING_LIMITS.durationMs);
  },
  stop,
};
declare global { interface Window { goldBandPreview: typeof api } }
window.goldBandPreview = api;
window.addEventListener('pagehide', () => { stop(); if (scene === 'during') browserApi.scenario?.dispose(); }, { once: true });
history.replaceState(null, '', `${scene === 'before' ? '/chat' : PREVIEW_ROUTE}?${options}`);
void import('@/webview-bootstrap');
