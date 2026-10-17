import { record } from 'rrweb';
import { browserApi } from '@/api/browser';
import { browserPreviewState } from '@/api/browserState';
import { applyAppearance, applyPersonalization } from '@/theme';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm } from '@/api/client';
import { createRecordingBuffer, RECORDING_LIMITS } from '../../web/rrweb-demo/recording';
import { createPreviewRun, createPreviewSession, PREVIEW_ROUTE, taskTitle } from './fixture';
import type { Language } from './content';

const options = new URLSearchParams(location.search);
const language: Language = options.get('language') === 'en' ? 'en' : 'zh';
const scene = options.get('scene') || 'after';
const preferences = browserPreviewState.getPreferences();
browserPreviewState.setPreferences({ ...preferences, language: language === 'en' ? 'en' : 'zh-cn', appearance: { ...preferences.appearance, colorScheme: 'dark' } });
const base = await browserApi.getConversationRun('default', 'mock-task', 'run-052');
const leaf = base.sessionTree.rounds.flatMap((round) => round.nodes.flatMap((node) => node.attempts))
  .find((attempt) => `${attempt.roundId}/${attempt.nodeId}/${attempt.attemptId}` === base.sessionTree.selectedSessionKey)!;
const baseSession = (await browserApi.getAcpSession(base.projectId, base.taskId, base.runId, leaf.roundId, leaf.nodeId, leaf.attemptId))!;
let run = createPreviewRun(base, language, scene === 'during' ? 1 : 5);
let session = createPreviewSession(baseSession, language, scene === 'during' ? 1 : 5);
const sessions = new Set<(event: AcpSessionUpdatedEventVm) => void>();
const runs = new Set<(event: ConversationRunStateUpdatedEventVm) => void>();
browserApi.getConversationRun = async (projectId, taskId, runId) => {
  if (projectId !== run.projectId || taskId !== run.taskId || runId !== run.runId) {
    throw { code: 'demo.resource-not-found', params: { projectId, taskId, runId } };
  }
  return structuredClone(run);
};
browserApi.getAcpSession = async (projectId, taskId, runId, roundId, nodeId, attemptId) => {
  if ((projectId ?? run.projectId) !== run.projectId || taskId !== run.taskId || runId !== run.runId
    || roundId !== leaf.roundId || nodeId !== leaf.nodeId || attemptId !== leaf.attemptId) {
    throw { code: 'demo.resource-not-found', params: { projectId, taskId, runId, roundId, nodeId, attemptId } };
  }
  return structuredClone(session);
};
browserApi.getAcpActivityDetail = async (projectId, taskId, runId, roundId, nodeId, attemptId) => ({ items: (await browserApi.getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId))!.events.filter(event => event.kind === 'toolCall'), hasMoreEarlier: false, earlierCursor: null });
browserApi.getAcpToolDetail = async (projectId, taskId, runId, roundId, nodeId, attemptId) => ({ event: (await browserApi.getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId))!.events.find(event => event.kind === 'toolCall') ?? null });
browserApi.getConversationSidebarBootstrap = async () => ({ workspaces: [{ projectId: 'default', workspacePath: '/default', name: 'Gold Band' }], pinRefs: [], lastActiveWorkspaceId: 'default', preferences: {} });
browserApi.getConversationTaskPage = async (projectId) => ({ projectId, tasks: [{ projectId, taskId: run.taskId, taskUuid: run.taskUuid, title: taskTitle(language), autoTitle: false, runMode: 'direct', lastActivityAt: session.sessionStartedAt!, runs: [], runHistoryStatus: 'ready', runsNextCursor: null, pinned: false, pinnedOrder: null }], nextCursor: null, errors: [] });
browserApi.subscribeAcpSessionUpdates = async (listener) => { sessions.add(listener); return () => { sessions.delete(listener); }; };
browserApi.subscribeConversationRunStateUpdates = async (listener) => { runs.add(listener); return () => { runs.delete(listener); }; };
const originalChanges = browserApi.getTurnFileChangeSet.bind(browserApi);
browserApi.getTurnFileChangeSet = async (...args) => {
  const changes = await originalChanges(...args);
  return { ...changes, changes: changes.changes.slice(0, 2), attachments: [], summary: { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0, addedLines: 8, deletedLines: 2 } };
};
const originalComparison = browserApi.getFileComparison.bind(browserApi);
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

function advance(step: number) {
  const boundedStep = Math.max(1, Math.min(5, step));
  run = createPreviewRun(base, language, boundedStep);
  session = createPreviewSession(baseSession, language, boundedStep);
  const locator = { projectId: run.projectId, taskId: run.taskId, taskUuid: run.taskUuid, runId: run.runId, roundId: leaf.roundId, nodeId: leaf.nodeId, attemptId: leaf.attemptId };
  for (const listener of sessions) listener({ ...locator, session: structuredClone(session), lifecycle: run.sessionTree.rounds[0].nodes[0].attempts[0].lifecycle });
  for (const listener of runs) listener({ ...locator, eventKind: step >= 5 ? 'run-completed' : 'node-started', status: run.runStatus, outcome: run.runOutcome });
}
function appearance(scheme: 'dark' | 'light', font: 'default' | 'mono') {
  const current = browserPreviewState.getPreferences();
  const updated = structuredClone(current);
  updated.appearance.colorScheme = scheme;
  updated.personalization.typography.ui.fontStack = font === 'default' ? { source: 'theme' } : { source: 'custom', families: ['Consolas', 'Courier New'] };
  browserPreviewState.setPreferences(updated);
  applyAppearance(updated.appearance);
  applyPersonalization(updated.personalization);
}
let buffer = createRecordingBuffer();
let dispose: (() => void) | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
function stop() {
  if (dispose) record.addCustomEvent('recording-end', {});
  dispose?.(); dispose = undefined; clearTimeout(timer);
  return buffer.stop('manual');
}
const api = {
  advance, appearance,
  start() {
    if (dispose) return;
    buffer = createRecordingBuffer();
    dispose = record({ emit(event) { if (buffer.append(event)) queueMicrotask(stop); }, inlineStylesheet: true, inlineImages: true, collectFonts: true, maskInputOptions: { password: true }, sampling: { mousemove: 80, scroll: 100 } });
    timer = setTimeout(stop, RECORDING_LIMITS.durationMs);
  },
  stop,
};
declare global { interface Window { goldBandPreview: typeof api } }
window.goldBandPreview = api;
window.addEventListener('pagehide', stop, { once: true });
history.replaceState(null, '', scene === 'before' ? '/chat' : PREVIEW_ROUTE);
void import('@/webview-bootstrap');
