import type { AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AgentRegistryVm, AppBootstrapVm, AutoTemplate, ContentVm, ConversationAutoConfigVm, ConversationCreateInput, ConversationRunModeVm, ConversationRunVm, ConversationSearchResultVm, ConversationSidebarVm, ConversationValidationResultVm, ConversationWorkspaceVm, CreateTaskInput, DesktopFontPreference, DesktopLanguage, DesktopThemePreference, LocalClaudeStatusVm, LogPageVm, LogQueryInput, ManagedAgentInput, PreferencesVm, ProfileInput, ProfileVm, RoundDetailVm, RoundSelection, RunDetailVm, RunSummaryVm, RunScheduledTaskResultVm, ScheduledOccurrenceVm, ScheduledTaskDiagnosticsVm, ScheduledTaskEditVm, ScheduledTaskVm, ScheduledScheduleSpec, TaskDetailVm, TaskListVm, UpdateBadgeStateVm, UpdateScheduledTaskInput, UpdateStatusVm, UpdaterSettingsVm, WorkflowDsl, WorkflowTemplateStore, WorkflowVm } from '../types';
import { mockAgentRegistry, mockBootstrap, mockContent, mockErrorBlockedConversationRun, mockErrorBlockedConversationSession, mockLogPage, mockRoundDetail, mockRunDetail, mockTaskDetail, mockTaskList, mockWorkflow, mockWorkflowTemplates } from '../mockData';
import type { RuntimeApi, ScheduledOccurrenceUpdatedEventVm, ScheduledTaskUpdatedEventVm } from './client';
import { browserPreviewState } from './browserState';
import { localTimestamp, toRoundSelectionInput } from './shared';

const browserFontCandidates = [
  'MiSans', 'Maple Mono NF CN', 'Microsoft YaHei UI', 'Microsoft YaHei', 'DengXian', 'DengXian Light', 'SimHei', 'SimSun', 'NSimSun', 'KaiTi', 'FangSong', 'YouYuan', 'LiSu', 'STXihei', 'STSong', 'STKaiti', 'STFangsong', 'PingFang SC', 'PingFang TC', 'PingFang HK', 'Hiragino Sans GB', 'Songti SC', 'Kaiti SC', 'Heiti SC', 'Heiti TC', 'Noto Sans CJK SC', 'Noto Sans CJK TC', 'Noto Sans SC', 'Noto Serif SC', 'Source Han Sans SC', 'Source Han Serif SC', 'Sarasa Gothic SC', 'LXGW WenKai', 'MiSans', 'HarmonyOS Sans SC', 'WenQuanYi Micro Hei', 'WenQuanYi Zen Hei', 'Segoe UI', 'Segoe UI Variable', 'Yu Gothic UI', 'Meiryo', 'Malgun Gothic', 'SF Pro Text', 'SF Pro Display', 'Inter', 'Roboto', 'Arial', 'Helvetica Neue', 'Helvetica', 'Ubuntu', 'Cantarell', 'DejaVu Sans', 'Liberation Sans',
] as const;

type LocalFontData = { family: string };
type LocalFontWindow = Window & { queryLocalFonts?: () => Promise<LocalFontData[]> };

const browserConversationRuns = new Map<string, ConversationRunVm>();
const browserScheduledTasks: ScheduledTaskVm[] = [];
const browserScheduledTaskDefinitions = new Map<string, ScheduledTaskEditVm>();
const browserScheduledTaskListeners = new Set<(event: ScheduledTaskUpdatedEventVm) => void>();
const browserScheduledOccurrences = new Map<string, ScheduledOccurrenceVm[]>();
const browserScheduledOccurrenceListeners = new Set<(event: ScheduledOccurrenceUpdatedEventVm) => void>();
let browserScheduledTaskSequence = 0;

function emitBrowserScheduledTaskUpdated(task: ScheduledTaskVm) {
  const event: ScheduledTaskUpdatedEventVm = {
    projectId: task.projectId,
    scheduledTaskId: task.id,
    status: task.status,
  };
  browserScheduledTaskListeners.forEach((listener) => listener(event));
}

function emitBrowserScheduledOccurrenceUpdated(occurrence: ScheduledOccurrenceVm, projectId: string) {
  const event: ScheduledOccurrenceUpdatedEventVm = {
    projectId,
    scheduledTaskId: occurrence.scheduledTaskId,
    occurrenceId: occurrence.id,
    status: occurrence.status,
    errorCode: occurrence.errorCode ?? null,
    taskId: occurrence.taskId ?? null,
    runId: occurrence.runId ?? null,
  };
  browserScheduledOccurrenceListeners.forEach((listener) => listener(event));
}

function browserTimezoneLabel(timezone: string) {
  return ({
    'Asia/Shanghai': '中国（上海）',
    'Asia/Tokyo': '日本（东京）',
    'Europe/London': '英国（伦敦）',
    'America/New_York': '美国（纽约）',
  } as Record<string, string>)[timezone] ?? timezone;
}

