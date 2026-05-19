import { invoke } from '@tauri-apps/api/core';
import {
  mockAgentRegistry,
  mockBootstrap,
  mockContent,
  mockLogPage,
  mockProfileList,
  mockRoundDetail,
  mockRunDetail,
  mockTaskDetail,
  mockTaskList,
  mockWorkflow,
  mockWorkflowTemplates,
} from './mockData';
import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AcpSessionQueryInput,
  AcpSessionVm,
  AgentRegistryVm,
  AppBootstrapVm,
  CreateTaskInput,
  ContentVm,
  DesktopLanguage,
  DesktopFontPreference,
  DesktopThemePreference,
  LogPageVm,
  LogQueryInput,
  ManagedAgentInput,
  PreferencesVm,
  ProfileInput,
  ProfileListVm,
  ProfileVm,
  RoundDetailVm,
  RoundSelection,
  RunDetailVm,
  RunSummaryVm,
  TaskDetailVm,
  TaskListVm,
  WorkflowDsl,
  WorkflowTemplateStore,
  WorkflowVm,
} from './types';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
let browserProfileList = { ...mockProfileList, profiles: [...mockProfileList.profiles] };
const browserFontCandidates = [
  'MiSans',
  'Maple Mono NF CN',
  'Microsoft YaHei UI',
  'Microsoft YaHei',
  'DengXian',
  'DengXian Light',
  'SimHei',
  'SimSun',
  'NSimSun',
  'KaiTi',
  'FangSong',
  'YouYuan',
  'LiSu',
  'STXihei',
  'STSong',
  'STKaiti',
  'STFangsong',
  'PingFang SC',
  'PingFang TC',
  'PingFang HK',
  'Hiragino Sans GB',
  'Songti SC',
  'Kaiti SC',
  'Heiti SC',
  'Heiti TC',
  'Noto Sans CJK SC',
  'Noto Sans CJK TC',
  'Noto Sans SC',
  'Noto Serif SC',
  'Source Han Sans SC',
  'Source Han Serif SC',
  'Sarasa Gothic SC',
  'LXGW WenKai',
  'MiSans',
  'HarmonyOS Sans SC',
  'WenQuanYi Micro Hei',
  'WenQuanYi Zen Hei',
  'Segoe UI',
  'Segoe UI Variable',
  'Yu Gothic UI',
  'Meiryo',
  'Malgun Gothic',
  'SF Pro Text',
  'SF Pro Display',
  'Inter',
  'Roboto',
  'Arial',
  'Helvetica Neue',
  'Helvetica',
  'Ubuntu',
  'Cantarell',
  'DejaVu Sans',
  'Liberation Sans',
] as const;

type LocalFontData = { family: string };
type LocalFontWindow = Window & { queryLocalFonts?: () => Promise<LocalFontData[]> };

function command<T>(name: string, args?: Record<string, unknown>, fallback?: T): Promise<T> {
  if (!isTauri && fallback !== undefined) {
    return Promise.resolve(fallback);
  }
  return invoke<T>(name, args);
}

