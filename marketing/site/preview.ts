import { record } from 'rrweb';
import { browserApi } from '@/api/browser';
import { browserPreviewState } from '@/api/browserState';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm } from '@/api/client';
import { createRecordingBuffer, RECORDING_LIMITS } from '../../web/rrweb-demo/recording';
import { createPreviewRun, PREVIEW_ROUTE, previewTask } from './fixture';
import type { Language } from './content';
import { createWorkflowScene, showcaseWorkflow, WORKFLOW_STEPS, type WorkflowStep } from './workflow-scene';
import { shotSchema, type Shot } from './timeline';
import { demoAgentRegistry, demoProfiles } from '../demo/catalog';
import { createDemoApi } from '../demo/api';
import { recordSetupStory, recordWorkflowStory, recordPersonalizationStory, recordReviewStory } from './storyboard';
import { createReviewApi } from './review-api';

const options = new URLSearchParams(location.search);
const language: Language = options.get('language') === 'en' ? 'en' : 'zh';
const scene = options.get('scene') || 'after';
const theme = options.get('theme') === 'light' ? 'light' : 'dark';
const preferences = browserPreviewState.getPreferences();
browserPreviewState.setPreferences({ ...preferences, language: language === 'en' ? 'en' : 'zh-cn', appearance: { ...preferences.appearance, colorScheme: theme } });
if (scene === 'personalize') {
  const icon = new Image();
  icon.src = '/agent-icons/codex.svg';
  await icon.decode();
  const canvas = document.createElement('canvas');
  canvas.width = canvas.height = 128;
  const context = canvas.getContext('2d')!;
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, 128, 128);
  context.drawImage(icon, 16, 16, 96, 96);
  const current = browserPreviewState.getPreferences();
  current.avatars.agent.recentAvatars = [{ id: 'site-avatar', dataUrl: canvas.toDataURL('image/png'), createdAt: new Date().toISOString() }];
  browserPreviewState.setPreferences(current);
}
const management = createDemoApi();
await management.saveDesktopPreferences(preferences.appearance, preferences.personalization, language === 'en' ? 'en' : 'zh-cn', false, false);
browserApi.getProfile = management.getProfile;
browserApi.listSkills = management.listSkills;
browserApi.listProjectSkills = management.listProjectSkills;
browserApi.readSkill = management.readSkill;
browserApi.getSkillSyncStatus = management.getSkillSyncStatus;
const base = await browserApi.getConversationRun('default', 'mock-task', 'run-052');
const authoring = await browserApi.getWorkflow('mock-task', 'default');
let run = scene === 'during' ? createWorkflowScene(base, language, 'ready') : createPreviewRun(base, language, 5);
let workflowStep: WorkflowStep = 'ready';
browserApi.getAgentRegistry = async () => structuredClone(demoAgentRegistry);
browserApi.getProfiles = async () => ({ profiles: demoProfiles(language === 'en' ? 'en' : 'zh-cn') });
if (scene === 'during') browserApi.getWorkflow = async () => ({ ...structuredClone(authoring), workflowJson: JSON.stringify(showcaseWorkflow), graph: structuredClone(run.workflowGraph), modelBindings: { definitionRevision: 'site-v1', bindingRevision: 1, bindings: showcaseWorkflow.nodes.flatMap(node => node.type === 'worker' ? [{ executionSlotId: node.executionSlotId!, agentId: 'claude-acp', permissionModeId: null }] : []) } });
const sessions = new Set<(event: AcpSessionUpdatedEventVm) => void>();
const runs = new Set<(event: ConversationRunStateUpdatedEventVm) => void>();
browserApi.getConversationRun = async () => structuredClone(run);
browserApi.getAcpSession = async (projectId, taskId, runId, roundId, nodeId, attemptId) => {
  if (projectId !== run.projectId || taskId !== run.taskId || runId !== run.runId || roundId !== 'round-001' || attemptId !== 'attempt-001') throw { code: 'site.scene-locator-invalid' };
  return scene === 'during' ? createWorkflowScene(base, language, workflowStep, nodeId).selectedSession! : structuredClone(run.selectedSession!);
};
browserApi.getAcpActivityDetail = async () => ({ items: structuredClone(run.selectedSession!.events.filter(event => event.kind === 'toolCall')), hasMoreEarlier: false, earlierCursor: null });
browserApi.getAcpToolDetail = async () => ({ event: structuredClone(run.selectedSession!.events.find(event => event.kind === 'toolCall') ?? null) });
browserApi.getConversationSidebarBootstrap = async () => ({ workspaces: [{ projectId: 'default', workspacePath: '/default', name: 'Gold Band' }], pinRefs: [], lastActiveWorkspaceId: 'default', preferences: {} });
browserApi.getConversationTaskPage = async (projectId) => ({ projectId, tasks: [previewTask(run, language)], nextCursor: null, errors: [] });
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
const review = scene === 'after' ? await createReviewApi({ ...browserApi, getTurnFileChangeSet: originalChanges, getFileComparison: originalComparison }, language, {
  projectId: run.projectId, taskId: run.taskId, runId: run.runId, roundId: run.selectedSession!.roundId!,
  nodeId: run.selectedSession!.nodeId!, attemptId: run.selectedSession!.attemptId!, branchId: 'root',
}) : null;
if (review) {
  Object.assign(browserApi, review.api);
  const changes = run.selectedSession!.events.find(event => event.kind === 'fileChangeSet')!;
  changes.raw = { ...changes.raw as object, summary: review.summary, attachmentCount: 1 };
}