function browserScheduleLabel(schedule: ScheduledScheduleSpec) {
  if (schedule.kind === 'At') {
    try {
      const formatted = new Intl.DateTimeFormat('zh-CN', {
        timeZone: schedule.timezone,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        hourCycle: 'h23',
      }).format(new Date(schedule.at)).replaceAll('/', '-');
      return `单次 ${formatted}`;
    } catch {
      return `单次 ${schedule.at}`;
    }
  }
  if (schedule.kind === 'Every') return `每隔 ${schedule.every.value} ${schedule.every.unit === 'minutes' ? '分钟' : '小时'}`;
  if (schedule.kind === 'Cron') return `Cron ${schedule.expression}`;
  const weekdayLabels: Record<string, string> = { Mon: '周一', Tue: '周二', Wed: '周三', Thu: '周四', Fri: '周五', Sat: '周六', Sun: '周日' };
  const preset = typeof schedule.preset === 'string'
    ? ({ Hourly: '每小时', Daily: '每天', Weekdays: '工作日' } as Record<string, string>)[schedule.preset] ?? schedule.preset
    : `每周 ${schedule.preset.Weekly.weekdays.map((day) => weekdayLabels[day] ?? day).join('、')}`;
  return `${preset} ${String(schedule.hour).padStart(2, '0')}:${String(schedule.minute).padStart(2, '0')}`;
}

function browserAgentIdentity(agentType: string) {
  const agent = mockAgentRegistry.agents.find((candidate) => candidate.agentType === agentType);
  return agent ? {
    agentType: agent.agentType,
    displayName: agent.displayName,
    iconKey: agent.iconKey,
  } : null;
}

function browserCompletedConversationRun(): ConversationRunVm {
  const run = structuredClone(mockErrorBlockedConversationRun);
  run.runId = 'run-052';
  run.title = '斜杠命令预览';
  run.runMode = 'direct';
  run.directConfig = { agentType: 'claude-acp' };
  run.agentIdentity = browserAgentIdentity('claude-acp');
  run.runStatus = 'completed';
  run.runOutcome = 'success';
  run.pauseReason = null;
  run.runtimeErrorMessage = null;
  run.selectedSession = {
    ...mockErrorBlockedConversationSession,
    sessionId: 'browser-session-052',
    roundId: 'round-001',
    nodeId: 'dev',
    attemptId: 'attempt-001',
    providerCwd: 'D:/Projects/code/ai/Gold-Band',
    cwd: 'D:/Projects/code/ai/Gold-Band',
    status: 'completed',
    stopReason: 'end_turn',
    config: {
      modelOverrideId: null,
      permissionModeOverrideId: null,
      currentModelId: 'default',
      currentModelName: 'Default (recommended)',
      currentModeId: 'bypassPermissions',
      currentModeName: 'Bypass Permissions',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          options: [
            { value: 'default', name: 'Default (recommended)' },
            { value: 'glm-5.2-hs', name: 'GLM 5.2' },
          ],
        },
        {
          id: 'mode',
          category: 'mode',
          options: [
            { value: 'bypassPermissions', name: 'Bypass Permissions' },
          ],
        },
        {
          id: 'effort',
          category: 'thought_level',
          type: 'select',
          currentValue: 'default',
          options: [
            { value: 'default', name: 'Default' },
            { value: 'low', name: 'Low' },
            { value: 'high', name: 'High' },
            { value: 'max', name: 'Max' },
          ],
        },
      ],
    },
  };
  const attempt = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0];
  if (attempt?.lifecycle) {
    attempt.status = 'completed';
    attempt.outcome = 'success';
    attempt.lifecycle = {
      ...attempt.lifecycle,
      runtime: {
        ...attempt.lifecycle.runtime,
        status: 'completed',
        outcome: 'success',
        pauseReason: null,
        active: false,
        continuable: false,
        phase: 'completed',
      },
      acp: {
        ...attempt.lifecycle.acp,
        status: 'completed',
        active: false,
        stopping: false,
        terminal: true,
      },
      displayStatus: 'success',
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'responding',
        statusKey: null,
        canStop: false,
        lockInput: false,
      },
    };
  }
  return run;
}