function localTimestamp(date = new Date()) {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function browserProfileId() {
  return `pf-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function getAppBootstrap() {
  return command<AppBootstrapVm>('get_app_bootstrap', undefined, mockBootstrap);
}

export async function getSystemFonts() {
  if (isTauri) {
    return invoke<string[]>('get_system_fonts');
  }
  const queriedFonts = await queryBrowserLocalFonts();
  if (queriedFonts.length > 0) {
    return queriedFonts;
  }
  const detectedFonts = detectBrowserFonts(browserFontCandidates);
  if (detectedFonts.length > 0) {
    return detectedFonts;
  }
  return normalizeFontFamilies(browserFontCandidates);
}

function detectBrowserFonts(candidates: readonly string[]) {
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  if (!context) {
    return [];
  }
  const sample = '任务编排 AI Workflow 0123456789';
  const size = '72px';
  const baseFamilies = ['monospace', 'sans-serif', 'serif'] as const;
  const baselines = new Map(
    baseFamilies.map((family) => {
      context.font = `${size} ${family}`;
      return [family, context.measureText(sample).width] as const;
    }),
  );
  return normalizeFontFamilies(
    candidates.filter((family) => {
      const quoted = quoteFontFamily(family);
      if (document.fonts.check(`16px ${quoted}`)) {
        return true;
      }
      return baseFamilies.some((baseFamily) => {
        context.font = `${size} ${quoted}, ${baseFamily}`;
        return context.measureText(sample).width !== baselines.get(baseFamily);
      });
    }),
  );
}

async function queryBrowserLocalFonts() {
  const fontWindow = window as LocalFontWindow;
  if (typeof fontWindow.queryLocalFonts !== 'function') {
    return [];
  }
  try {
    const fonts = await fontWindow.queryLocalFonts();
    return normalizeFontFamilies(fonts.map((font) => font.family));
  } catch {
    return [];
  }
}

function normalizeFontFamilies(families: readonly string[]) {
  const collator = new Intl.Collator(['zh-CN', 'en'], { sensitivity: 'base', numeric: true });
  return Array.from(new Set(families.map((family) => family.trim()).filter(Boolean))).sort((left, right) => collator.compare(left, right));
}

function quoteFontFamily(family: string) {
  return `"${family.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`;
}

export function getAgentRegistry() {
  return command<AgentRegistryVm>('get_agent_registry', undefined, mockAgentRegistry);
}

export function createAgent(agentType: string, input: ManagedAgentInput) {
  return command<AgentRegistryVm>('create_agent', { agentType, input }, mockAgentRegistry);
}

export function updateAgent(agentType: string, input: ManagedAgentInput) {
  return command<AgentRegistryVm>('update_agent', { agentType, input }, mockAgentRegistry);
}

export function deleteAgent(agentType: string) {
  return command<AgentRegistryVm>('delete_agent', { agentType }, mockAgentRegistry);
}

export function doctorAgent(agentType: string) {
  return command<AgentRegistryVm>('doctor_agent', { agentType }, mockAgentRegistry);
}

export function getTaskList() {
  return command<TaskListVm>('get_task_list', undefined, mockTaskList);
}

export function getProfiles() {
  return command<ProfileListVm>('get_profiles', undefined, browserProfileList);
}

export function getProfile(id: string) {
  return command<ProfileVm>('get_profile', { id }, browserProfileList.profiles.find((profile) => profile.id === id) ?? browserProfileList.profiles[0]);
}

export function createProfile(input: ProfileInput) {
  const now = localTimestamp();
  const profile = { ...input, id: browserProfileId(), createdAt: now, updatedAt: now, path: '' };
  browserProfileList = { profiles: [...browserProfileList.profiles, profile] };
  return command<ProfileVm>('create_profile', { input }, profile);
}

export function updateProfile(id: string, input: ProfileInput) {
  const existing = browserProfileList.profiles.find((profile) => profile.id === id);
  const now = localTimestamp();
  const profile = { ...existing, ...input, id, updatedAt: now, createdAt: existing?.createdAt ?? now, path: existing?.path ?? '' };
  browserProfileList = { profiles: browserProfileList.profiles.map((item) => item.id === id ? profile : item) };
  return command<ProfileVm>('update_profile', { id, input }, profile);
}

export function chooseWorkspace() {
  return command<AppBootstrapVm | null>('choose_workspace', undefined, mockBootstrap);
}

export function selectRecentWorkspace(workspace: string) {
  return command<AppBootstrapVm>('select_recent_workspace', { workspace }, { ...mockBootstrap, repoRoot: workspace });
}

export function getTaskDetail(taskId: string) {
  return command<TaskDetailVm>('get_task_detail', { taskId }, { ...mockTaskDetail, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockTaskDetail.task });
}

export function getWorkflow(taskId: string) {
  return command<WorkflowVm>('get_workflow', { taskId }, { ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task });
}

export function createTask(input: CreateTaskInput) {
  const task = {
    ...mockWorkflow.task,
    id: `task-${String(mockTaskList.tasks.length + 1).padStart(3, '0')}`,
    title: input.title || input.requirementFileName.replace(/\.(md|txt)$/i, ''),
    description: input.description ?? null,
    requirement: input.requirementContent,
    requirementPreview: input.requirementContent.slice(0, 120),
    workflowExists: true,
    workflowValid: true,
    workflowError: null,
  };
  return command<WorkflowVm>('create_task', { input }, {
    ...mockWorkflow,
    task,
    workflowJson: JSON.stringify(input.workflow, null, 2),
  });
}

export function saveTaskWorkflow(taskId: string, workflow: WorkflowDsl) {
  return command<WorkflowVm>('save_task_workflow', { taskId, input: { workflow } }, {
    ...mockWorkflow,
    task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task,
    workflowJson: JSON.stringify(workflow, null, 2),
  });
}

export function getWorkflowTemplates() {
  return command<WorkflowTemplateStore>('get_workflow_templates', undefined, mockWorkflowTemplates);
}

export function saveWorkflowTemplate(name: string, workflow: WorkflowDsl) {
  const template = {
    id: name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `workflow-${mockWorkflowTemplates.templates.length + 1}`,
    name,
    workflow,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  const fallback = {
    ...mockWorkflowTemplates,
    lastUsedTemplateId: template.id,
    templates: [...mockWorkflowTemplates.templates, template],
  };
  return command<WorkflowTemplateStore>('save_workflow_template', { input: { name, workflow } }, fallback);
}

export function getRunDetail(taskId: string, runId: string) {
  return command<RunDetailVm>('get_run_detail', { taskId, runId }, { ...mockRunDetail, run: { ...mockRunDetail.run, id: runId, taskId } });
}

export function getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection) {
  return command<RoundDetailVm>('get_round_detail', { taskId, runId, roundId, selection: toRoundSelectionInput(selection) }, mockRoundDetail(selection, { taskId, runId, roundId }));
}

function toRoundSelectionInput(selection?: RoundSelection) {
  if (!selection) return selection;
  if (selection.kind === 'round' || selection.kind === 'requirement') return { kind: selection.kind, context_node_id: selection.contextNodeId };
  if (selection.kind === 'event' || selection.kind === 'log') return { kind: selection.kind, id: selection.id, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'node') return { kind: selection.kind, node_id: selection.nodeId, context_node_id: selection.contextNodeId };
  if (selection.kind === 'worker-ref') return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, context_node_id: selection.contextNodeId };
  return { kind: selection.kind, node_id: selection.nodeId, attempt_id: selection.attemptId, name: selection.name, context_node_id: selection.contextNodeId };
}