function advance(step: number | WorkflowStep) {
  if (scene === 'during') {
    const phase = typeof step === 'number' ? WORKFLOW_STEPS[step] : step;
    workflowStep = phase;
    run = createWorkflowScene(base, language, phase);
  } else run = createPreviewRun(base, language, typeof step === 'number' ? Math.max(1, Math.min(5, step)) : 5);
  const locator = { projectId: run.projectId, taskId: run.taskId, taskUuid: run.taskUuid, runId: run.runId, roundId: 'round-001', nodeId: run.selectedSession!.nodeId!, attemptId: 'attempt-001' };
  const leaf = run.sessionTree.rounds[0].nodes.find(node => node.nodeId === locator.nodeId)!.attempts[0];
  for (const listener of sessions) listener({ ...locator, session: structuredClone(run.selectedSession), lifecycle: leaf.lifecycle });
  for (const listener of runs) listener({ ...locator, eventKind: run.runStatus === 'completed' ? 'run-completed' : 'node-started', status: run.runStatus, outcome: run.runOutcome });
}
let buffer = createRecordingBuffer();
let dispose: (() => void) | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;
function stop(reason: 'manual' | 'duration' | 'bytes' | 'events' = 'manual') {
  if (dispose) record.addCustomEvent('recording-end', {});
  dispose?.(); dispose = undefined; clearTimeout(timer);
  return buffer.stop(reason);
}
const api = {
  advance,
  disposeReview: () => review?.dispose(),
  lastCheckpoint: null as string | null,
  viewportTarget: null as { width: number; height: number } | null,
  pointerTarget: null as { x: number; y: number } | null,
  async hover(element: Element) {
    element.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    const rect = element.getBoundingClientRect();
    api.pointerTarget = { x: Math.round(rect.left + rect.width / 2), y: Math.round(rect.top + rect.height / 2) };
    const deadline = performance.now() + 8000;
    while (api.pointerTarget) {
      if (performance.now() > deadline) throw { code: 'site.pointer-timeout' };
      await new Promise(requestAnimationFrame);
    }
  },
  async resize(width: number, height: number) {
    api.viewportTarget = { width, height };
    const deadline = performance.now() + 8000;
    try {
      while (innerWidth !== width || innerHeight !== height) {
        if (performance.now() > deadline) throw { code: 'site.viewport-timeout' };
        await new Promise(requestAnimationFrame);
      }
    } finally { api.viewportTarget = null; }
  },
  storyboard() {
    if (scene === 'before') return recordSetupStory(api, language);
    if (scene === 'personalize') return recordPersonalizationStory(api, language);
    if (scene === 'after') return recordReviewStory(api, language);
    return recordWorkflowStory(api, language);
  },
  shot(shot: Omit<Shot, 'at'>) { api.lastCheckpoint = shot.id; record.addCustomEvent('site-shot', shotSchema.parse({ ...shot, at: 0 })); },
  async save() {
    const evidence = review ? await review.evidence() : null;
    if (evidence) {
      const state = evidence as { head: string; initialHead: string; changes: unknown[] };
      if (state.head === state.initialHead || state.changes.length) throw { code: 'site.review-incomplete' };
    }
    if (scene === 'personalize') {
      const restored = browserPreviewState.getPreferences();
      if (restored.appearance.themeId !== preferences.appearance.themeId || restored.appearance.colorScheme !== theme
        || restored.personalization.typography.ui.fontStack.source !== 'theme' || restored.personalization.avatars.agent.image.source !== 'theme') {
        throw { code: 'site.appearance-restore-failed', params: { expectedTheme: preferences.appearance.themeId,
          theme: restored.appearance.themeId, scheme: restored.appearance.colorScheme,
          font: restored.personalization.typography.ui.fontStack.source, avatar: restored.personalization.avatars.agent.image.source } };
      }
    }
    const recording = stop();
    const response = await fetch('/__site-recording', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ language, scene, theme, recording }) });
    if (!response.ok) throw { code: 'site.recording-save-failed' };
    const result = await response.json();
    if (review) await review.dispose();
    return { ...result, ...(evidence ? { gitEvidence: evidence } : {}) };
  },
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
window.addEventListener('pagehide', () => { stop(); void review?.dispose(); }, { once: true });
history.replaceState(null, '', `${scene === 'before' ? '/chat' : PREVIEW_ROUTE}?${options}`);
void import('@/webview-bootstrap');