export const browserApi: RuntimeApi = {
  async subscribeScheduledTaskUpdates(listener) {
    browserScheduledTaskListeners.add(listener);
    return () => browserScheduledTaskListeners.delete(listener);
  },
  async subscribeScheduledOccurrenceUpdates(listener) {
    browserScheduledOccurrenceListeners.add(listener);
    return () => browserScheduledOccurrenceListeners.delete(listener);
  },
  checkLocalClaude() {
    return Promise.resolve({ found: false, path: null });
  },
  getAppBootstrap() {
    return Promise.resolve(browserPreviewState.getAppBootstrap());
  },
  async getSystemFonts() {
    const queriedFonts = await queryBrowserLocalFonts();
    if (queriedFonts.length > 0) return queriedFonts;
    const detectedFonts = detectBrowserFonts(browserFontCandidates);
    if (detectedFonts.length > 0) return detectedFonts;
    return normalizeFontFamilies(browserFontCandidates);
  },
  getAgentRegistry() {
    return Promise.resolve(mockAgentRegistry);
  },
  getAgentCommandCatalog(agentType: string, workspacePath: string) {
    const commands = agentType === 'codex-acp'
      ? [
        { name: 'review', description: 'Review my current changes and find issues' },
        { name: 'review-branch', description: 'Review the code changes against a specific branch', inputHint: 'branch name' },
        { name: 'review-commit', description: 'Review the code changes introduced by a commit', inputHint: 'commit sha' },
        { name: 'init', description: 'Create an AGENTS.md file with instructions for Codex' },
        { name: 'compact', description: 'Summarize the conversation to preserve context' },
        { name: 'logout', description: 'Log out of Codex' },
      ]
      : [
        { name: 'pretext', description: 'Measure and lay out multiline text without DOM reflow' },
        { name: 'deep-research', description: 'Research, verify sources, and synthesize a cited report' },
        { name: 'design-sync', description: 'Push the current React design system to Claude Design', inputHint: 'project hint' },
        { name: 'update-config', description: 'Update Claude Code settings, permissions, hooks, or environment' },
        { name: 'verify', description: 'Run the app and verify that the current change works' },
        { name: 'debug', description: 'Enable debug logging and diagnose an issue', inputHint: 'issue' },
        { name: 'code-review', description: 'Review the current diff for correctness and maintainability', inputHint: 'target' },
        { name: 'simplify', description: 'Simplify and clean up the current changes' },
        { name: 'security-review', description: 'Perform a security review of the pending changes' },
        { name: 'reload-skills', description: 'Reload skills added or changed on disk' },
      ];
    return Promise.resolve({
      agentType,
      workspaceKey: workspacePath,
      updatedAt: localTimestamp(),
      commands,
    });
  },
  createAgent(_agentType: string, _input: ManagedAgentInput) {
    return Promise.resolve(mockAgentRegistry);
  },
  updateAgent(_agentType: string, _input: ManagedAgentInput) {
    return Promise.resolve(mockAgentRegistry);
  },
  deleteAgent(_agentType: string) {
    return Promise.resolve(mockAgentRegistry);
  },
  doctorAgent(_agentType: string) {
    return Promise.resolve(mockAgentRegistry);
  },
  getTaskList() {
    return Promise.resolve(mockTaskList);
  },
  getProfiles() {
    return Promise.resolve(browserPreviewState.getProfiles());
  },
  getProfile(id: string) {
    return Promise.resolve(browserPreviewState.getProfile(id) ?? browserPreviewState.getProfiles().profiles[0]);
  },
  createProfile(input: ProfileInput) {
    const now = localTimestamp();
    const profile: ProfileVm = { ...input, id: browserProfileId(), scope: 'user', isBuiltIn: false, createdAt: now, updatedAt: now, path: '' };
    return Promise.resolve(browserPreviewState.addProfile(profile));
  },
  updateProfile(id: string, input: ProfileInput) {
    const existing = browserPreviewState.getProfiles().profiles.find((profile) => profile.id === id);
    if (!existing) return browserCommandError('app.unexpected');
    if (existing.isBuiltIn) return browserCommandError('profile.readonly-built-in');
    const profile: ProfileVm = { ...existing, ...input, id, scope: 'user', isBuiltIn: false, createdAt: existing.createdAt, updatedAt: localTimestamp(), path: existing.path };
    return Promise.resolve(browserPreviewState.updateProfile(profile));
  },
  deleteProfile(id: string, force = false) {
    const existing = browserPreviewState.getProfiles().profiles.find((profile) => profile.id === id);
    if (!existing) return browserCommandError('app.unexpected');
    if (existing.isBuiltIn) return browserCommandError('profile.readonly-built-in');
    if (!force && existing.summary.includes('[requires-confirmation]')) {
      return browserCommandError('profile.delete-confirmation-required', {
        templateCount: 1,
        taskCount: 1,
        runCount: 0,
      });
    }
    return Promise.resolve(browserPreviewState.removeProfile(id));
  },
  chooseWorkspace() {
    return Promise.resolve({ ...mockBootstrap, updateBadges: browserPreviewState.getUpdateBadges() });
  },
  selectRecentWorkspace(workspace: string) {
    return Promise.resolve({ ...mockBootstrap, repoRoot: workspace, updateBadges: browserPreviewState.getUpdateBadges() });
  },
  removeRecentWorkspace(workspace: string) {
    const bootstrap = browserPreviewState.getAppBootstrap();
    if (workspace === bootstrap.repoRoot) {
      return browserCommandError('workspace.recent-current-locked', { workspace });
    }
    if (bootstrap.recentWorkspaces.length <= 1) {
      return browserCommandError('workspace.recent-minimum-required', { workspace });
    }
    return Promise.resolve(
      browserPreviewState.setRecentWorkspaces(
        bootstrap.recentWorkspaces.filter((item) => item !== workspace),
      ),
    );
  },
  getTaskDetail(taskId: string) {
    return Promise.resolve({ ...mockTaskDetail, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockTaskDetail.task });
  },
  getWorkflow(taskId: string) {
    return Promise.resolve({ ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task });
  },
  createTask(input: CreateTaskInput) {
    const task = {
      ...mockWorkflow.task,
      id: `task-${String(mockTaskList.tasks.length + 1).padStart(3, '0')}`,
      title: input.title?.trim() || `task-${String(mockTaskList.tasks.length + 1).padStart(3, '0')}`,
      description: input.description ?? null,
      requirement: input.requirementContent,
      requirementPreview: input.requirementContent.slice(0, 120),
      workflowExists: true,
      workflowValid: true,
      workflowError: null,
    };
    return Promise.resolve({ ...mockWorkflow, task, workflowJson: JSON.stringify(input.workflow, null, 2) });
  },
  saveTaskWorkflow(_projectId, taskId, workflow) {
    return Promise.resolve({ ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task, workflowJson: JSON.stringify(workflow, null, 2) });
  },
  getWorkflowTemplates() {
    return Promise.resolve(browserPreviewState.getWorkflowTemplates());
  },
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl) {
    const current = browserPreviewState.getWorkflowTemplates();
    let nextWorkflow = workflow;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const workflowId = `workflow-${crypto.randomUUID().replaceAll('-', '')}`;
      if (!current.templates.some((template) => template.workflow.id === workflowId)) {
        nextWorkflow = { ...workflow, id: workflowId };
        break;
      }
    }
    const template = {
      id: name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `workflow-${current.templates.length + 1}`,
      name,
      workflow: nextWorkflow,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: template.id,
      templates: [...current.templates, template],
    }));
  },
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl) {
    const current = browserPreviewState.getWorkflowTemplates();
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: templateId,
      templates: current.templates.map((template) => template.id === templateId ? { ...template, workflow, updatedAt: new Date().toISOString() } : template),
    }));
  },
  deleteWorkflowTemplate(templateId: string) {
    const current = browserPreviewState.getWorkflowTemplates();
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: current.lastUsedTemplateId === templateId ? 'default' : current.lastUsedTemplateId,
      templates: current.templates.filter((template) => template.id !== templateId),
    }));
  },
  getAutoTemplates() {
    return Promise.resolve(browserPreviewState.getAutoTemplates());
  },
  saveAutoTemplate(name: string, config: ConversationAutoConfigVm) {
    const current = browserPreviewState.getAutoTemplates();
    const idBase = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `auto-${current.templates.length + 1}`;
    let id = idBase;
    let suffix = 1;
    while (current.templates.some((template) => template.id === id)) {
      suffix += 1;
      id = `${idBase}-${suffix}`;
    }
    const now = new Date().toISOString();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: [...current.templates, { id, name, config, createdAt: now, updatedAt: now }],
    }));
  },
  updateAutoTemplate(templateId: string, name: string, config: ConversationAutoConfigVm) {
    const current = browserPreviewState.getAutoTemplates();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: current.templates.map((template) => template.id === templateId ? { ...template, name, config, updatedAt: new Date().toISOString() } : template),
    }));
  },
  deleteAutoTemplate(templateId: string) {
    const current = browserPreviewState.getAutoTemplates();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: current.templates.filter((template) => template.id !== templateId),
    }));
  },
  replaceAutoTemplates(templates: AutoTemplate[]) {
    return Promise.resolve(browserPreviewState.setAutoTemplates({ version: '0.1', templates }));
  },
  getRunDetail(taskId: string, runId: string) {
    return Promise.resolve({ ...mockRunDetail, run: { ...mockRunDetail.run, id: runId, taskId } });
  },
  getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection) {
    return Promise.resolve(mockRoundDetail(selection, { taskId, runId, roundId }));
  },
  startRun(taskId: string) {
    return Promise.resolve({ ...mockRunDetail.run, taskId });
  },
  continueRun(_projectId, taskId, runId, _promptId, _prompt) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  pauseRun(taskId: string, runId: string, _projectId?: string | null) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId, status: 'paused', pauseReason: 'process-interrupted', resumable: true });
  },
  stopActiveSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ kind: 'session-cancelled', run: null, session: fallback ?? null });
  },
  submitManualCheck(_projectId, taskId, runId, _roundId, _nodeId, _attemptId, _outcome) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  retryRun(taskId: string, runId: string) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  getLogPage(query: LogQueryInput) {
    return Promise.resolve(mockLogPage(query));
  },
  getAcpSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _query, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(fallback ?? null);
  },
  subscribeAcpSessionUpdates() {
    return Promise.resolve(() => {});
  },
  subscribeConversationRunStateUpdates() {
    return Promise.resolve(() => {});
  },
  subscribeInterventionNavigate() {
    return Promise.resolve(() => {});
  },
  submitConversationPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _prompt, _promptId, fallback, _outerNodeId, _outerAttemptId, _attachmentPaths) {
    return Promise.resolve({ kind: 'acp-session', session: fallback ?? null, run: null });
  },
  sendAcpPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _prompt, _promptId, fallback, _outerNodeId, _outerAttemptId, _attachmentPaths) {
    return Promise.resolve(fallback ?? null);
  },
  setAcpSessionModel(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _modelId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  setAcpSessionPermissionMode(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _permissionModeId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  setAcpSessionConfigOption(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _optionId, _optionValue, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  respondAcpPermission(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _requestId, _optionId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(fallback ?? null);
  },
  respondElicitation(_projectId: string | null | undefined, _taskId: string, _runId: string, _roundId: string, _nodeId: string, _attemptId: string, _elicitationId: string, _action: string, _content?: Record<string, unknown> | null, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return Promise.resolve();
  },
  cancelAcpSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(fallback ?? null);
  },
  getAcpRawFrames(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, query, _outerNodeId, _outerAttemptId) {
    const empty: AcpRawFramePageVm = {
      items: [],
      page: query?.page ?? 0,
      pageSize: query?.pageSize ?? 100,
      total: 0,
      hasPrevious: false,
      hasNext: false,
      order: query?.order ?? 'desc',
      search: query?.search ?? null,
      kind: query?.kind ?? null,
      direction: query?.direction ?? null,
    };
    return Promise.resolve(empty);
  },
  showArtifact(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, name, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ ...mockContent, title: name });
  },
  showAttachment(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, name, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ ...mockContent, title: name, kind: 'attachment' });
  },
  showConversationAttachment(_projectId: string, _taskId: string, name: string) {
    if (/\.(png|jpe?g|webp|gif|bmp)$/i.test(name)) {
      return Promise.resolve({
        ...mockContent,
        title: name,
        kind: 'input-attachment',
        content: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
        metadata: { mimeType: 'image/png', isImage: true, encoding: 'data-url' },
      });
    }
    return Promise.resolve({ ...mockContent, title: name, kind: 'input-attachment' });
  },
  showConversationMessageAttachment(_projectId: string, _taskId: string, _runId: string, _roundId: string, _nodeId: string, _attemptId: string, name: string, _path: string, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return this.showConversationAttachment(_projectId, _taskId, name);
  },
  showWorkerRef(_taskId: string, _runId: string, _roundId: string, _nodeId: string, attemptId: string, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return Promise.resolve({ ...mockContent, title: attemptId, kind: 'worker-ref' });
  },
  saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean, verboseLogging: boolean) {
    const preferences = browserPreviewState.setPreferences({ theme, language, font, useLocalClaude, verboseLogging });
    return Promise.resolve(preferences);
  },
  saveUpdaterSettings(overrideUrl: string | null) {
    const current = browserPreviewState.getUpdaterSettings();
    const normalized = overrideUrl?.trim() ? overrideUrl.trim() : null;
    return Promise.resolve(browserPreviewState.setUpdaterSettings({
      ...current,
      overrideUrl: normalized,
      effectiveUrl: normalized ?? current.builtInUrl,
    }));
  },
  updateNotificationAttention(_input) {
    return Promise.resolve();
  },
  getMetricsSettings() {
    return Promise.resolve({
      enabled: false,
      toggleLocked: false,
      metricsBaseUrl: null,
      heartbeatEndpoint: null,
      nodeMetricsEndpoint: null,
      apiKeySet: false,
    });
  },
  saveMetricsSettings(_enabled: boolean, _metricsBaseUrl: string | null, _apiKey: string | null) {
    return this.getMetricsSettings();
  },
  getUpdateStatus() {
    return Promise.resolve(browserPreviewState.getUpdateStatus());
  },
  markSettingsUpdateSeen(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, settingsEntrySeenVersion: version }));
  },
  markSettingsAdvancedUpdateSeen(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, settingsAdvancedSeenVersion: version }));
  },
  dismissUpdateAnnouncement(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, announcementClosedVersion: version }));
  },
  checkUpdateManual() {
    return Promise.resolve(browserPreviewState.setUpdateStatus({
      status: 'error',
      checkedAt: localTimestamp(),
      update: null,
      error: { code: 'updater.check-failed', params: { message: 'Browser preview cannot check desktop updates.' } },
      background: false,
    }));
  },
  downloadAndInstallUpdate() {
    return Promise.resolve();
  },
  // ── Conversation UI mocks ──
  saveDesktopUiMode(_mode) {
    return Promise.resolve();
  },
  getConversationSidebar() {
    const sidebar: ConversationSidebarVm = {
      workspaces: [{ projectId: 'default', workspacePath: '/default', name: 'Default Workspace' }],
      pinnedTasks: [],
      tasksByWorkspace: { default: [] },
    };
    return Promise.resolve(sidebar);
  },
  listScheduledTasks(projectId) {
    return Promise.resolve(browserScheduledTasks
      .filter((task) => !projectId || task.projectId === projectId)
      .map((task) => ({ ...task })));
  },
  setScheduledTaskEnabled(_projectId, scheduledTaskId, enabled) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId);
    if (task) {
      task.enabled = enabled;
      task.status = enabled ? 'enabled' : 'paused';
      task.updatedAt = new Date().toISOString();
      emitBrowserScheduledTaskUpdated(task);
      return Promise.resolve({ ...task });
    }
    return Promise.resolve({ id: scheduledTaskId, projectId: 'default', workspaceName: 'Default Workspace', title: '示例定时任务', enabled, mode: 'direct', sessionPolicy: 'new', schedule: '', scheduleLabel: '', timezoneLabel: '中国（上海）', nextAt: null, status: enabled ? 'enabled' : 'paused', lastTriggerAt: null, lastTriggerStatus: null, lastTriggerLabel: '尚未运行', createdAt: '', updatedAt: '' });
  },
  createScheduledTask(input) {
    const now = new Date().toISOString();
    const id = `scheduled-${Date.now()}-${++browserScheduledTaskSequence}`;
    const timezone = input.schedule.kind === 'At' || input.schedule.kind === 'Every' || input.schedule.kind === 'Repeat' || input.schedule.kind === 'Cron'
      ? input.schedule.timezone ?? 'Asia/Shanghai'
      : 'Asia/Shanghai';
    const task: ScheduledTaskVm = { id, projectId: input.projectId, workspaceName: input.projectId === 'default' ? 'Default Workspace' : input.projectId, title: input.content.split(/\r?\n/)[0].slice(0, 48), enabled: true, mode: input.runMode, sessionPolicy: input.sessionPolicy ?? 'new', schedule: browserScheduleLabel(input.schedule), scheduleLabel: browserScheduleLabel(input.schedule), timezoneLabel: browserTimezoneLabel(timezone), nextAt: null, status: 'enabled', lastTriggerAt: null, lastTriggerStatus: null, lastTriggerLabel: '尚未运行', createdAt: now, updatedAt: now };
    const definition: ScheduledTaskEditVm = {
      scheduledTaskId: id,
      projectId: input.projectId,
      content: input.content,
      attachmentNames: [],
      runMode: input.runMode,
      workflowTemplateId: input.workflowTemplateId,
      includeInterview: input.includeInterview,
      directConfig: input.directConfig,
      autoConfig: input.autoConfig,
      schedule: input.schedule,
      overlapPolicy: input.overlapPolicy,
      sessionPolicy: input.sessionPolicy ?? 'new',
      directAgentType: input.directConfig?.agentType ?? null,
      expectedUpdatedAt: now,
    };
    browserScheduledTaskDefinitions.set(id, definition);
    browserScheduledOccurrences.set(id, []);
    browserScheduledTasks.push(task);
    emitBrowserScheduledTaskUpdated(task);
    return Promise.resolve({ ...task });
  },
  getScheduledTask(_projectId, scheduledTaskId) {
    const definition = browserScheduledTaskDefinitions.get(scheduledTaskId);
    return definition ? Promise.resolve(structuredClone(definition)) : browserCommandError('scheduled-task.not-found');
  },
  updateScheduledTask(input: UpdateScheduledTaskInput) {
    const definition = browserScheduledTaskDefinitions.get(input.scheduledTaskId);
    if (!definition) return browserCommandError('scheduled-task.not-found');
    if (definition.expectedUpdatedAt !== input.expectedUpdatedAt) return browserCommandError('scheduled-task.conflict');
    const now = new Date().toISOString();
    const next: ScheduledTaskEditVm = {
      ...definition,
      ...input,
      expectedUpdatedAt: now,
      directAgentType: input.directConfig?.agentType ?? definition.directAgentType ?? null,
    };
    browserScheduledTaskDefinitions.set(input.scheduledTaskId, next);
    const task = browserScheduledTasks.find((item) => item.id === input.scheduledTaskId);
    if (task) {
      const timezone = input.schedule.kind === 'At' || input.schedule.kind === 'Every' || input.schedule.kind === 'Repeat' || input.schedule.kind === 'Cron'
        ? input.schedule.timezone ?? 'Asia/Shanghai'
        : 'Asia/Shanghai';
      Object.assign(task, {
        title: input.content.split(/\r?\n/)[0].slice(0, 48),
        mode: input.runMode,
        sessionPolicy: input.sessionPolicy,
        schedule: browserScheduleLabel(input.schedule),
        scheduleLabel: browserScheduleLabel(input.schedule),
        timezoneLabel: browserTimezoneLabel(timezone),
        updatedAt: now,
      });
      emitBrowserScheduledTaskUpdated(task);
    }
    return Promise.resolve(structuredClone(next));
  },
  deleteScheduledTask(_projectId, scheduledTaskId) {
    const index = browserScheduledTasks.findIndex((task) => task.id === scheduledTaskId);
    if (index < 0) return browserCommandError('scheduled-task.not-found');
    const [task] = browserScheduledTasks.splice(index, 1);
    browserScheduledTaskDefinitions.delete(scheduledTaskId);
    browserScheduledOccurrences.delete(scheduledTaskId);
    emitBrowserScheduledTaskUpdated({ ...task, status: 'deleted' });
    return Promise.resolve();
  },
  listScheduledTaskOccurrences(projectId, scheduledTaskId, limit = 50) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    return Promise.resolve((browserScheduledOccurrences.get(scheduledTaskId) ?? []).slice(0, Math.max(1, Math.min(limit, 200))).map((occurrence) => structuredClone(occurrence)));
  },
  getScheduledTaskDiagnostics(projectId, scheduledTaskId) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    const occurrences = browserScheduledOccurrences.get(scheduledTaskId) ?? [];
    const last = occurrences[0] ?? null;
    return Promise.resolve({
      scheduledTaskId,
      projectId,
      nextAt: task.nextAt ?? null,
      lastStatus: last?.status ?? task.lastTriggerStatus ?? null,
      lastError: last?.errorCode ?? null,
      runCount: occurrences.filter((occurrence) => Boolean(occurrence.runId)).length,
      retryCount: occurrences.reduce((count, occurrence) => count + Math.max(0, occurrence.attempt - 1), 0),
      occurrences: occurrences.slice(0, 200).map((occurrence) => structuredClone(occurrence)),
    });
  },
  runScheduledTaskNow(projectId, scheduledTaskId) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    const now = new Date().toISOString();
    const occurrenceId = `occurrence-${Date.now()}-${++browserScheduledTaskSequence}`;
    const taskId = `browser-task-${scheduledTaskId}`;
    const runId = `browser-run-${Date.now()}-${browserScheduledTaskSequence}`;
    const running: ScheduledOccurrenceVm = {
      id: occurrenceId,
      scheduledTaskId,
      scheduledAt: now,
      triggerKind: 'manual',
      status: 'running',
      attempt: 1,
      errorCode: null,
      errorParams: null,
      taskId,
      runId,
      roundId: null,
      attemptId: null,
      startedAt: now,
      finishedAt: null,
    };
    const history = browserScheduledOccurrences.get(scheduledTaskId) ?? [];
    browserScheduledOccurrences.set(scheduledTaskId, [running, ...history]);
    emitBrowserScheduledOccurrenceUpdated(running, task.projectId);
    const finished: ScheduledOccurrenceVm = { ...running, status: 'succeeded', finishedAt: new Date().toISOString() };
    browserScheduledOccurrences.set(scheduledTaskId, [finished, ...history]);
    Object.assign(task, {
      lastTriggerAt: finished.finishedAt,
      lastTriggerStatus: finished.status,
      lastTriggerLabel: '执行成功',
      updatedAt: finished.finishedAt ?? now,
    });
    emitBrowserScheduledOccurrenceUpdated(finished, task.projectId);
    emitBrowserScheduledTaskUpdated(task);
    return Promise.resolve({
      occurrence: structuredClone(finished),
      taskId,
      runId,
      roundId: null,
      attemptId: null,
    } satisfies RunScheduledTaskResultVm);
  },
  getConversationWorkspaces() {
    return Promise.resolve([{ projectId: 'default', workspacePath: '/default', name: 'Default Workspace' }]);
  },
  getConversationRun(_projectId, _taskId, runId) {
    if (runId === 'run-051') return Promise.resolve(mockErrorBlockedConversationRun);
    if (runId === 'run-052') return Promise.resolve(browserCompletedConversationRun());
    const created = browserConversationRuns.get(runId);
    if (created) return Promise.resolve(created);
    const run: ConversationRunVm = {
      projectId: 'default',
      taskId: 'mock-task',
      runId,
      title: 'Mock Task',
      autoTitle: true,
      runMode: 'auto',
      runStatus: 'completed',
      sessionTree: { rounds: [], selectedSessionKey: null },
      selectedSession: null,
      activeSessions: [],
      artifacts: [],
      attachments: [],
      inputAttachments: [],
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      runtimeErrorMessage: null,
    };
    return Promise.resolve(run);
  },
  switchConversationSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _outerNodeId, _outerAttemptId) {
    if (_runId === 'run-051') return Promise.resolve({ selectedSession: mockErrorBlockedConversationSession, artifacts: [], attachments: [] });
    if (_runId === 'run-052') return Promise.resolve({ selectedSession: browserCompletedConversationRun().selectedSession, artifacts: [], attachments: [] });
    return Promise.resolve({ selectedSession: null, artifacts: [], attachments: [] });
  },
  validateConversationCreate(_input) {
    return Promise.resolve({ valid: true, missingItems: [] });
  },
  createConversationRun(input) {
    const run: ConversationRunVm = {
      projectId: input.projectId,
      taskId: `task-${Date.now()}`,
      runId: `run-${Date.now()}`,
      title: input.content.slice(0, 12) || 'New Task',
      autoTitle: true,
      runMode: input.runMode,
      directConfig: input.directConfig,
      agentIdentity: input.directConfig ? browserAgentIdentity(input.directConfig.agentType) : null,
      lastActivityAt: new Date().toISOString(),
      runStatus: 'running',
      sessionTree: { rounds: [], selectedSessionKey: null },
      selectedSession: null,
      activeSessions: [],
      artifacts: [],
      attachments: [],
      inputAttachments: [],
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      runtimeErrorMessage: null,
    };
    browserConversationRuns.set(run.runId, run);
    return Promise.resolve(run);
  },
  rerunConversationTask(_projectId, _taskId) {
    return this.createConversationRun({ projectId: _projectId, content: 'Rerun', runMode: 'auto' });
  },
  updateTaskMetadata() {
    return Promise.resolve();
  },
  deleteConversationTask(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  pinConversation(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  unpinConversation(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  reorderPinnedConversations(_pins) {
    return this.getConversationSidebar();
  },
  searchConversationTasks(_query, _limit) {
    return Promise.resolve([]);
  },
  getConversationRunMode(_projectId) {
    return Promise.resolve({ mode: 'auto' });
  },
  saveConversationRunMode() {
    return Promise.resolve();
  },
  chooseConversationWorkspace() {
    const ws: ConversationWorkspaceVm = { projectId: 'default', workspacePath: '/default', name: 'Default Workspace' };
    return Promise.resolve(ws);
  },
  addConversationWorkspace() {
    return this.getConversationSidebar();
  },
  removeConversationWorkspace(_projectId) {
    return this.getConversationSidebar();
  },
  syncConversationWorkspace(_workspacePath) {
    return this.getConversationSidebar();
  },
  saveConversationPreference(_key, _value) {
    return Promise.resolve();
  },
  saveLastConversationWorkspace(_projectId) {
    return Promise.resolve();
  },
  pickAttachmentFiles() {
    return Promise.resolve([]);
  },
  materializeConversationAttachments(files) {
    return Promise.resolve(files.map((file, index) => ({
      path: `browser-memory://attachments/${Date.now()}-${index}-${encodeURIComponent(file.name)}`,
      name: file.name,
      size: file.size,
    })));
  },
  getSupportedAttachmentExtensions() {
    return Promise.resolve([
      "png", "jpg", "jpeg", "webp", "gif", "bmp",
      "txt", "md", "json", "jsonl", "csv",
      "html", "htm", "css", "js", "ts", "tsx", "jsx",
      "rs", "py", "go", "java", "c", "h", "cpp", "hpp",
      "yaml", "yml", "xml", "toml", "log", "sql", "sh", "bash", "zsh",
    ]);
  },
  openInFileManager(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve();
  },
  // MCP & SKILL stubs
  listMcpServers() { return Promise.resolve([]); },
  addMcpServer(_jsonContent: string) { return Promise.resolve([]); },
  updateMcpServer(_id: string, _jsonContent: string) { return Promise.resolve([]); },
  deleteMcpServer(_id: string) { return Promise.resolve([]); },
  toggleMcpServer(_id: string, _enabled: boolean) { return Promise.resolve([]); },
  checkMcpServerHealth(_id: string) { return Promise.resolve({ status: 'unknown' }); },
  listMcpTools(_id: string) { return Promise.resolve([]); },
  listSkills() { return Promise.resolve({ global: [], project: [] }); },
  listProjectSkills(_workspacePath: string) { return Promise.resolve([]); },
  readSkill(_name: string, _source: string, _workspacePath?: string | null, _directoryPath?: string | null) { return Promise.resolve({ meta: { name: '', description: '', source: 'global' as const, directoryPath: '', agentSource: '.gold-band', loadWarnings: [], syncedAgentTypes: [] }, body: '' }); },
  writeSkill(_name: string, _source: string, _content: string, _workspacePath?: string | null, _oldName?: string | null, _directoryPath?: string | null, _syncTargets?: string[] | null) { return Promise.resolve({ global: [], project: [] }); },
  deleteSkill(_name: string, _source: string) { return Promise.resolve({ global: [], project: [] }); },
  updateSkillSyncTargets(_name: string, _source: string, _workspacePath: string | null | undefined, _directoryPath: string, _syncTargets: string[]) { return Promise.resolve({ global: [], project: [] }); },
  getSkillSyncStatus(_name: string, _directoryPath: string, _workspacePath?: string | null) { return Promise.resolve([]); },
  checkSkillNameConflict(_name: string, _source: string, _workspacePath?: string | null, _oldName?: string | null, _directoryPath?: string | null, _syncTargets?: string[] | null) { return Promise.resolve([] as string[]); },
};

function browserProfileId() {
  return `pf-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function browserCommandError(code: string, params: Record<string, unknown> = {}) {
  return Promise.reject({ code, params });
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

void mockWorkflowTemplates;
void toRoundSelectionInput;
void mockBootstrap;
void mockContent;
void mockAgentRegistry;
void mockTaskDetail;
void mockWorkflow;
void mockRoundDetail;
void mockRunDetail;
void mockLogPage;
void mockTaskList;