export function startRun(taskId: string) {
  return command<RunSummaryVm>('start_run', { taskId }, { ...mockRunDetail.run, taskId });
}

export function continueRun(taskId: string, runId: string, promptId?: string | null) {
  return command<RunSummaryVm>('continue_run', { taskId, runId, promptId }, { ...mockRunDetail.run, taskId, id: runId });
}

export function submitManualCheck(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure') {
  return command<RunSummaryVm>('submit_manual_check', { taskId, runId, roundId, nodeId, attemptId, outcome }, { ...mockRunDetail.run, taskId, id: runId });
}

export function retryRun(taskId: string, runId: string) {
  return command<RunSummaryVm>('retry_run', { taskId, runId }, { ...mockRunDetail.run, taskId, id: runId });
}

export function killRun(taskId: string, runId: string) {
  return command<RunSummaryVm>('kill_run', { taskId, runId }, { ...mockRunDetail.run, taskId, id: runId, status: 'completed', outcome: 'killed' });
}

export function getLogPage(query: LogQueryInput) {
  return command<LogPageVm>('get_log_page', { query }, mockLogPage(query));
}

export function getAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpSessionQueryInput, fallback?: AcpSessionVm | null) {
  return command<AcpSessionVm | null>('get_acp_session', { taskId, runId, roundId, nodeId, attemptId, query }, fallback ?? null);
}

export function sendAcpPrompt(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: AcpSessionVm | null) {
  return command<AcpSessionVm | null>('send_acp_prompt', { taskId, runId, roundId, nodeId, attemptId, prompt, promptId }, fallback ?? null);
}

export function respondAcpPermission(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: AcpSessionVm | null) {
  return command<AcpSessionVm | null>('respond_acp_permission', { taskId, runId, roundId, nodeId, attemptId, requestId, optionId }, fallback ?? null);
}

export function cancelAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null) {
  return command<AcpSessionVm | null>('cancel_acp_session', { taskId, runId, roundId, nodeId, attemptId }, fallback ?? null);
}

export function getAcpRawFrames(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpRawFrameQueryInput) {
  return command<AcpRawFramePageVm>('get_acp_raw_frames', { taskId, runId, roundId, nodeId, attemptId, query }, {
    items: [],
    page: query?.page ?? 0,
    pageSize: query?.pageSize ?? 100,
    total: 0,
    hasPrevious: false,
    hasNext: false,
    order: 'latest',
    search: query?.search ?? null,
    kind: query?.kind ?? null,
    direction: query?.direction ?? null,
  });
}

export function showArtifact(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string) {
  return command<ContentVm>('show_artifact', { taskId, runId, roundId, nodeId, attemptId, name }, { ...mockContent, title: name });
}

export function showAttachment(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string) {
  return command<ContentVm>('show_attachment', { taskId, runId, roundId, nodeId, attemptId, name }, { ...mockContent, title: name, kind: 'attachment' });
}

export function showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string) {
  return command<ContentVm>('show_worker_ref', { taskId, runId, roundId, nodeId, attemptId }, { ...mockContent, title: attemptId, kind: 'worker-ref' });
}

export function saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference) {
  return command<PreferencesVm>('save_desktop_preferences', { theme, language, font }, { theme, language, font });
}
