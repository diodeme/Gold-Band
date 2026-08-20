import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useMemo, useRef, useState, useTransition } from 'react';
import {
  checkUpdateManual,
  chooseWorkspace,
  continueRun,
  createConversationRun,
  createScheduledTask,
  createTask,
  deleteConversationTask,
  dismissUpdateAnnouncement,
  downloadAndInstallUpdate,
  getAgentRegistry,
  getConversationRun,
  getConversationRunMode,
  getConversationSidebar,
  acknowledgeConversationTerminalResult,
  getProfiles,
  getWorkflowTemplates,
  switchConversationSession,
  markSettingsAdvancedUpdateSeen,
  markSettingsUpdateSeen,
  getAppBootstrap,
  getRoundDetail,
  getTaskList,
  getWorkflow,
  clearDesktopAvatar,
  importDesktopWallpaper,
  pauseRun,
  pinConversation,
  rerunConversationTask,
  removeRecentWorkspace,
  saveDesktopPreferences,
  saveDesktopAvatar,
  saveDesktopAvatarShape,
  saveDesktopWallpaperOpacity,
  saveUpdaterSettings,
  saveTaskWorkflow,
  selectRecentWorkspace,
  selectRecentDesktopAvatar,
  selectRecentDesktopWallpaper,
  restoreThemeDesktopWallpaper,
  startRun,
  unpinConversation,
  updateTaskMetadata,
  validateConversationCreate,
  addConversationWorkspace,
  removeConversationWorkspace,
  syncConversationWorkspace,
  saveConversationRunMode,
  saveConversationPreference,
  saveLastConversationWorkspace,
  getGitCapability,
  subscribeAcpSessionUpdates,
  subscribeConversationRunStateUpdates,
  subscribeConversationTerminalResultUpdates,
  subscribeScheduledTaskUpdates,
  updateNotificationAttention,
  recordActivity,
} from './api';
import { isTauriRuntime } from './api/shared';
import { registerHeartbeatActivityListeners } from './lib/heartbeat-activity';
import { prefetchScheduledRuntimeSettings } from '@/components/scheduled-tasks/useScheduledRuntimeSettings';
import {
  applyConversationSidebarRunLifecycle,
  applyConversationSidebarRunStateUpdate,
  applyConversationSidebarTaskActivity,
  applyConversationSidebarTerminalResultAcknowledgement,
  applyConversationSidebarTerminalResultUpdate,
  conversationTaskActivityFromLifecycle,
  conversationTaskActivityFromUpdate,
} from './lib/conversation-sidebar-activity';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Breadcrumbs } from './components/Breadcrumbs';
import { Button } from '@/components/ui/button';
import { X } from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { WindowCloseCoordinator } from '@/components/WindowCloseCoordinator';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Markdown } from '@/components/prompt-kit/markdown';
import { Shell } from './components/Shell';
import { BrandLoadingState } from '@/components/BrandLoadingState';
import i18n, { displayAppError, i18nLanguage } from './i18n';
import {
  isRuntimeControlledConversationLifecycle,
  planConversationAcpRunUpdate,
  resolveConversationEventSelectedSessionKey,
  resolveConversationRefreshSelectedSessionKey,
  resolveConversationRunReentrySelection,
  type ConversationSessionFollowMode,
  type ConversationSessionFollowState,
} from '@/lib/conversation-session-follow';
import {
  applyConversationBackgroundSessionRuntimeSnapshot,
  applyConversationSelectedSessionSnapshot,
  conversationSessionKeyFromParts,
  findConversationLeafByKey,
  isConversationActiveLifecycle,
  isConversationActiveStatus,
  mergeConversationRunSnapshot,
  type ConversationRunSnapshotSource,
} from '@/lib/conversation-run-snapshot';
import { useTranslation } from 'react-i18next';
import { AgentManagementPage } from './pages/AgentManagementPage';
import { ContextManagementPage } from './pages/ContextManagementPage';
import { ConversationHomePage } from './pages/ConversationHomePage';
import {
  ConversationComposerDraftBoundary,
  type ConversationComposerDraftBoundaryHandle,
} from '@/components/conversation/ConversationComposerDraftBoundary';
import { ConversationRunPage } from './pages/ConversationRunPage';
import { ConversationSearchDialog } from './components/conversation/ConversationSearchDialog';
import {
  prioritizeConversationSidebarWorkspace,
  type ConversationSidebarWorkspaceRevealRequest,
} from './components/conversation/ConversationSidebar';
import { RunModeManagementPage } from './pages/RunModeManagementPage';
import { ScheduledTaskManagementPage } from './pages/ScheduledTaskManagementPage';
import { ScheduledTaskDetailPage } from './pages/ScheduledTaskDetailPage';
import { PersonalAnalyticsPage } from './pages/PersonalAnalyticsPage';
import { RoundDetailPage } from './pages/RoundDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { createInitialCreateTaskDraft, TaskListPage, type CreateTaskDraftState } from './pages/TaskListPage';
import { resetConversationComposerDraft } from '@/lib/conversation-composer-draft';
import { GitRequirementDialog } from '@/components/git/GitRequirementDialog';
import { resolveConversationWorkspaceRemovalTransition } from '@/lib/conversation-workspace-removal';
import { WorkflowPage } from './pages/WorkflowPage';
import { WorkspaceSelectPage } from './pages/WorkspaceSelectPage';
import { pushRoute, replaceRoute, routeFromPath, taskListPage, conversationHomePage } from './routes';
import { applyAppearance, applyPersonalization, applyWallpaperPersonalization, defaultPersonalizationPreference, resolveAppearance, syncDesktopWindowSurface } from './theme';
import { useInterventionNotifications } from './lib/use-intervention-notifications';
import { useScheduledNotifications } from './lib/use-scheduled-notifications';
import { scheduledNotificationNavigation } from './lib/scheduled-task-notifications';
import {
  shouldRunWorkbenchBackgroundRefresh,
  WORKBENCH_BACKGROUND_REFRESH_HIDDEN_INTERVAL_MS,
  WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS,
} from '@/lib/workbench-background-refresh';
import {
  shouldAutoOpenWorkspacePicker,
  shouldRenderWorkspacePicker,
} from '@/lib/workspace-picker-scope';
import {
  conversationRunModeForWorkspace,
  conversationRunModeOrDefault,
  mergeConversationRunMode,
  setConversationRunModeForWorkspace,
  type ConversationRunModesByWorkspace,
} from '@/lib/conversation-run-mode-config';
import { ConversationRunModePersistence } from '@/lib/conversation-run-mode-persistence';
import {
  CONVERSATION_WORK_LOCATION_PREFERENCE_KEY,
  conversationWorkLocationForProject,
  parseConversationWorkLocationPreference,
  setConversationWorkLocationForProject,
} from '@/lib/conversation-work-location';
import { createDefaultAvatarPreferences } from '@/lib/avatar';
import { createDefaultWallpaperPreferences } from '@/lib/wallpaper';
import { AvatarPreferencesProvider } from '@/components/avatar/AvatarPreferencesContext';
import {
  ConversationWorkspaceStore,
  createConversationWorkspaceScope,
  createDraftConversationWorkspaceScope,
} from '@/components/workspace/right-workspace-context';
import { conversationPageForSearchResult } from '@/lib/conversation-search';
import {
  beginConversationSessionSelection,
  conversationPageForSession,
  conversationPageForIntervention,
  conversationPageMatchesRun,
  conversationTerminalResultAcknowledgementTarget,
  findConversationLeafForPage,
  isConversationRunNavigationLoading,
  resolveConversationHomeWorkspaceId,
  shouldCommitConversationNavigation,
} from '@/lib/conversation-navigation';
import { preloadConversationTurnFileChangeSets } from '@/lib/turn-file-change-set-cache';
import { ConversationRunCache, conversationRunCacheKey } from '@/lib/conversation-run-cache';
import {
  applyConversationTaskSnapshot,
  findConversationTask,
} from '@/lib/conversation-task-state';
import {
  INITIAL_DESKTOP_WINDOW_MINIMUM_SYNC_STATE,
  syncDesktopWindowMinimum,
  type DesktopWindowMinimumSyncState,
} from '@/lib/desktop-window-layout';
import {
  FALLBACK_WORKSPACE_FILES,
  FALLBACK_WORKSPACE_LAYOUT,
  workspaceLayoutProfileForSurface,
} from '@/components/workspace/workspace-layout';
import type {
  AgentRegistryVm,
  AppBootstrapVm,
  AppConfigVm,
  AppInfoVm,
  ConversationAttemptLifecycleVm,
  ConversationTaskActivityVm,
  ConversationTaskRowVm,
  ConversationPage,
  ConversationRunModeVm,
  ConversationWorkLocation,
  ConversationRunVm,
  ConversationSessionLeafVm,
  ConversationSessionTreeVm,
  ConversationTreeNodeVm,
  WorkflowTemplateStore,
  ConversationSidebarVm,
  CreateTaskInput,
  ProfileVm,
  DesktopLanguage,
  AppearancePreference,
  PersonalizationPreference,
  DesktopUiMode,
  MetricsSettingsVm,
  PreferencesVm,
  UpdateBadgeStateVm,
  PrimaryModule,
  RoundDetailVm,
  RoundSelection,
  TaskListVm,
  TaskPage,
  UpdateStatusVm,
  UpdaterSettingsVm,
  WorkflowDsl,
  WorkflowModelBindings,
  WorkflowVm,
  InterventionNavigateEventVm,
  AvatarKind,
  AvatarShape,
  SaveDesktopAvatarInput,
  ResolvedColorScheme,
  WorkflowRepairTarget,
  WallpaperPreferencesVm,
} from './types';

export function workflowRepairTargetFromMissingItems(
  missingItems: Array<{ params: Record<string, unknown> }>,
): WorkflowRepairTarget | null {
  for (const item of missingItems) {
    const workflowTemplateId = item.params.workflowTemplateId;
    const nodeId = item.params.nodeId;
    if (typeof workflowTemplateId === 'string' && workflowTemplateId.trim()
      && typeof nodeId === 'string' && nodeId.trim()) {
      return { workflowTemplateId, nodeId };
    }
  }
  return null;
}

function findScheduledLinkedLeaf(
  tree: ConversationSessionTreeVm,
  roundId: string,
  attemptId?: string,
): ConversationSessionLeafVm | null {
  const walkNode = (node: ConversationTreeNodeVm): ConversationSessionLeafVm | null => {
    const leaf = node.attempts.find((attempt) =>
      attempt.roundId === roundId && (!attemptId || attempt.attemptId === attemptId));
    if (leaf) return leaf;
    for (const child of node.outerNodes ?? []) {
      const nested = walkNode(child);
      if (nested) return nested;
    }
    return null;
  };
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      const leaf = walkNode(node);
      if (leaf) return leaf;
    }
  }
  return null;
}

const defaultPreferences: PreferencesVm = { appearance: { schemaVersion: 2, themeId: 'builtin.gold-band', colorScheme: 'system', visualQualityByTheme: {} }, personalization: defaultPersonalizationPreference, language: 'zh-cn', useLocalClaude: false, verboseLogging: false, avatars: createDefaultAvatarPreferences(), wallpapers: createDefaultWallpaperPreferences() };
const defaultUpdaterSettings: UpdaterSettingsVm = {
  channel: 'default',
  builtInUrl: 'https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json',
  overrideUrl: null,
  effectiveUrl: 'https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json',
  pollIntervalMinutes: 240,
};

const defaultMetricsSettings: MetricsSettingsVm = {
  enabled: false,
  toggleLocked: false,
  metricsBaseUrl: null,
  heartbeatEndpoint: null,
  nodeMetricsEndpoint: null,
  apiKeySet: false,
};
const defaultUpdateStatus: UpdateStatusVm = {
  status: 'idle',
  checkedAt: null,
  update: null,
  error: null,
  background: false,
};
const defaultUpdateBadges: UpdateBadgeStateVm = {
  settingsEntrySeenVersion: null,
  settingsAdvancedSeenVersion: null,
};
const defaultAppInfo: AppInfoVm = {
  channel: 'default',
  feedbackEnabled: false,
  appName: 'Gold Band',
  appKey: 'gold-band',
  configDirName: '.gold-band',
};
const defaultAppConfig: AppConfigVm = {
  acpSessionTitleRefreshEnabled: false,
  acpChatEventPageSize: 360,
  conversationInlineContentMaxBytes: 64_000,
  conversationInlineImageMaxBytes: 4 * 1024 * 1024,
  conversationInlineImageMaxDimension: 2_560,
  turnFiles: { cardPreviewLimit: 3 },
  workspaceLayout: FALLBACK_WORKSPACE_LAYOUT,
  workspaceFiles: FALLBACK_WORKSPACE_FILES,
};
type RefreshMode = 'initial' | 'manual' | 'background';
type VisibleRefreshMode = Exclude<RefreshMode, 'background'>;

function conversationTreeHasSessionKey(tree: ConversationSessionTreeVm, key: string) {
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (conversationSessionKeyFromParts(attempt) === key) return true;
      }
      for (const outer of node.outerNodes ?? []) {
        for (const attempt of outer.attempts) {
          if (conversationSessionKeyFromParts(attempt) === key) return true;
        }
      }
    }
  }
  return false;
}

function gitRequirementStatus(error: unknown) {
  if (!error || typeof error !== 'object') return null;
  const code = (error as { code?: unknown }).code;
  switch (code) {
    case 'run.git-not-installed':
      return 'not-installed' as const;
    case 'run.git-repository-required':
      return 'repository-required' as const;
    case 'run.git-head-required':
      return 'head-required' as const;
    case 'run.git-worktree-required':
      return 'worktree-required' as const;
    case 'run.git-repository-unavailable':
      return 'repository-unavailable' as const;
    default:
      return null;
  }
}

function selectedConversationLeaf(tree?: ConversationSessionTreeVm | null) {
  const selectedKey = tree?.selectedSessionKey;
  if (!tree || !selectedKey) return null;
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (conversationSessionKeyFromParts(attempt) === selectedKey) return attempt;
      }
      for (const outer of node.outerNodes ?? []) {
        for (const attempt of outer.attempts) {
          if (conversationSessionKeyFromParts(attempt) === selectedKey) return attempt;
        }
      }
    }
  }
  return null;
}

export function App() {
  const { t } = useTranslation();
  const initialRoute = routeFromPath(window.location.pathname);
  const [uiMode, setUiMode] = useState<DesktopUiMode>(initialRoute.uiMode);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    const stored = typeof localStorage !== 'undefined' && localStorage.getItem('gold-band-sidebar-collapsed') === 'true';
    return stored;
  });
  const [bootstrap, setBootstrap] = useState<AppBootstrapVm | null>(null);
  const windowRevealedRef = useRef(false);
  const windowLayoutSyncRef = useRef<Promise<void>>(Promise.resolve());
  const windowMinimumSyncStateRef = useRef<DesktopWindowMinimumSyncState>(
    INITIAL_DESKTOP_WINDOW_MINIMUM_SYNC_STATE,
  );
  const activeWindowLayoutRef = useRef<{
    layout: AppConfigVm['workspaceLayout'];
    profile: AppConfigVm['workspaceLayout']['conversation'];
  } | null>(null);
  const [primaryModule, setPrimaryModule] = useState<PrimaryModule>(initialRoute.module);
  const [taskPage, setTaskPage] = useState<TaskPage>(initialRoute.taskPage);
  const [conversationPage, setConversationPage] = useState<ConversationPage>(initialRoute.conversationPage);
  const [workflowRepairTarget, setWorkflowRepairTarget] = useState<WorkflowRepairTarget | null>(null);
  const conversationPageRef = useRef<ConversationPage>(initialRoute.conversationPage);
  const conversationStopRequestRef = useRef(0);
  const conversationRunStopPendingRef = useRef(false);
  const [conversationSidebar, setConversationSidebar] = useState<ConversationSidebarVm>({ workspaces: [], pinnedTasks: [], tasksByWorkspace: {} });
  const conversationSidebarRef = useRef<ConversationSidebarVm>({ workspaces: [], pinnedTasks: [], tasksByWorkspace: {} });
  const conversationRunStateRefreshRef = useRef<Parameters<typeof subscribeConversationRunStateUpdates>[0] | null>(null);
  const conversationAcpSessionRefreshRef = useRef<Parameters<typeof subscribeAcpSessionUpdates>[0] | null>(null);
  const conversationTerminalAcknowledgementsInFlightRef = useRef(new Set<string>());
  const [conversationSearchOpen, setConversationSearchOpen] = useState(false);
  const [conversationRunModesByWorkspace, setConversationRunModesByWorkspace] = useState<ConversationRunModesByWorkspace>({});
  const conversationRunModesRef = useRef<ConversationRunModesByWorkspace>({});
  const conversationRunModeRequestRef = useRef(new Map<string, number>());
  const [conversationRunModePersistence] = useState(
    () => new ConversationRunModePersistence(saveConversationRunMode),
  );
  const [conversationWorkspaceStore] = useState(() => new ConversationWorkspaceStore());
  const [conversationRunCache] = useState(() => new ConversationRunCache());
  const [conversationRun, setConversationRun] = useState<ConversationRunVm | null>(null);
  const conversationRunRef = useRef<ConversationRunVm | null>(null);
  const conversationNavigationRequestRef = useRef(0);
  const presentedConversationPage = conversationPage;
  const conversationSessionFollowRef = useRef<ConversationSessionFollowState>({
    runKey: null,
    mode: 'auto',
    selectedSessionKey: null,
    version: 0,
  });
  const conversationSelectedSessionKeyRef = useRef<string | null>(null);

  useEffect(() => {
    conversationPageRef.current = conversationPage;
  }, [conversationPage]);
  const [forceSettingsTab, setForceSettingsTab] = useState<'advanced' | null>(null);
  const [conversationWorkflowTemplates, setConversationWorkflowTemplates] = useState<WorkflowTemplateStore | null>(null);
  const [, startTransition] = useTransition();

  const updateConversationSessionFollow = useCallback((
    mode: ConversationSessionFollowMode,
    selectedSessionKey?: string | null,
    scopeRun?: ConversationRunVm | null,
  ) => {
    const run = scopeRun ?? conversationRunRef.current;
    const nextSelectedSessionKey = selectedSessionKey !== undefined
      ? selectedSessionKey
      : conversationSelectedSessionKeyRef.current ?? null;
    const nextFollowState: ConversationSessionFollowState = {
      runKey: run ? conversationRunCacheKey(run) : conversationSessionFollowRef.current.runKey,
      mode,
      selectedSessionKey: nextSelectedSessionKey,
      version: conversationSessionFollowRef.current.version + 1,
    };
    conversationSessionFollowRef.current = nextFollowState;
    if (run) {
      conversationRunCache.store(run, {
        followMode: mode,
        selectedSessionKey: nextSelectedSessionKey,
      });
    }
  }, [conversationRunCache]);

  const applyConversationRunSnapshot = useCallback((
    snapshot: ConversationRunVm,
    source: ConversationRunSnapshotSource,
    options?: { selectedSessionKey?: string | null; preserveSelectedSession?: boolean },
  ) => {
    setConversationRun((current) => {
      const merged = mergeConversationRunSnapshot(current, snapshot, source, options);
      const mergedRunKey = conversationRunCacheKey(merged);
      const previousFollowState = conversationSessionFollowRef.current;
      const scopedFollowState = previousFollowState.runKey === mergedRunKey
        ? previousFollowState
        : {
            runKey: mergedRunKey,
            mode: 'auto' as const,
            selectedSessionKey: null,
            version: previousFollowState.version + 1,
          };
      const nextSelectedSessionKey = merged.sessionTree.selectedSessionKey ?? null;
      conversationRunRef.current = merged;
      conversationSelectedSessionKeyRef.current = nextSelectedSessionKey;
      conversationSessionFollowRef.current = {
        ...scopedFollowState,
        selectedSessionKey: nextSelectedSessionKey,
      };
      conversationRunCache.store(merged, {
        followMode: scopedFollowState.mode,
        selectedSessionKey: nextSelectedSessionKey,
      });
      return merged;
    });
  }, [conversationRunCache]);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(null);
  const activeWorkspaceIdRef = useRef<string | null>(null);
  const [draftConversationWorkspaceId, setDraftConversationWorkspaceId] = useState<string | null>(null);
  const workspaceRevealRequestIdRef = useRef(0);
  const [workspaceRevealRequest, setWorkspaceRevealRequest] = useState<ConversationSidebarWorkspaceRevealRequest | null>(null);

  const applyConversationSidebar = useCallback((sidebar: ConversationSidebarVm, projectId?: string | null) => {
    const activeProjectId = projectId ?? activeWorkspaceIdRef.current ?? sidebar.lastActiveWorkspaceId ?? null;
    const nextSidebar = prioritizeConversationSidebarWorkspace(sidebar, activeProjectId);
    conversationSidebarRef.current = nextSidebar;
    setConversationSidebar(nextSidebar);
  }, []);

  const applyConversationTask = useCallback((task: ConversationTaskRowVm) => {
    setConversationSidebar((current) => {
      const next = applyConversationTaskSnapshot(current, task);
      conversationSidebarRef.current = next;
      return next;
    });
  }, []);

  const applyConversationTaskActivity = useCallback((
    projectId: string,
    taskId: string,
    activity: ConversationTaskActivityVm | null,
  ) => {
    setConversationSidebar((current) => {
      const next = applyConversationSidebarTaskActivity(
        current,
        projectId,
        taskId,
        activity,
      );
      conversationSidebarRef.current = next;
      return next;
    });
  }, []);

  const applyConversationLifecycleSnapshotToSidebar = useCallback((
    projectId: string,
    taskId: string,
    runId: string,
    lifecycle: Parameters<typeof conversationTaskActivityFromLifecycle>[0],
    activity = conversationTaskActivityFromLifecycle(lifecycle),
  ) => {
    setConversationSidebar((current) => {
      const withActivity = applyConversationSidebarTaskActivity(
        current,
        projectId,
        taskId,
        activity,
      );
      const next = applyConversationSidebarRunLifecycle(
        withActivity,
        projectId,
        taskId,
        runId,
        lifecycle,
      );
      conversationSidebarRef.current = next;
      return next;
    });
  }, []);

  // Derive active workspace: explicit local state > persisted lastActiveWorkspaceId > first workspace
  const effectiveWorkspaceId =
    activeWorkspaceId
    ?? conversationSidebar.lastActiveWorkspaceId
    ?? conversationSidebar.workspaces[0]?.projectId
    ?? 'default';

  const rememberConversationWorkspace = useCallback((projectId: string) => {
    activeWorkspaceIdRef.current = projectId;
    setActiveWorkspaceId(projectId);
    workspaceRevealRequestIdRef.current += 1;
    setWorkspaceRevealRequest({ projectId, requestId: workspaceRevealRequestIdRef.current });
    setConversationSidebar((prev) => {
      const next = prioritizeConversationSidebarWorkspace(prev, projectId);
      conversationSidebarRef.current = next;
      return next;
    });
    saveLastConversationWorkspace(projectId).catch(() => {});
  }, []);
  const activeWorkspace = conversationSidebar.workspaces.find((w) => w.projectId === effectiveWorkspaceId)
    ?? conversationSidebar.workspaces[0];
  const draftWorkspace = conversationSidebar.workspaces.find((w) => w.projectId === draftConversationWorkspaceId)
    ?? activeWorkspace
    ?? conversationSidebar.workspaces[0];
  const conversationWorkspaceContextId = presentedConversationPage.kind === 'conversation-run'
    ? presentedConversationPage.projectId
    : (draftConversationWorkspaceId ?? effectiveWorkspaceId);
  const defaultExpandedWorkspaceId = presentedConversationPage.kind === 'conversation-run'
    ? presentedConversationPage.projectId
    : effectiveWorkspaceId;
  const defaultProjectId = draftWorkspace?.projectId ?? 'default';
  const defaultWorkspaceName = draftWorkspace?.name ?? 'Default Workspace';
  const conversationWorkLocationPreference = parseConversationWorkLocationPreference(
    conversationSidebar.preferences?.[CONVERSATION_WORK_LOCATION_PREFERENCE_KEY],
  );
  const conversationWorkLocation = conversationWorkLocationForProject(
    conversationWorkLocationPreference,
    defaultProjectId,
  );
  const conversationRunMode = conversationRunModeForWorkspace(conversationRunModesByWorkspace, defaultProjectId);
  const [roundSelection, setRoundSelection] = useState<RoundSelection>({ kind: 'round' });
  const [agentRegistry, setAgentRegistry] = useState<AgentRegistryVm | null>(null);
  const [profiles, setProfiles] = useState<ProfileVm[]>([]);
  const [taskList, setTaskList] = useState<TaskListVm | null>(null);
  const [createTaskDraft, setCreateTaskDraft] = useState<CreateTaskDraftState>(() => createInitialCreateTaskDraft());
  const composerDraftRef = useRef<ConversationComposerDraftBoundaryHandle | null>(null);
  const [workflow, setWorkflow] = useState<WorkflowVm | null>(null);
  const [roundDetail, setRoundDetail] = useState<RoundDetailVm | null>(null);
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const [loading, setLoading] = useState<VisibleRefreshMode | null>(null);
  const [busy, setBusy] = useState(false);
  const preferenceSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const conversationWorkLocationSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const preferenceSaveGenerationRef = useRef(0);
  const [downloadProgress, setDownloadProgress] = useState<{ downloaded: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [gitRequirement, setGitRequirement] = useState<{
    status: 'not-installed' | 'repository-required' | 'head-required' | 'worktree-required' | 'repository-unavailable';
    runKind: 'auto' | 'workflow' | 'worktree';
    projectId?: string | null;
  } | null>(null);

  const persistConversationWorkLocation = useCallback((
    location: ConversationWorkLocation,
    projectId: string,
  ) => {
    const save = conversationWorkLocationSaveQueueRef.current.then(async () => {
      const currentSidebar = conversationSidebarRef.current;
      const currentPreference = parseConversationWorkLocationPreference(
        currentSidebar.preferences?.[CONVERSATION_WORK_LOCATION_PREFERENCE_KEY],
      );
      const nextPreference = setConversationWorkLocationForProject(
        currentPreference,
        projectId,
        location,
      );
      await saveConversationPreference(CONVERSATION_WORK_LOCATION_PREFERENCE_KEY, nextPreference);
      setConversationSidebar((current) => {
        const next = {
          ...current,
          preferences: {
            ...current.preferences,
            [CONVERSATION_WORK_LOCATION_PREFERENCE_KEY]: nextPreference,
          },
        };
        conversationSidebarRef.current = next;
        return next;
      });
    });
    conversationWorkLocationSaveQueueRef.current = save.catch(() => {});
    return save;
  }, []);

  const selectConversationWorkLocation = useCallback(async (
    location: ConversationWorkLocation,
    projectId: string,
  ) => {
    try {
      if (location === 'main') {
        await persistConversationWorkLocation(location, projectId);
        return;
      }
      const capability = await getGitCapability(projectId);
      if (capability.status !== 'ready') {
        setGitRequirement({
          status: capability.status,
          runKind: 'worktree',
          projectId,
        });
        return;
      }
      await persistConversationWorkLocation(location, projectId);
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }, [persistConversationWorkLocation, t]);

  const loadConversationRunMode = useCallback((projectId: string) => {
    const requestId = (conversationRunModeRequestRef.current.get(projectId) ?? 0) + 1;
    conversationRunModeRequestRef.current.set(projectId, requestId);
    return conversationRunModePersistence.waitFor(projectId)
      .then(() => getConversationRunMode(projectId))
      .then((mode) => {
        const nextMode = conversationRunModeOrDefault(mode);
        if (requestId === conversationRunModeRequestRef.current.get(projectId)) {
          const nextModes = setConversationRunModeForWorkspace(
            conversationRunModesRef.current,
            projectId,
            nextMode,
          );
          conversationRunModesRef.current = nextModes;
          setConversationRunModesByWorkspace(nextModes);
        }
        return nextMode;
      })
      .catch(() => undefined);
  }, [conversationRunModePersistence]);
  const [updateAnnouncementOpen, setUpdateAnnouncementOpen] = useState(false);
  const backgroundRefreshInFlightRef = useRef(false);

  useEffect(() => {
    conversationRunRef.current = conversationRun;
    conversationSelectedSessionKeyRef.current = conversationRun?.sessionTree.selectedSessionKey ?? null;
  }, [conversationRun]);

  const handleConversationAutoFollowChange = useCallback((enabled: boolean) => {
    if (conversationPage.kind !== 'conversation-run') return;
    const mode: ConversationSessionFollowMode = enabled ? 'auto' : 'manual';
    updateConversationSessionFollow(mode, conversationSelectedSessionKeyRef.current);
  }, [conversationPage, updateConversationSessionFollow]);

  useEffect(() => {
    return registerHeartbeatActivityListeners(() => {
      recordActivity().catch(() => {});
    });
  }, []);

  const loadProfiles = useCallback(async () => {
    const result = await getProfiles();
    setProfiles(result.profiles);
    return result.profiles;
  }, []);

  const preferences = bootstrap?.preferences ?? defaultPreferences;
  const updaterSettings = bootstrap?.updaterSettings ?? defaultUpdaterSettings;
  const metricsSettings = bootstrap?.metricsSettings ?? null;
  const updateStatus = bootstrap?.updateStatus ?? defaultUpdateStatus;
  const updateBadges = bootstrap?.updateBadges ?? defaultUpdateBadges;
  const persistedAvailableUpdate = bootstrap?.persistedAvailableUpdate ?? null;
  const effectiveAvailableUpdate = updateStatus.update ?? persistedAvailableUpdate;
  const availableUpdateVersion = effectiveAvailableUpdate?.version ?? null;
  const showSettingsUpdateDot = availableUpdateVersion !== null && updateBadges.settingsEntrySeenVersion !== availableUpdateVersion;
  const showSettingsAdvancedUpdateDot = availableUpdateVersion !== null && updateBadges.settingsAdvancedSeenVersion !== availableUpdateVersion;
  const showUpdatesSectionDot = availableUpdateVersion !== null;
  const appInfo = bootstrap?.appInfo ?? defaultAppInfo;
  const appConfig = bootstrap?.appConfig ?? defaultAppConfig;
  const activeWorkspaceLayoutProfile = useMemo(
    () => workspaceLayoutProfileForSurface({
      uiMode,
      conversationPage: presentedConversationPage,
      primaryModule,
      layout: appConfig.workspaceLayout,
    }),
    [appConfig.workspaceLayout, presentedConversationPage, primaryModule, uiMode],
  );
  activeWindowLayoutRef.current = {
    layout: appConfig.workspaceLayout,
    profile: activeWorkspaceLayoutProfile,
  };
  const shouldShowUpdateAnnouncement = useMemo(
    () => availableUpdateVersion !== null && updateBadges.announcementClosedVersion !== availableUpdateVersion,
    [availableUpdateVersion, updateBadges.announcementClosedVersion],
  );
  useEffect(() => {
    applyAppearance(preferences.appearance);
  }, [preferences.appearance]);

  useEffect(() => {
    if (preferences.appearance.colorScheme !== 'system') return undefined;
    const colorScheme = window.matchMedia('(prefers-color-scheme: dark)');
    const syncSystemTheme = () => applyAppearance(preferences.appearance);
    colorScheme.addEventListener('change', syncSystemTheme);
    return () => colorScheme.removeEventListener('change', syncSystemTheme);
  }, [preferences.appearance]);

  useEffect(() => {
    applyPersonalization(preferences.personalization);
    applyWallpaperPersonalization(preferences.personalization.wallpaper, preferences.wallpapers);
  }, [preferences.appearance, preferences.personalization, preferences.wallpapers]);

  useEffect(() => {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem('gold-band-sidebar-collapsed', sidebarCollapsed ? 'true' : 'false');
  }, [sidebarCollapsed]);

  useEffect(() => {
    if (!isTauriRuntime() || !bootstrap) return;
    let cancelled = false;
    const revealWindow = async () => {
      const appWindow = getCurrentWindow();
      if (!windowRevealedRef.current) {
        await syncDesktopWindowSurface(resolveAppearance(preferences.appearance));
      }
      await syncDesktopWindowMinimum(
        appWindow,
        appConfig.workspaceLayout,
        activeWorkspaceLayoutProfile,
        windowMinimumSyncStateRef.current,
      ).then((state) => {
        windowMinimumSyncStateRef.current = state;
      }).catch(() => {});
      if (cancelled || windowRevealedRef.current) return;
      windowRevealedRef.current = true;
      await appWindow.show().catch(() => {
        windowRevealedRef.current = false;
      });
    };
    windowLayoutSyncRef.current = windowLayoutSyncRef.current
      .catch(() => {})
      .then(revealWindow);
    return () => {
      cancelled = true;
    };
  }, [activeWorkspaceLayoutProfile, appConfig.workspaceLayout, bootstrap, preferences.appearance]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    const appWindow = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;

    appWindow.onResized(() => {
      if (!windowMinimumSyncStateRef.current.pending) return;
      windowLayoutSyncRef.current = windowLayoutSyncRef.current
        .catch(() => {})
        .then(async () => {
          if (!active || !windowMinimumSyncStateRef.current.pending) return;
          const target = activeWindowLayoutRef.current;
          if (!target) return;
          const state = await syncDesktopWindowMinimum(
            appWindow,
            target.layout,
            target.profile,
            windowMinimumSyncStateRef.current,
          );
          if (active) windowMinimumSyncStateRef.current = state;
        })
        .catch(() => {});
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    }).catch(() => {});

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let active = true;
    const appWindow = getCurrentWindow();
    const convPage = conversationPage.kind === 'conversation-run' ? conversationPage : null;
    const sync = async () => {
      try {
        const [windowFocused, windowMinimized, windowVisible] = await Promise.all([
          appWindow.isFocused(),
          appWindow.isMinimized(),
          appWindow.isVisible(),
        ]);
        if (!active) return;
        const leaf = convPage
          ? selectedConversationLeaf(conversationRunRef.current?.sessionTree)
          : null;
        await updateNotificationAttention({
          windowFocused: windowFocused && !document.hidden,
          windowMinimized,
          windowVisible,
          projectId: convPage?.projectId ?? null,
          taskId: convPage?.taskId ?? null,
          runId: convPage?.runId ?? null,
          roundId: leaf?.roundId ?? null,
          nodeId: leaf?.nodeId ?? null,
          attemptId: leaf?.attemptId ?? null,
          outerNodeId: leaf?.outerNodeId ?? null,
          outerAttemptId: leaf?.outerAttemptId ?? null,
        });
      } catch {
      }
    };
    void sync();
    const onVisibilityChange = () => void sync();
    document.addEventListener('visibilitychange', onVisibilityChange);
    let unlistenFocus: (() => void) | undefined;
    void appWindow.onFocusChanged(() => void sync()).then((dispose) => {
      if (active) {
        unlistenFocus = dispose;
      } else {
        dispose();
      }
    });
    return () => {
      active = false;
      document.removeEventListener('visibilitychange', onVisibilityChange);
      unlistenFocus?.();
    };
  }, [conversationPage, conversationRun]);

  useEffect(() => {
    void i18n.changeLanguage(i18nLanguage(preferences.language));
  }, [preferences.language]);

  useEffect(() => {
    if (primaryModule !== 'settings' && conversationPage.kind !== 'settings') {
      setForceSettingsTab(null);
    }
  }, [primaryModule, conversationPage.kind]);

  useEffect(() => {
    replaceRoute(primaryModule, taskPage, uiMode === 'conversation' ? conversationPage : undefined);
    const onPopState = () => {
      const nextRoute = routeFromPath(window.location.pathname);
      setUiMode(nextRoute.uiMode);
      setPrimaryModule(nextRoute.module);
      setTaskPage(nextRoute.taskPage);
      setConversationPage(nextRoute.conversationPage);
      setRoundSelection({ kind: 'round' });
      setWorkspacePickerOpen(false);
    };
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  useEffect(() => {
    getAppBootstrap()
      .then((bootstrap) => {
        setBootstrap(bootstrap);
        // 静默预取定时任务运行时设置，让首次进入「设置 → 定时任务」也免加载闪烁。
        void prefetchScheduledRuntimeSettings();
        if (shouldAutoOpenWorkspacePicker(bootstrap, uiMode)) {
          setWorkspacePickerOpen(true);
        }
      })
      .catch((err) => setError(displayAppError(t, err)));
  }, [t, uiMode]);

  // Load conversation sidebar data when in conversation mode
  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    getConversationSidebar()
      .then((sidebar) => applyConversationSidebar(sidebar))
      .catch(() => {}); // Silently fail - sidebar will show empty state
  }, [applyConversationSidebar, bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return undefined;
    let active = true;
    let dispose: (() => void) | undefined;
    void subscribeConversationRunStateUpdates((event) => {
      if (!active) return;
      setConversationSidebar((current) => {
        const next = applyConversationSidebarRunStateUpdate(current, event);
        if (next !== current) conversationSidebarRef.current = next;
        return next;
      });
      conversationRunStateRefreshRef.current?.(event);
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    }).catch(() => {});
    return () => {
      active = false;
      dispose?.();
    };
  }, [bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return undefined;
    let active = true;
    let dispose: (() => void) | undefined;
    void subscribeConversationTerminalResultUpdates((event) => {
      if (!active) return;
      setConversationSidebar((current) => {
        const next = applyConversationSidebarTerminalResultUpdate(current, event);
        if (next !== current) conversationSidebarRef.current = next;
        return next;
      });
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    }).catch(() => {});
    return () => {
      active = false;
      dispose?.();
    };
  }, [bootstrap, uiMode]);

  useEffect(() => {
    if (uiMode !== 'conversation') return;
    const acknowledgementTarget = conversationTerminalResultAcknowledgementTarget(
      conversationSidebar,
      conversationPage,
      conversationRun,
    );
    if (!acknowledgementTarget) return;
    const acknowledgementKey = `${acknowledgementTarget.projectId}:${acknowledgementTarget.taskId}:${acknowledgementTarget.eventId}`;
    if (conversationTerminalAcknowledgementsInFlightRef.current.has(acknowledgementKey)) return;
    conversationTerminalAcknowledgementsInFlightRef.current.add(acknowledgementKey);
    void acknowledgeConversationTerminalResult(
      acknowledgementTarget.projectId,
      acknowledgementTarget.taskId,
      acknowledgementTarget.eventId,
    ).then((acknowledgement) => {
      setConversationSidebar((current) => {
        const next = applyConversationSidebarTerminalResultAcknowledgement(
          current,
          acknowledgementTarget.projectId,
          acknowledgementTarget.taskId,
          acknowledgementTarget.eventId,
          acknowledgement.unreadTerminalResult,
        );
        if (next !== current) conversationSidebarRef.current = next;
        return next;
      });
    }).catch(() => {}).finally(() => {
      conversationTerminalAcknowledgementsInFlightRef.current.delete(acknowledgementKey);
    });
  }, [conversationPage, conversationRun, conversationSidebar, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return undefined;
    let active = true;
    let dispose: (() => void) | undefined;
    void subscribeAcpSessionUpdates((event) => {
      if (!active) return;
      const projectId = event.projectId?.trim();
      if (projectId) {
        const sidebarActivity = conversationTaskActivityFromUpdate(event);
        if (event.lifecycle) {
          applyConversationLifecycleSnapshotToSidebar(
            projectId,
            event.taskId,
            event.runId,
            event.lifecycle,
            sidebarActivity === undefined
              ? conversationTaskActivityFromLifecycle(event.lifecycle)
              : sidebarActivity,
          );
        } else if (sidebarActivity !== undefined) {
          applyConversationTaskActivity(projectId, event.taskId, sidebarActivity);
        }
      }
      conversationAcpSessionRefreshRef.current?.(event);
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    }).catch(() => {});
    return () => {
      active = false;
      dispose?.();
    };
  }, [applyConversationLifecycleSnapshotToSidebar, applyConversationTaskActivity, bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    let active = true;
    let dispose: (() => void) | undefined;
    void subscribeScheduledTaskUpdates(() => {
      void getConversationSidebar()
        .then((sidebar) => {
          if (active) applyConversationSidebar(sidebar);
        })
        .catch(() => {});
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    }).catch(() => {});
    return () => {
      active = false;
      dispose?.();
    };
  }, [applyConversationSidebar, bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap) return;
    getAgentRegistry().then(setAgentRegistry).catch(() => {});
  }, [bootstrap]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    loadProfiles().catch(() => setProfiles([]));
    getWorkflowTemplates().then(setConversationWorkflowTemplates).catch(() => {});
  }, [bootstrap, loadProfiles, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || !defaultProjectId) return;
    void loadConversationRunMode(defaultProjectId);
  }, [bootstrap, uiMode, defaultProjectId, loadConversationRunMode]);

  // Load conversation run when navigating to a run page
  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || conversationPage.kind !== 'conversation-run') return;
    const { projectId, taskId, runId, roundId } = conversationPage;
    const targetRunKey = conversationRunCacheKey(conversationPage);
    if (conversationSessionFollowRef.current.runKey !== targetRunKey) {
      conversationSessionFollowRef.current = {
        runKey: targetRunKey,
        mode: roundId ? 'manual' : 'auto',
        selectedSessionKey: null,
        version: conversationSessionFollowRef.current.version + 1,
      };
      conversationSelectedSessionKeyRef.current = null;
    }
    const followStateAtRequest = conversationSessionFollowRef.current;
    const requestedSelectedSessionKey = !roundId && followStateAtRequest.mode === 'manual'
      ? followStateAtRequest.selectedSessionKey
      : null;
    const requestId = conversationNavigationRequestRef.current + 1;
    conversationNavigationRequestRef.current = requestId;
    let cancelled = false;
    getConversationRun(projectId, taskId, runId, requestedSelectedSessionKey)
      .then(async (run) => {
        if (!roundId) return { run, explicitSelectedSessionKey: null };
        const leaf = findConversationLeafForPage(run.sessionTree, conversationPage);
        if (!leaf) return { run, explicitSelectedSessionKey: null };
        const selectedSessionKey = conversationSessionKeyFromParts(leaf);
        if (run.sessionTree.selectedSessionKey === selectedSessionKey && run.selectedSession) {
          return { run, explicitSelectedSessionKey: selectedSessionKey };
        }
        return {
          run: await getConversationRun(projectId, taskId, runId, selectedSessionKey),
          explicitSelectedSessionKey: selectedSessionKey,
        };
      })
      .then(({ run, explicitSelectedSessionKey }) => {
        if (cancelled || !shouldCommitConversationNavigation(
          requestId,
          conversationNavigationRequestRef.current,
          conversationPageRef.current,
          run,
        )) return;
        const latestFollowState = conversationSessionFollowRef.current.runKey === targetRunKey
          ? conversationSessionFollowRef.current
          : followStateAtRequest;
        const followSelectionChanged = latestFollowState.version !== followStateAtRequest.version
          && (
            latestFollowState.mode !== followStateAtRequest.mode
            || latestFollowState.selectedSessionKey !== followStateAtRequest.selectedSessionKey
          );
        const selectionPlan = resolveConversationRunReentrySelection({
          followMode: latestFollowState.mode,
          rememberedSelectedSessionKey: latestFollowState.mode === 'manual' || followSelectionChanged
            ? latestFollowState.selectedSessionKey
            : null,
          explicitSelectedSessionKey: followSelectionChanged ? null : explicitSelectedSessionKey,
          defaultSelectedSessionKey: run.sessionTree.selectedSessionKey ?? null,
          hasSessionKey: (key) => Boolean(findConversationLeafByKey(run.sessionTree, key)),
        });
        conversationSessionFollowRef.current = {
          runKey: targetRunKey,
          mode: selectionPlan.followMode,
          selectedSessionKey: selectionPlan.selectedSessionKey,
          version: latestFollowState.version + (
            latestFollowState.mode !== selectionPlan.followMode
            || latestFollowState.selectedSessionKey !== selectionPlan.selectedSessionKey
              ? 1
              : 0
          ),
        };
        conversationSelectedSessionKeyRef.current = selectionPlan.selectedSessionKey;
        applyConversationRunSnapshot(run, 'initial-load', {
          selectedSessionKey: selectionPlan.selectedSessionKey,
          preserveSelectedSession: selectionPlan.preserveSelectedSession,
        });
        void preloadConversationTurnFileChangeSets(run);
      })
      .catch((err: unknown) => {
        if (cancelled || requestId !== conversationNavigationRequestRef.current) return;
        if (conversationPageRef.current.kind === 'conversation-run'
          && conversationPageRef.current.projectId === projectId
          && conversationPageRef.current.taskId === taskId
          && conversationPageRef.current.runId === runId) {
          setError(displayAppError(t, err));
        }
      });
    return () => { cancelled = true; };
  }, [applyConversationRunSnapshot, bootstrap, t, uiMode, conversationPage]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || conversationPage.kind !== 'conversation-run') return undefined;
    if (!conversationPageMatchesRun(conversationPage, conversationRun)) return undefined;
    let active = true;
    let refreshTimer: number | null = null;
    let refreshInFlight = false;
    let refreshAgain = false;
    let pendingEventSessionKey: string | null = null;
    let pendingEventRuntimeControlled = false;
    const { projectId, taskId, runId } = conversationPage;

    const refreshConversationRun = () => {
      refreshTimer = null;
      if (refreshInFlight) {
        refreshAgain = true;
        return;
      }
      refreshInFlight = true;
      const followStateAtRequest = conversationSessionFollowRef.current;
      const currentSelectedKey = conversationSelectedSessionKeyRef.current
        ?? conversationRunRef.current?.sessionTree.selectedSessionKey
        ?? null;
      const currentRun = conversationRunRef.current;
      const currentSelectedLeaf = currentRun
        ? findConversationLeafByKey(currentRun.sessionTree, currentSelectedKey)
        : null;
      const selectedKey = resolveConversationRefreshSelectedSessionKey({
        followMode: followStateAtRequest.mode,
        pendingEventSessionKey,
        currentSelectedKey,
        currentSelectedRuntimeControlled: isRuntimeControlledConversationLifecycle(currentSelectedLeaf?.lifecycle),
        pendingEventRuntimeControlled,
      });
      pendingEventSessionKey = null;
      pendingEventRuntimeControlled = false;
      getConversationRun(projectId, taskId, runId, selectedKey)
        .then((run) => {
          if (!active) return;
          const latestFollowState = conversationSessionFollowRef.current;
          const latestSelectedKey = latestFollowState.selectedSessionKey
            ?? conversationSelectedSessionKeyRef.current
            ?? null;
          const latestRun = conversationRunRef.current;
          const latestSelectedLeaf = latestRun
            ? findConversationLeafByKey(latestRun.sessionTree, latestSelectedKey)
            : null;
          const responseTargetLeaf = findConversationLeafByKey(run.sessionTree, selectedKey);
          const effectiveSelectedKey = resolveConversationRefreshSelectedSessionKey({
            followMode: latestFollowState.mode,
            pendingEventSessionKey: selectedKey,
            currentSelectedKey: latestSelectedKey,
            currentSelectedRuntimeControlled: isRuntimeControlledConversationLifecycle(latestSelectedLeaf?.lifecycle),
            pendingEventRuntimeControlled: isRuntimeControlledConversationLifecycle(responseTargetLeaf?.lifecycle),
          });
          applyConversationRunSnapshot(run, 'live-refresh', {
            selectedSessionKey: effectiveSelectedKey,
            preserveSelectedSession: latestFollowState.mode === 'manual',
          });
        })
        .catch(() => {})
        .finally(() => {
          refreshInFlight = false;
          if (!active || !refreshAgain) return;
          refreshAgain = false;
          if (refreshTimer === null) {
            refreshTimer = window.setTimeout(refreshConversationRun, 0);
          }
        });
    };

    const queueConversationRunRefresh = (
      sessionKey?: string | null,
      runtimeControlled = false,
      delayMs = 120,
    ) => {
      if (sessionKey !== undefined) {
        pendingEventSessionKey = sessionKey;
        pendingEventRuntimeControlled = runtimeControlled;
      }
      if (refreshTimer !== null) {
        if (delayMs === 0) {
          window.clearTimeout(refreshTimer);
          refreshTimer = window.setTimeout(refreshConversationRun, 0);
        }
        return;
      }
      refreshTimer = window.setTimeout(refreshConversationRun, delayMs);
    };

    const refreshSelectedRunFromStateEvent: Parameters<typeof subscribeConversationRunStateUpdates>[0] = (event) => {
      if (!active) return;
      if (event.projectId !== projectId || event.taskId !== taskId || event.runId !== runId) return;
      const sessionKey = conversationSessionKeyFromParts(event);
      const currentRun = conversationRunRef.current;
      const eventLeaf = currentRun
        ? findConversationLeafByKey(currentRun.sessionTree, sessionKey)
        : null;
      queueConversationRunRefresh(
        sessionKey,
        isRuntimeControlledConversationLifecycle(eventLeaf?.lifecycle),
        0,
      );
    };
    conversationRunStateRefreshRef.current = refreshSelectedRunFromStateEvent;

    const refreshSelectedRunFromAcpEvent: Parameters<typeof subscribeAcpSessionUpdates>[0] = (event) => {
      if (!active) return;
      if (event.projectId !== projectId || event.taskId !== taskId || event.runId !== runId) return;
      const sessionKey = conversationSessionKeyFromParts(event);
      const currentRun = conversationRunRef.current;
      const currentSelectedKey = conversationSelectedSessionKeyRef.current
        ?? currentRun?.sessionTree.selectedSessionKey
        ?? null;
      const treeHasSession = currentRun
        ? conversationTreeHasSessionKey(currentRun.sessionTree, sessionKey)
        : false;
      const alreadySelected = currentSelectedKey === sessionKey;
      const currentSelectedLeaf = currentRun
        ? findConversationLeafByKey(currentRun.sessionTree, currentSelectedKey)
        : null;
      const incomingLeaf = currentRun
        ? findConversationLeafByKey(currentRun.sessionTree, sessionKey)
        : null;
      const currentSelectedRuntimeControlled = isRuntimeControlledConversationLifecycle(currentSelectedLeaf?.lifecycle);
      const incomingRuntimeControlled = isRuntimeControlledConversationLifecycle(
        event.lifecycle ?? incomingLeaf?.lifecycle,
      );
      const currentSelectedActive = Boolean(
        currentSelectedLeaf && (isConversationActiveLifecycle(currentSelectedLeaf.lifecycle) || isConversationActiveStatus(currentSelectedLeaf.status)),
      );
      const hasRuntimeSnapshot = Boolean(event.session || event.lifecycle);
      const incomingActive = event.lifecycle
        ? isConversationActiveLifecycle(event.lifecycle)
        : event.session
          ? isConversationActiveStatus(event.session.status)
          : Boolean(event.event);
      const followState = conversationSessionFollowRef.current;
      const followPending = followState.mode === 'auto'
        && Boolean(currentSelectedKey)
        && !currentSelectedActive
        && incomingActive
        && currentSelectedRuntimeControlled
        && incomingRuntimeControlled;
      const updatePlan = planConversationAcpRunUpdate({
        treeHasSession,
        alreadySelected,
        hasRuntimeSnapshot,
        hasLiveEvent: Boolean(event.event),
        sessionStatus: event.lifecycle?.displayStatus ?? event.session?.status,
        pendingPermissionCount: event.session?.pendingPermissions?.length ?? 0,
        followPending,
      });
      if (hasRuntimeSnapshot && updatePlan.patchSelectedSession) {
        setConversationRun((current) => {
          const patched = applyConversationSelectedSessionSnapshot(current, event);
          conversationRunRef.current = patched;
          return patched;
        });
      }
      if (hasRuntimeSnapshot && updatePlan.patchBackgroundSession) {
        setConversationRun((current) => {
          const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, event);
          conversationRunRef.current = patched;
          return patched;
        });
      }
      if (!updatePlan.queueRunRefresh) {
        return;
      }
      queueConversationRunRefresh(resolveConversationEventSelectedSessionKey({
        currentSelectedKey,
        incomingSessionKey: sessionKey,
        followMode: followState.mode,
        currentSelectedActive,
        incomingActive,
        currentSelectedRuntimeControlled,
        incomingRuntimeControlled,
      }), incomingRuntimeControlled);
    };
    conversationAcpSessionRefreshRef.current = refreshSelectedRunFromAcpEvent;

    return () => {
      active = false;
      if (refreshTimer !== null) window.clearTimeout(refreshTimer);
      if (conversationRunStateRefreshRef.current === refreshSelectedRunFromStateEvent) {
        conversationRunStateRefreshRef.current = null;
      }
      if (conversationAcpSessionRefreshRef.current === refreshSelectedRunFromAcpEvent) {
        conversationAcpSessionRefreshRef.current = null;
      }
    };
  }, [applyConversationRunSnapshot, bootstrap, uiMode, conversationPage, conversationRun?.projectId, conversationRun?.taskId, conversationRun?.runId]);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let active = true;
    let refreshInFlight = false;
    let refreshPending = false;
    let unlisten: (() => void) | undefined;

    const refreshAgentRegistry = async () => {
      if (refreshInFlight) {
        refreshPending = true;
        return;
      }
      refreshInFlight = true;
      try {
        const next = await getAgentRegistry();
        if (active) setAgentRegistry(next);
      } catch {
        // The periodic/background diagnostic remains best-effort; manual refresh still surfaces errors.
      } finally {
        refreshInFlight = false;
        if (active && refreshPending) {
          refreshPending = false;
          void refreshAgentRegistry();
        }
      }
    };

    void listen('gold-band://agent-registry-updated', () => {
      if (active) void refreshAgentRegistry();
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<UpdateStatusVm>('gold-band://update-status', (event) => {
      if (!active) return;
      setBootstrap((current) => current ? {
        ...current,
        updateStatus: event.payload,
        persistedAvailableUpdate: event.payload.update ?? (event.payload.status === 'available' ? current.persistedAvailableUpdate : null),
      } : current);
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<{ downloaded: number; total: number | null }>('gold-band://update-download-progress', (event) => {
      if (!active) return;
      setDownloadProgress(event.payload);
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const resetWorkspaceViews = () => {
    setTaskPage({ kind: 'task-list' });
    setRoundSelection({ kind: 'round' });
    setAgentRegistry(null);
    setTaskList(null);
    setCreateTaskDraft(createInitialCreateTaskDraft());
    setWorkflow(null);
    setRoundDetail(null);
    setPrimaryModule('task-orchestration');
    setWorkspacePickerOpen(false);
  };

  const hasPageData = primaryModule === 'agent-management'
    ? agentRegistry !== null
    : primaryModule === 'knowledge-base'
      ? true
      : taskPage.kind === 'task-list'
      ? taskList !== null
      : taskPage.kind === 'workflow'
        ? workflow !== null
        : roundDetail !== null;

  const refresh = useCallback(async (mode: RefreshMode = 'manual') => {
    if (!bootstrap) return;
    if (mode === 'background' && backgroundRefreshInFlightRef.current) return;
    if (mode === 'background') {
      backgroundRefreshInFlightRef.current = true;
    } else {
      setLoading(mode);
    }
    setError(null);
    try {
      if (primaryModule === 'agent-management') {
        setAgentRegistry(await getAgentRegistry());
      } else if (primaryModule === 'knowledge-base') {
        return;
      } else if (taskPage.kind === 'task-list') {
        setTaskList(await getTaskList());
      } else if (taskPage.kind === 'workflow') {
        setWorkflow(await getWorkflow(taskPage.taskId));
      } else if (taskPage.kind === 'round-detail') {
        setRoundDetail(await getRoundDetail(taskPage.taskId, taskPage.runId, taskPage.roundId, roundSelection));
      }
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      if (mode === 'background') {
        backgroundRefreshInFlightRef.current = false;
      } else {
        setLoading(null);
      }
    }
  }, [bootstrap, primaryModule, roundSelection, t, taskPage]);

  useEffect(() => {
    if (uiMode !== 'workbench') return;
    void refresh(hasPageData ? 'background' : 'initial');
  }, [hasPageData, refresh, uiMode]);

  useEffect(() => {
    if (!shouldRunWorkbenchBackgroundRefresh({
      uiMode,
      bootstrapReady: Boolean(bootstrap),
      hasPageData,
    })) return undefined;
    let intervalId: number;
    const startInterval = (ms: number) => {
      window.clearInterval(intervalId);
      intervalId = window.setInterval(() => void refresh('background'), ms) as unknown as number;
    };
    startInterval(WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS);
    const onVisibilityChange = () => {
      startInterval(document.hidden ? WORKBENCH_BACKGROUND_REFRESH_HIDDEN_INTERVAL_MS : WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS);
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      window.clearInterval(intervalId);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [bootstrap, hasPageData, refresh, uiMode]);

  const openProfileManagement = () => {
    setWorkspacePickerOpen(false);
    setPrimaryModule('knowledge-base');
    pushRoute('knowledge-base', taskPage);
  };

  const navigate = (page: TaskPage) => {
    setPrimaryModule('task-orchestration');
    setWorkspacePickerOpen(false);
    setTaskPage(page);
    setRoundSelection({ kind: 'round' });
    pushRoute('task-orchestration', page);
  };

  // 在会话模式 sessionTree 中按 (roundId, nodeId, attemptId) 匹配出叶子（含 outer 字段）。
  const findSessionLeaf = (
    tree: ConversationSessionTreeVm | undefined | null,
    roundId: string,
    nodeId: string,
    attemptId: string,
  ): ConversationSessionLeafVm | null => {
    if (!tree) return null;
    const walkNode = (node: ConversationTreeNodeVm): ConversationSessionLeafVm | null => {
      if (node.nodeId === nodeId) {
        const hit = node.attempts.find((a) => a.attemptId === attemptId && a.roundId === roundId);
        if (hit) return hit;
      }
      for (const child of node.outerNodes ?? []) {
        const found = walkNode(child);
        if (found) return found;
      }
      return null;
    };
    for (const round of tree.rounds) {
      for (const node of round.nodes) {
        const found = walkNode(node);
        if (found) return found;
      }
    }
    return null;
  };

  // 干预弹窗「查看详情」导航：按 uiMode deep link 到对应节点。
  const handleInterventionNavigate = useCallback(async (event: InterventionNavigateEventVm) => {
    setWorkspacePickerOpen(false);
    if ('scheduledTaskId' in event) {
      const target = scheduledNotificationNavigation(event);
      if (target.kind === 'scheduled-detail') {
        const page: ConversationPage = {
          kind: 'scheduled-task-detail',
          projectId: target.projectId,
          scheduledTaskId: target.scheduledTaskId,
        };
        onSelectConversation(page);
        return;
      }
      const page: ConversationPage = {
        kind: 'conversation-run',
        projectId: target.projectId,
        taskId: target.taskId,
        runId: target.runId,
        roundId: target.roundId ?? undefined,
        attemptId: target.attemptId ?? undefined,
      };
      onSelectConversation(page);
      return;
    }
    if (uiMode !== 'conversation') {
      // 工作台模式：定位到 round-detail 并选中节点。
      setPrimaryModule('task-orchestration');
      const page: TaskPage = { kind: 'round-detail', taskId: event.taskId, runId: event.runId, roundId: event.roundId };
      setTaskPage(page);
      setRoundSelection({ kind: 'node', nodeId: event.nodeId, attemptId: event.attemptId });
      pushRoute('task-orchestration', page);
      return;
    }

    // 会话模式：定位到 run，并在 sessionTree 内匹配叶子后切换 session。
    const runPage = conversationPageForIntervention(event);
    const targetProjectId = event.projectId;
    onSelectConversation(runPage);

    let run = conversationRunRef.current
      && conversationPageMatchesRun(runPage, conversationRunRef.current)
      ? conversationRunRef.current
      : null;
    if (!run) {
      try {
        const loaded = await getConversationRun(targetProjectId, event.taskId, event.runId, null);
        applyConversationRunSnapshot(loaded, 'initial-load', { selectedSessionKey: null, preserveSelectedSession: false });
        run = loaded;
      } catch {
        return;
      }
    }

    const resolvedRun = run;

    const leaf = findSessionLeaf(resolvedRun.sessionTree, event.roundId, event.nodeId, event.attemptId);
    if (!leaf) return;
    const key = conversationSessionKeyFromParts({
      roundId: leaf.roundId,
      nodeId: leaf.nodeId,
      attemptId: leaf.attemptId,
      outerNodeId: leaf.outerNodeId,
      outerAttemptId: leaf.outerAttemptId,
    });
    conversationSelectedSessionKeyRef.current = key;
    updateConversationSessionFollow('manual', key, resolvedRun);
    setConversationRun((current) => {
      const base = current && conversationPageMatchesRun(runPage, current) ? current : resolvedRun;
      const next = beginConversationSessionSelection(base, key);
      conversationRunRef.current = next;
      return next;
    });
    try {
      const switched = await switchConversationSession(
        targetProjectId,
        event.taskId,
        event.runId,
        leaf.roundId,
        leaf.nodeId,
        leaf.attemptId,
        leaf.outerNodeId,
        leaf.outerAttemptId,
      );
      if (conversationSelectedSessionKeyRef.current !== key) return;
      startTransition(() => {
        setConversationRun((prev) => {
          if (!prev || conversationSelectedSessionKeyRef.current !== key) return prev;
          const next: ConversationRunVm = {
            ...prev,
            selectedSession: switched.selectedSession,
            sessionTree: { ...prev.sessionTree, selectedSessionKey: key },
          };
          conversationRunRef.current = next;
          return next;
        });
      });
    } catch {
      // 切换 session 失败时静默：用户已在 run 页面，可手动选择。
    }
  }, [
    uiMode,
    taskPage,
    applyConversationRunSnapshot,
    updateConversationSessionFollow,
  ]);

  useInterventionNotifications(handleInterventionNavigate);
  useScheduledNotifications();

  const runAction = async <T,>(
    action: () => Promise<T>,
    options?: { surfaceError?: boolean; rethrow?: boolean },
  ) => {
    setBusy(true);
    setError(null);
    try {
      const result = await action();
      await refresh('background');
      return result;
    } catch (err) {
      const gitStatus = gitRequirementStatus(err);
      if (gitStatus) {
        setGitRequirement({ status: gitStatus, runKind: 'workflow', projectId: defaultProjectId });
      }
      if (options?.surfaceError !== false) {
        setError(gitStatus ? null : displayAppError(t, err));
      }
      if (options?.rethrow) {
        throw err;
      }
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const updateConversationRunMode = (mode: ConversationRunModeVm, projectId = defaultProjectId) => {
    const requestVersion = (conversationRunModeRequestRef.current.get(projectId) ?? 0) + 1;
    conversationRunModeRequestRef.current.set(projectId, requestVersion);
    const currentMode = conversationRunModeForWorkspace(conversationRunModesRef.current, projectId);
    const nextMode = mergeConversationRunMode(currentMode, mode);
    const nextModes = setConversationRunModeForWorkspace(
      conversationRunModesRef.current,
      projectId,
      nextMode,
    );
    conversationRunModesRef.current = nextModes;
    setConversationRunModesByWorkspace(nextModes);
    return conversationRunModePersistence.enqueue(projectId, nextMode).catch(() => {});
  };

  const onStopRun = (taskId: string, runId: string) => {
    void runAction(() => pauseRun(taskId, runId));
  };

  const onConversationPauseRun = async (projectId: string, taskId: string, runId: string) => {
    if (conversationRunStopPendingRef.current) return;
    conversationRunStopPendingRef.current = true;
    const requestVersion = conversationStopRequestRef.current + 1;
    conversationStopRequestRef.current = requestVersion;
    setError(null);
    try {
      await pauseRun(taskId, runId, projectId);
      const selectedKey = conversationRunRef.current?.sessionTree.selectedSessionKey ?? null;
      const refreshSelectedRun = conversationPageRef.current.kind === 'conversation-run'
        && conversationPageRef.current.projectId === projectId
        && conversationPageRef.current.taskId === taskId
        && conversationPageRef.current.runId === runId;
      const [refreshed, sidebar] = await Promise.all([
        refreshSelectedRun
          ? getConversationRun(projectId, taskId, runId, selectedKey)
          : Promise.resolve(null),
        getConversationSidebar(),
      ]);
      if (conversationStopRequestRef.current !== requestVersion) return;
      const currentPage = conversationPageRef.current;
      if (refreshed
        && currentPage.kind === 'conversation-run'
        && currentPage.projectId === projectId
        && currentPage.taskId === taskId
        && currentPage.runId === runId) {
        applyConversationRunSnapshot(refreshed, 'session-stopped', {
          selectedSessionKey: selectedKey,
          preserveSelectedSession: conversationSessionFollowRef.current.mode === 'manual',
        });
      }
      applyConversationSidebar(sidebar);
    } catch (err) {
      if (conversationStopRequestRef.current === requestVersion) {
        setError(displayAppError(t, err));
      }
    } finally {
      if (conversationStopRequestRef.current === requestVersion) {
        conversationRunStopPendingRef.current = false;
      }
    }
  };

  const onCreateTask = async (input: CreateTaskInput) => {
    const created = await runAction(() => createTask(input), { surfaceError: false, rethrow: true });
    if (created) setWorkflow(created);
    return created;
  };

  const onSaveTaskWorkflow = async (taskId: string, workflow: WorkflowDsl, modelBindings: WorkflowModelBindings) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await saveTaskWorkflow(undefined, taskId, workflow, modelBindings);
      setWorkflow(saved);
      return saved;
    } finally {
      setBusy(false);
    }
  };

  const applyWorkspace = (nextBootstrap: AppBootstrapVm) => {
    setBootstrap(nextBootstrap);
    resetWorkspaceViews();
    replaceRoute('task-orchestration', { kind: 'task-list' });
  };

  const onChooseWorkspace = async () => {
    setBusy(true);
    setError(null);
    try {
      const nextBootstrap = await chooseWorkspace();
      if (nextBootstrap) {
        applyWorkspace(nextBootstrap);
      }
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusy(false);
    }
  };

  const onSelectRecentWorkspace = async (workspace: string) => {
    setBusy(true);
    setError(null);
    try {
      applyWorkspace(await selectRecentWorkspace(workspace));
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusy(false);
    }
  };

  const onRemoveRecentWorkspace = async (workspace: string) => {
    setBusy(true);
    setError(null);
    try {
      const nextBootstrap = await removeRecentWorkspace(workspace);
      setBootstrap(nextBootstrap);
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusy(false);
    }
  };

  const onSavePreferences = (appearance: AppearancePreference, personalization: PersonalizationPreference, language: DesktopLanguage, useLocalClaude: boolean, verboseLogging: boolean) => {
    const generation = ++preferenceSaveGenerationRef.current;
    setBusy(true);
    const save = preferenceSaveQueueRef.current
      .catch(() => undefined)
      .then(() => saveDesktopPreferences(appearance, personalization, language, useLocalClaude, verboseLogging))
      .then((saved) => {
        if (generation !== preferenceSaveGenerationRef.current) return;
        setBootstrap((current) => current ? { ...current, preferences: saved } : current);
      })
      .catch((err) => {
        if (generation === preferenceSaveGenerationRef.current) setError(displayAppError(t, err));
      })
      .finally(() => {
        if (generation === preferenceSaveGenerationRef.current) setBusy(false);
      });
    preferenceSaveQueueRef.current = save;
  };

  const applySavedPreferences = useCallback((saved: PreferencesVm) => {
    setBootstrap((current) => current ? { ...current, preferences: saved } : current);
  }, []);

  const onSaveAvatar = useCallback(async (input: SaveDesktopAvatarInput) => {
    setError(null);
    try {
      const saved = await saveDesktopAvatar(input);
      applySavedPreferences(saved);
      return saved.avatars;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onSelectRecentAvatar = useCallback(async (kind: AvatarKind, avatarId: string) => {
    setError(null);
    try {
      const saved = await selectRecentDesktopAvatar(kind, avatarId);
      applySavedPreferences(saved);
      return saved.avatars;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onSaveAvatarShape = useCallback(async (kind: AvatarKind, shape: AvatarShape | null) => {
    setError(null);
    try {
      const saved = await saveDesktopAvatarShape(kind, shape);
      applySavedPreferences(saved);
      return saved.avatars;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onClearAvatar = useCallback(async (kind: AvatarKind) => {
    setError(null);
    try {
      const saved = await clearDesktopAvatar(kind);
      applySavedPreferences(saved);
      return saved.avatars;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onImportWallpaper = useCallback(async (colorScheme: ResolvedColorScheme): Promise<WallpaperPreferencesVm | undefined> => {
    setError(null);
    try {
      const saved = await importDesktopWallpaper(colorScheme);
      if (!saved) return undefined;
      applySavedPreferences(saved);
      return saved.wallpapers;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onSelectRecentWallpaper = useCallback(async (colorScheme: ResolvedColorScheme, wallpaperId: string): Promise<WallpaperPreferencesVm | undefined> => {
    setError(null);
    try {
      const saved = await selectRecentDesktopWallpaper(colorScheme, wallpaperId);
      applySavedPreferences(saved);
      return saved.wallpapers;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onSaveWallpaperOpacity = useCallback(async (colorScheme: ResolvedColorScheme, opacityPercent: number): Promise<WallpaperPreferencesVm | undefined> => {
    setError(null);
    try {
      const saved = await saveDesktopWallpaperOpacity(colorScheme, opacityPercent);
      applySavedPreferences(saved);
      return saved.wallpapers;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onRestoreThemeWallpaper = useCallback(async (colorScheme: ResolvedColorScheme): Promise<WallpaperPreferencesVm | undefined> => {
    setError(null);
    try {
      const saved = await restoreThemeDesktopWallpaper(colorScheme);
      applySavedPreferences(saved);
      return saved.wallpapers;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    }
  }, [applySavedPreferences, t]);

  const onSaveUpdaterSettings = async (overrideUrl: string | null) => {
    setBusy(true);
    try {
      const saved = await saveUpdaterSettings(overrideUrl);
      setBootstrap((current) => current ? { ...current, updaterSettings: saved } : current);
      return saved;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const onCheckUpdate = async () => {
    setBusy(true);
    try {
      const status = await checkUpdateManual();
      setBootstrap((current) => current ? { ...current, updateStatus: status, persistedAvailableUpdate: status.update ?? null } : current);
      return status;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const onMarkSettingsUpdateSeen = useCallback(async () => {
    if (!availableUpdateVersion) return;
    if (updateBadges.settingsEntrySeenVersion === availableUpdateVersion) return;
    try {
      const badges = await markSettingsUpdateSeen(availableUpdateVersion);
      setBootstrap((current) => current ? { ...current, updateBadges: badges } : current);
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }, [availableUpdateVersion, t, updateBadges.settingsEntrySeenVersion]);

  const onMarkSettingsAdvancedUpdateSeen = useCallback(async () => {
    if (!availableUpdateVersion) return;
    if (updateBadges.settingsAdvancedSeenVersion === availableUpdateVersion) return;
    try {
      const badges = await markSettingsAdvancedUpdateSeen(availableUpdateVersion);
      setBootstrap((current) => current ? { ...current, updateBadges: badges } : current);
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }, [availableUpdateVersion, t, updateBadges.settingsAdvancedSeenVersion]);

  const onDismissUpdateAnnouncement = useCallback(async () => {
    if (!availableUpdateVersion) return;
    if (updateBadges.announcementClosedVersion === availableUpdateVersion) return;
    try {
      const badges = await dismissUpdateAnnouncement(availableUpdateVersion);
      setBootstrap((current) => current ? { ...current, updateBadges: badges } : current);
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }, [availableUpdateVersion, t, updateBadges.announcementClosedVersion]);

  const onOpenUpdateAnnouncement = () => {
    setUpdateAnnouncementOpen(true);
  };

  const onGoToSettingsUpdate = () => {
    setUpdateAnnouncementOpen(false);
    setWorkspacePickerOpen(false);
    setForceSettingsTab('advanced');
    if (uiMode === 'conversation') {
      setConversationPage({ kind: 'settings' });
      pushRoute(primaryModule, taskPage, { kind: 'settings' });
    } else {
      setPrimaryModule('settings');
      pushRoute('settings', taskPage);
    }
  };

  const onInstallUpdate = async () => {
    setBusy(true);
    setDownloadProgress(null);
    setBootstrap((current) => current ? { ...current, updateStatus: { ...current.updateStatus, status: 'downloading', error: null } } : current);
    try {
      await downloadAndInstallUpdate();
    } catch (err) {
      setDownloadProgress(null);
      setBootstrap((current) => current ? { ...current, updateStatus: { ...current.updateStatus, status: 'available', error: { code: 'updater.install-failed', params: { message: String(err) } } } } : current);
      setError(displayAppError(t, err));
    } finally {
      setBusy(false);
    }
  };

  function onSelectConversation(page: ConversationPage) {
    setWorkspacePickerOpen(false);
    if (conversationRunRef.current) {
      const currentRun = conversationRunRef.current;
      const currentRunKey = conversationRunCacheKey(currentRun);
      const currentFollowState = conversationSessionFollowRef.current.runKey === currentRunKey
        ? conversationSessionFollowRef.current
        : null;
      conversationRunCache.store(currentRun, {
        followMode: currentFollowState?.mode ?? 'auto',
        selectedSessionKey: currentFollowState?.selectedSessionKey
          ?? currentRun.sessionTree.selectedSessionKey
          ?? null,
      });
    }
    if (page.kind === 'conversation-run') {
      const targetRunKey = conversationRunCacheKey(page);
      const cachedEntry = conversationRunCache.restoreEntry(page);
      const cached = cachedEntry?.run ?? null;
      const cachedLinkedLeaf = cached && page.roundId
        ? findConversationLeafForPage(cached.sessionTree, page)
        : null;
      const cachedMatchesLinkedTarget = !page.roundId || Boolean(cachedLinkedLeaf);
      if (cached && cachedEntry && cachedMatchesLinkedTarget) {
        const explicitSelectedSessionKey = cachedLinkedLeaf
          ? conversationSessionKeyFromParts(cachedLinkedLeaf)
          : null;
        const rememberedSelectedSessionKey = cachedEntry.viewState.selectedSessionKey
          && findConversationLeafByKey(cached.sessionTree, cachedEntry.viewState.selectedSessionKey)
          ? cachedEntry.viewState.selectedSessionKey
          : null;
        const selectedSessionKey = explicitSelectedSessionKey
          ?? rememberedSelectedSessionKey
          ?? cached.sessionTree.selectedSessionKey
          ?? null;
        const followMode: ConversationSessionFollowMode = explicitSelectedSessionKey
          ? 'manual'
          : cachedEntry.viewState.followMode;
        const cachedForPage = selectedSessionKey
          ? beginConversationSessionSelection(cached, selectedSessionKey)
          : cached;
        conversationRunRef.current = cachedForPage;
        conversationSelectedSessionKeyRef.current = selectedSessionKey;
        conversationSessionFollowRef.current = {
          runKey: targetRunKey,
          mode: followMode,
          selectedSessionKey,
          version: conversationSessionFollowRef.current.version + 1,
        };
        conversationRunCache.store(cachedForPage, { followMode, selectedSessionKey });
        setConversationRun(cachedForPage);
      } else {
        conversationSelectedSessionKeyRef.current = null;
        conversationSessionFollowRef.current = {
          runKey: targetRunKey,
          mode: page.roundId ? 'manual' : 'auto',
          selectedSessionKey: null,
          version: conversationSessionFollowRef.current.version + 1,
        };
      }
    }
    setConversationPage(page);
    if (page.kind === 'agents') {
      setPrimaryModule('agent-management');
    } else if (page.kind === 'contexts') {
      setPrimaryModule('knowledge-base');
    } else {
      setPrimaryModule('task-orchestration');
    }
    pushRoute(primaryModule, taskPage, page);
  }

  const content = uiMode === 'conversation'
    ? renderConversationContent()
    : shouldRenderWorkspacePicker(uiMode, workspacePickerOpen)
    ? (
      <WorkspaceSelectPage
        bootstrap={bootstrap}
        appInfo={appInfo}
        busy={busy}
        onChooseWorkspace={onChooseWorkspace}
        onSelectRecentWorkspace={onSelectRecentWorkspace}
        onRemoveRecentWorkspace={onRemoveRecentWorkspace}
      />
    )
    : primaryModule === 'settings'
      ? (
        <SettingsPage
          key={forceSettingsTab ? 'settings-advanced' : 'settings-default'}
          initialTab={forceSettingsTab ?? undefined}
          preferences={preferences}
          appInfo={appInfo}
          updaterSettings={updaterSettings}
          metricsSettings={metricsSettings}
          updateStatus={updateStatus}
          availableUpdate={effectiveAvailableUpdate}
          showAdvancedUpdateDot={showSettingsAdvancedUpdateDot}
          showUpdatesSectionDot={showUpdatesSectionDot}
          downloadProgress={downloadProgress}
          clientVersion={bootstrap?.clientVersion ?? ''}
          busy={busy}
          onSave={onSavePreferences}
          onSaveAvatar={onSaveAvatar}
          onSelectRecentAvatar={onSelectRecentAvatar}
          onSaveAvatarShape={onSaveAvatarShape}
          onClearAvatar={onClearAvatar}
          onImportWallpaper={onImportWallpaper}
          onSelectRecentWallpaper={onSelectRecentWallpaper}
          onSaveWallpaperOpacity={onSaveWallpaperOpacity}
          onRestoreThemeWallpaper={onRestoreThemeWallpaper}
          onSaveUpdaterSettings={onSaveUpdaterSettings}
          onCheckUpdate={onCheckUpdate}
          onInstallUpdate={onInstallUpdate}
          onViewSettings={onMarkSettingsUpdateSeen}
          onViewAdvanced={onMarkSettingsAdvancedUpdateSeen}
        />
      )
      : primaryModule === 'agent-management'
        ? <AgentManagementPage vm={agentRegistry} loading={loading !== null} onRefresh={() => void refresh('manual')} onRegistryChange={setAgentRegistry} />
        : primaryModule === 'knowledge-base'
          ? <ContextManagementPage agentRegistry={agentRegistry} onAgentRegistryChange={setAgentRegistry} />
          : renderTaskContent();

  return (
    <AvatarPreferencesProvider preferences={preferences.avatars}>
    <ConversationComposerDraftBoundary ref={composerDraftRef}>
    <Shell
      uiMode={uiMode}
      active={primaryModule}
      conversationPage={presentedConversationPage}
      conversationSidebar={conversationSidebar}
      activeWorkspaceId={conversationWorkspaceContextId}
      defaultExpandedWorkspaceId={defaultExpandedWorkspaceId}
      workspaceRevealRequest={workspaceRevealRequest}
      conversationTaskUuid={
        presentedConversationPage.kind === 'conversation-run'
        && conversationRun?.projectId === presentedConversationPage.projectId
        && conversationRun.taskId === presentedConversationPage.taskId
        && conversationRun.runId === presentedConversationPage.runId
          ? conversationRun.taskUuid
          : null
      }
      conversationWorkspaceStore={conversationWorkspaceStore}
      appName={appInfo.appName}
      feedbackEnabled={appInfo.feedbackEnabled}
      platform={bootstrap?.platform}
      windowFrameStyle={bootstrap?.windowChrome.frameStyle}
      appConfig={appConfig}
      repoRoot={bootstrap?.repoRoot}
      needsWorkspace={bootstrap?.needsWorkspace}
      showSettingsUpdateDot={showSettingsUpdateDot}
      sidebarCollapsed={sidebarCollapsed}
      onSelect={(module) => {
        const nextTaskPage = module === 'task-orchestration' ? taskListPage : taskPage;
        setWorkspacePickerOpen(false);
        setPrimaryModule(module);
        setTaskPage(nextTaskPage);
        pushRoute(module, nextTaskPage);
      }}
      onSelectConversation={onSelectConversation}
      onToggleSidebar={() => setSidebarCollapsed((value) => !value)}
      onOpenPersonalAnalytics={() => {
        const page: ConversationPage = { kind: 'personal-analytics' };
        setWorkspacePickerOpen(false);
        setUiMode('conversation');
        setPrimaryModule('task-orchestration');
        setConversationPage(page);
        pushRoute('task-orchestration', taskListPage, page);
      }}
      onChooseWorkspace={() => setWorkspacePickerOpen(true)}
      onConversationNew={() => {
        const targetPid = resolveConversationHomeWorkspaceId(
          conversationPage,
          draftConversationWorkspaceId,
          effectiveWorkspaceId,
        );
        if (targetPid) setDraftConversationWorkspaceId(targetPid);
        onSelectConversation({ kind: 'conversation-home' });
      }}
      onConversationSearch={() => setConversationSearchOpen(true)}
      onConversationSelectTask={(projectId, taskId) => {
        const tasks = conversationSidebar.tasksByWorkspace[projectId] ?? [];
        const task = tasks.find((t) => t.taskId === taskId);
        const runId = task?.latestRun?.runId;
        if (runId) {
          onSelectConversation({ kind: 'conversation-run', projectId, taskId, runId });
        }
      }}
      onConversationSelectRun={(projectId, taskId, runId) => {
        onSelectConversation({ kind: 'conversation-run', projectId, taskId, runId });
      }}
      onConversationPauseRun={onConversationPauseRun}
      onConversationRenameTask={(projectId, taskId, title) => {
        setError(null);
        updateTaskMetadata(projectId, taskId, title)
          .then(applyConversationTask)
          .catch((err) => setError(displayAppError(t, err)));
      }}
      onConversationDeleteTask={(projectId, taskId) => {
        deleteConversationTask(projectId, taskId)
          .then((sidebar) => {
            conversationWorkspaceStore.deleteConversation(projectId, taskId);
            applyConversationSidebar(sidebar);
            if (conversationPage.kind === 'conversation-run' && conversationPage.projectId === projectId && conversationPage.taskId === taskId) {
              setConversationRun(null);
              setConversationPage({ kind: 'conversation-home' });
            }
          })
          .catch((err) => setError(displayAppError(t, err)));
      }}
      onConversationPinTask={(projectId, taskId) => {
        pinConversation(projectId, taskId).then((sidebar) => applyConversationSidebar(sidebar)).catch(() => {});
      }}
      onConversationUnpinTask={(projectId, taskId) => {
        unpinConversation(projectId, taskId).then((sidebar) => applyConversationSidebar(sidebar)).catch(() => {});
      }}
      onConversationNewInWorkspace={(projectId) => {
        setDraftConversationWorkspaceId(projectId);
        onSelectConversation({ kind: 'conversation-home' });
      }}
      onConversationAddWorkspace={() => {
        addConversationWorkspace().then((sidebar) => applyConversationSidebar(sidebar)).catch(() => {});
      }}
      onConversationRemoveWorkspace={(projectId) => {
        setError(null);
        return removeConversationWorkspace(projectId)
          .then((sidebar) => {
            conversationWorkspaceStore.deleteProject(projectId);
            const transition = resolveConversationWorkspaceRemovalTransition({
              removedProjectId: projectId,
              lastActiveWorkspaceId: sidebar.lastActiveWorkspaceId,
              activeWorkspaceId: activeWorkspaceIdRef.current,
              draftWorkspaceId: draftConversationWorkspaceId,
              page: conversationPage,
            });
            activeWorkspaceIdRef.current = transition.activeWorkspaceId;
            setActiveWorkspaceId(transition.activeWorkspaceId);
            setDraftConversationWorkspaceId(transition.draftWorkspaceId);
            if (transition.navigateHome) {
              conversationRunRef.current = null;
              setConversationRun(null);
              setConversationPage({ kind: 'conversation-home' });
            }
            applyConversationSidebar(sidebar, transition.activeWorkspaceId);
          })
          .catch((err) => {
            setError(displayAppError(t, err));
            throw err;
          });
      }}
    >
      <WindowCloseCoordinator platform={bootstrap?.platform} />
      <AlertDialog open={Boolean(error)} onOpenChange={(open) => { if (!open) setError(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('common.operationFailed')}</AlertDialogTitle>
            <AlertDialogDescription className="whitespace-pre-wrap break-words">
              {error}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogAction onClick={() => setError(null)}>{t('common.close')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      {shouldShowUpdateAnnouncement ? (
        <div className="pointer-events-none fixed left-1/2 top-13 z-10 -translate-x-1/2">
          <Alert className="pointer-events-auto w-auto min-w-[300px] max-w-[520px] border-border/60 bg-background/95 px-4 py-3 text-foreground shadow-lg backdrop-blur">
            <AlertDescription className="flex items-center justify-between gap-4 text-sm">
              <button type="button" className="inline-flex min-w-0 items-center gap-2 font-medium text-foreground hover:text-primary" onClick={onOpenUpdateAnnouncement}>
                <span className="size-2 rounded-full bg-destructive" aria-hidden="true" />
                <span className="truncate">{t('settings.updater.announcement.title', { version: availableUpdateVersion })}</span>
              </button>
              <Button size="icon" variant="ghost" className="-mr-3 h-7 w-7 shrink-0 text-muted-foreground" onClick={onDismissUpdateAnnouncement} aria-label={t('settings.updater.announcement.dismiss')}>
                <X className="size-4" />
              </Button>
            </AlertDescription>
          </Alert>
        </div>
      ) : null}
      {content}
      <AlertDialog open={updateAnnouncementOpen} onOpenChange={setUpdateAnnouncementOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.updater.announcement.dialogTitle', { version: availableUpdateVersion ?? '' })}</AlertDialogTitle>
            <div className="space-y-3 text-sm text-muted-foreground">
              <p>{t('settings.updater.announcement.dialogDescription')}</p>
              {effectiveAvailableUpdate?.notes ? (
                <div className="max-h-72 overflow-y-auto rounded-md border border-border/50 bg-muted/20 p-3 text-left">
                  <Markdown>{effectiveAvailableUpdate.notes}</Markdown>
                </div>
              ) : null}
            </div>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={onGoToSettingsUpdate}>{t('settings.updater.announcement.goToSettings')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <ConversationSearchDialog
        open={conversationSearchOpen}
        onOpenChange={setConversationSearchOpen}
        onSelectResult={(result) => {
          const page = conversationPageForSearchResult(result);
          if (page) onSelectConversation(page);
        }}
      />
    </Shell>
    </ConversationComposerDraftBoundary>
    </AvatarPreferencesProvider>
  );

  function renderConversationContent() {
    const conversationPage = presentedConversationPage;
    if (conversationPage.kind === 'personal-analytics') {
      return (
        <PersonalAnalyticsPage
          agentRegistry={agentRegistry}
          onOpenAgentManagement={() => onSelectConversation({ kind: 'agents' })}
          onOpenTask={(task) => {
            onSelectConversation({
              kind: 'conversation-run',
              projectId: task.projectId,
              taskId: task.taskId,
              runId: task.latestRunId,
            });
          }}
        />
      );
    }
    if (conversationPage.kind === 'agents') {
      return <AgentManagementPage vm={agentRegistry} loading={loading !== null} onRefresh={() => void refresh('manual')} onRegistryChange={setAgentRegistry} />;
    }
    if (conversationPage.kind === 'contexts') {
      return <ContextManagementPage agentRegistry={agentRegistry} onAgentRegistryChange={setAgentRegistry} />;
    }
    if (conversationPage.kind === 'settings') {
      return (
        <TooltipProvider>
          <SettingsPage
            key={forceSettingsTab ? 'settings-advanced' : 'settings-default'}
            initialTab={forceSettingsTab ?? undefined}
            preferences={preferences}
            appInfo={appInfo}
            updaterSettings={updaterSettings}
            metricsSettings={metricsSettings}
            updateStatus={updateStatus}
            availableUpdate={effectiveAvailableUpdate}
            showAdvancedUpdateDot={showSettingsAdvancedUpdateDot}
            showUpdatesSectionDot={showUpdatesSectionDot}
            downloadProgress={downloadProgress}
            clientVersion={bootstrap?.clientVersion ?? ''}
            busy={busy}
            onSave={onSavePreferences}
            onSaveAvatar={onSaveAvatar}
            onSelectRecentAvatar={onSelectRecentAvatar}
            onSaveAvatarShape={onSaveAvatarShape}
            onClearAvatar={onClearAvatar}
            onImportWallpaper={onImportWallpaper}
            onSelectRecentWallpaper={onSelectRecentWallpaper}
            onSaveWallpaperOpacity={onSaveWallpaperOpacity}
            onRestoreThemeWallpaper={onRestoreThemeWallpaper}
            onSaveUpdaterSettings={onSaveUpdaterSettings}
            onCheckUpdate={onCheckUpdate}
            onInstallUpdate={onInstallUpdate}
            onViewSettings={onMarkSettingsUpdateSeen}
            onViewAdvanced={onMarkSettingsAdvancedUpdateSeen}
          />
        </TooltipProvider>
      );
    }
    if (conversationPage.kind === 'conversation-home' || conversationPage.kind === 'scheduled-task-create') {
      return (
        <>
        <ConversationHomePage
          projectId={defaultProjectId}
          workspaceName={defaultWorkspaceName}
          workspaces={conversationSidebar.workspaces}
          runMode={conversationRunMode}
          agentRegistry={agentRegistry}
          workflowTemplates={conversationWorkflowTemplates}
          profiles={profiles}
          busy={busy}
          inlineContentMaxBytes={appConfig.conversationInlineContentMaxBytes}
          initialScheduledMode={conversationPage.kind === 'scheduled-task-create'}
          workLocation={conversationWorkLocation}
          onRunModeChange={updateConversationRunMode}
          onLoadProfiles={loadProfiles}
          onSubmit={async (input) => {
            const nextMode: ConversationRunModeVm = input.runMode === 'direct'
              ? {
                mode: 'direct',
                directConfig: input.directConfig ?? conversationRunMode.directConfig,
                directPreferences: input.directConfig
                  ? {
                    ...conversationRunMode.directPreferences,
                    [input.directConfig.agentType]: input.directConfig,
                  }
                  : conversationRunMode.directPreferences,
              }
              : input.runMode === 'auto'
                ? { mode: 'auto', autoConfig: input.autoConfig ?? conversationRunMode.autoConfig }
                : {
                  mode: 'workflow',
                  workflowTemplateId: input.workflowTemplateId ?? conversationRunMode.workflowTemplateId,
                  optionalEntryPreferences: conversationRunMode.optionalEntryPreferences,
                };
            setBusy(true);
            void updateConversationRunMode(nextMode, input.projectId);
            try {
              const validation = await validateConversationCreate(input);
              if (!validation.valid) {
                setWorkflowRepairTarget(workflowRepairTargetFromMissingItems(validation.missingItems));
                return validation.missingItems.map((m) => t(`conversation.validation.${m.code}`, { defaultValue: m.label || m.code })).join('\n');
              }
              setWorkflowRepairTarget(null);
              const { task, run } = await createConversationRun(input);
              applyConversationTask(task);
              conversationWorkspaceStore.promoteDraft(
                createDraftConversationWorkspaceScope(input.projectId),
                createConversationWorkspaceScope({
                  projectId: run.projectId,
                  taskId: run.taskId,
                  taskUuid: run.taskUuid,
                  runId: run.runId,
                }),
              );
              rememberConversationWorkspace(run.projectId);
              updateConversationSessionFollow('auto', run.sessionTree.selectedSessionKey ?? null, run);
              applyConversationRunSnapshot(run, 'create');
              resetConversationComposerDraft(composerDraftRef.current);
              setConversationPage({
                kind: 'conversation-run',
                projectId: run.projectId,
                taskId: run.taskId,
                runId: run.runId,
              });
              pushRoute('task-orchestration', taskListPage, {
                kind: 'conversation-run',
                projectId: run.projectId,
                taskId: run.taskId,
                runId: run.runId,
              });
              return null;
            } catch (err) {
              const gitStatus = gitRequirementStatus(err);
              if (gitStatus) {
                setGitRequirement({
                  status: gitStatus,
                  runKind: input.workLocation === 'worktree'
                    ? 'worktree'
                    : input.runMode === 'auto' ? 'auto' : 'workflow',
                  projectId: input.projectId,
                });
                return displayAppError(t, err);
              }
              return displayAppError(t, err);
            } finally {
              setBusy(false);
            }
          }}
          onCreateScheduledTask={async (input) => {
            await createScheduledTask(input);
          }}
          onOpenAgentManagement={() => onSelectConversation({ kind: 'agents' })}
          onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
          onWorkflowRepairTargetChange={setWorkflowRepairTarget}
          onScheduledModeExit={conversationPage.kind === 'scheduled-task-create'
            ? () => onSelectConversation({ kind: 'conversation-home' })
            : undefined}
          onWorkspaceChange={(projectId) => {
            resetConversationComposerDraft(composerDraftRef.current);
            setDraftConversationWorkspaceId(projectId);
            void loadConversationRunMode(projectId);
          }}
          onWorkLocationChange={selectConversationWorkLocation}
        />
        {gitRequirement ? (
          <GitRequirementDialog
            key={`${gitRequirement.projectId ?? 'default'}:${gitRequirement.runKind}:${gitRequirement.status}`}
            open
            projectId={gitRequirement.projectId}
            runKind={gitRequirement.runKind}
            initialStatus={gitRequirement.status}
            onReady={async () => {
              const requirement = gitRequirement;
              setGitRequirement(null);
              if (requirement.runKind === 'worktree' && requirement.projectId) {
                try {
                  await persistConversationWorkLocation('worktree', requirement.projectId);
                } catch (err) {
                  setError(displayAppError(t, err));
                }
              }
            }}
            onUseOtherWorkflow={() => {
              const requirement = gitRequirement;
              setGitRequirement(null);
              if (requirement.runKind === 'worktree' && requirement.projectId) {
                void persistConversationWorkLocation('main', requirement.projectId)
                  .catch((err) => setError(displayAppError(t, err)));
              } else {
                setConversationPage({ kind: 'run-mode-management' });
              }
            }}
            onOpenChange={(open) => { if (!open) setGitRequirement(null); }}
          />
        ) : null}
        </>
      );
    }
    if (conversationPage.kind === 'scheduled-tasks') {
      return <ScheduledTaskManagementPage projectId={defaultProjectId} onCreate={() => onSelectConversation({ kind: 'scheduled-task-create' })} onOpenDetail={(task) => onSelectConversation({ kind: 'scheduled-task-detail', projectId: task.projectId, scheduledTaskId: task.id })} />;
    }
    if (conversationPage.kind === 'scheduled-task-detail') {
      return <ScheduledTaskDetailPage projectId={conversationPage.projectId} scheduledTaskId={conversationPage.scheduledTaskId} onBack={() => onSelectConversation({ kind: 'scheduled-tasks' })} onOpenOccurrence={onSelectConversation} />;
    }
    if (conversationPage.kind === 'run-mode-management') {
      return (
        <RunModeManagementPage
          projectId={defaultProjectId}
          workspaceName={defaultWorkspaceName}
          workspaces={conversationSidebar.workspaces}
          runMode={conversationRunMode}
          agentRegistry={agentRegistry}
          workflowTemplates={conversationWorkflowTemplates}
          repairTarget={workflowRepairTarget}
          onProjectChange={(projectId) => {
            setDraftConversationWorkspaceId(projectId);
            void loadConversationRunMode(projectId);
          }}
          onSave={(mode) => updateConversationRunMode(mode, defaultProjectId)}
          onWorkflowTemplatesChange={setConversationWorkflowTemplates}
        />
      );
    }
    if (conversationPage.kind === 'conversation-run') {
      if (isConversationRunNavigationLoading(conversationPage, conversationRun) || !conversationRun) {
        return (
          <BrandLoadingState label={t('conversation.runtime.loadingSession')} />
        );
      }
      const taskTitle = findConversationTask(
        conversationSidebar,
        conversationPage.projectId,
        conversationPage.taskId,
      )?.title ?? conversationPage.taskId;
      return (
        <ConversationRunPage
          run={conversationRun}
          taskTitle={taskTitle}
          appConfig={appConfig}
          agentRegistry={agentRegistry}
          followMode={conversationSessionFollowRef.current.mode}
          initialSessionTreeExpansion={conversationRunCache.peekViewState(conversationRun)?.sessionTreeExpansion ?? {}}
          onSessionTreeExpansionChange={(sessionTreeExpansion) => {
            conversationRunCache.updateViewState(conversationRun, { sessionTreeExpansion });
          }}
          onRerun={() => {
            if (!conversationRun) return;
            rerunConversationTask(conversationRun.projectId, conversationRun.taskId)
              .then((run) => {
                rememberConversationWorkspace(run.projectId);
                updateConversationSessionFollow('auto', run.sessionTree.selectedSessionKey ?? null, run);
                applyConversationRunSnapshot(run, 'rerun');
                setConversationPage({
                  kind: 'conversation-run',
                  projectId: run.projectId,
                  taskId: run.taskId,
                  runId: run.runId,
                });
                getConversationSidebar().then((sidebar) => applyConversationSidebar(sidebar)).catch(() => {});
                pushRoute('task-orchestration', taskListPage, {
                  kind: 'conversation-run',
                  projectId: run.projectId,
                  taskId: run.taskId,
                  runId: run.runId,
                });
              })
              .catch((err) => setError(displayAppError(t, err)));
          }}
          onEditWorkflow={() => {}}
          onSaveWorkflow={async (json, modelBindings) => {
            const dsl = JSON.parse(json) as Parameters<typeof saveTaskWorkflow>[2];
            const saved = await saveTaskWorkflow(conversationPage.projectId, conversationPage.taskId, dsl, modelBindings);
            const refreshed = await getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId);
            applyConversationRunSnapshot(refreshed, 'workflow-save', {
              selectedSessionKey: conversationSelectedSessionKeyRef.current,
              preserveSelectedSession: conversationSessionFollowRef.current.mode === 'manual',
            });
            return saved;
          }}
          onSelectSession={(leaf, followActive) => {
            const key = leaf.outerNodeId
              ? `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`
              : `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
            const followMode: ConversationSessionFollowMode = followActive ? 'auto' : 'manual';
            conversationSelectedSessionKeyRef.current = key;
            updateConversationSessionFollow(followMode, key);
            pushRoute(
              'task-orchestration',
              taskListPage,
              conversationPageForSession(conversationPage, leaf),
            );
            setConversationRun((current) => {
              if (!current || !conversationPageMatchesRun(conversationPage, current)) return current;
              const next = beginConversationSessionSelection(current, key);
              conversationRunRef.current = next;
              return next;
            });
            switchConversationSession(
              conversationPage.projectId,
              conversationPage.taskId,
              conversationPage.runId,
              leaf.roundId,
              leaf.nodeId,
              leaf.attemptId,
              leaf.outerNodeId,
              leaf.outerAttemptId,
            ).then((switched) => {
              if (conversationSelectedSessionKeyRef.current !== key) {
                return;
              }
              startTransition(() => {
                setConversationRun((prev) => {
                  if (!prev || conversationSelectedSessionKeyRef.current !== key) return prev;
                  const next = {
                    ...prev,
                    selectedSession: switched.selectedSession,
                    sessionTree: { ...prev.sessionTree, selectedSessionKey: key },
                  };
                  conversationRunRef.current = next;
                  conversationSelectedSessionKeyRef.current = key;
                  return next;
                });
              });
              if (conversationRunRef.current && conversationSelectedSessionKeyRef.current === key) {
                conversationRunRef.current = {
                  ...conversationRunRef.current,
                  selectedSession: switched.selectedSession,
                  sessionTree: {
                    ...conversationRunRef.current.sessionTree,
                    selectedSessionKey: key,
                  },
                };
              }
            }).catch(() => {});
          }}
          onLifecycleSnapshot={(snapshot) => {
            applyConversationLifecycleSnapshotToSidebar(
              conversationPage.projectId,
              snapshot.taskId,
              snapshot.runId,
              snapshot.lifecycle,
            );
            setConversationRun((current) => {
              const selectedPatched = applyConversationSelectedSessionSnapshot(current, snapshot);
              const patched = selectedPatched === current
                ? applyConversationBackgroundSessionRuntimeSnapshot(current, snapshot)
                : selectedPatched;
              conversationRunRef.current = patched;
              return patched;
            });
          }}
          onAutoFollowChange={handleConversationAutoFollowChange}
          onTitleChange={(title) => {
            setError(null);
            updateTaskMetadata(conversationPage.projectId, conversationPage.taskId, title)
              .then(applyConversationTask)
              .catch((err) => setError(displayAppError(t, err)));
          }}
        />
      );
    }
    return (
      <ConversationHomePage
        projectId={defaultProjectId}
        workspaceName={defaultWorkspaceName}
        workspaces={conversationSidebar.workspaces}
        runMode={conversationRunMode}
        agentRegistry={agentRegistry}
        workflowTemplates={conversationWorkflowTemplates}
        profiles={profiles}
        busy={busy}
        inlineContentMaxBytes={appConfig.conversationInlineContentMaxBytes}
        workLocation={conversationWorkLocation}
        onRunModeChange={updateConversationRunMode}
        onLoadProfiles={loadProfiles}
        onSubmit={(_input) => null}
        onOpenAgentManagement={() => onSelectConversation({ kind: 'agents' })}
        onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
        onWorkspaceChange={(projectId) => {
          resetConversationComposerDraft(composerDraftRef.current);
          setDraftConversationWorkspaceId(projectId);
          void loadConversationRunMode(projectId);
        }}
        onWorkLocationChange={selectConversationWorkLocation}
      />
    );
  }

  function renderTaskContent() {
    const pageBreadcrumbs = <Breadcrumbs page={taskPage} onNavigate={navigate} />;
    if (taskPage.kind === 'task-list') {
      return (
        <TaskListPage
          vm={taskList}
          loading={loading}
          breadcrumbs={pageBreadcrumbs}
          onNavigate={navigate}
          onRefresh={() => void refresh('manual')}
          onCreateTask={onCreateTask}
          onOpenProfileManagement={openProfileManagement}
          createTaskDraft={createTaskDraft}
          onCreateTaskDraftChange={setCreateTaskDraft}
        />
      );
    }
    if (taskPage.kind === 'workflow') {
      return (
        <>
        <WorkflowPage
          vm={workflow}
          busy={busy}
          refreshing={loading === 'manual'}
          breadcrumbs={pageBreadcrumbs}
          onNavigate={navigate}
          onRefresh={() => void refresh('manual')}
          onStartRun={(taskId) => runAction(() => startRun(taskId))}
          onContinueRun={(taskId, runId) => void runAction(() => continueRun(undefined, taskId, runId))}
          onStopRun={onStopRun}
          onSaveWorkflow={onSaveTaskWorkflow}
          onOpenProfileManagement={openProfileManagement}
        />
        {gitRequirement ? (
          <GitRequirementDialog
            key={`${gitRequirement.projectId ?? 'default'}:${gitRequirement.status}`}
            open
            projectId={gitRequirement.projectId}
            runKind={gitRequirement.runKind}
            initialStatus={gitRequirement.status}
            onReady={() => setGitRequirement(null)}
            onUseOtherWorkflow={() => {
              setGitRequirement(null);
              navigate({ kind: 'task-list' });
            }}
            onOpenChange={(open) => { if (!open) setGitRequirement(null); }}
          />
        ) : null}
        </>
      );
    }
    return <RoundDetailPage vm={roundDetail} breadcrumbs={pageBreadcrumbs} selection={roundSelection} refreshing={loading === 'manual'} busy={busy} appConfig={appConfig} workspaceProjectId={bootstrap?.repoRoot ? bootstrap.repoRoot.toLowerCase().replace(/[^a-z0-9\-_]/g, '-') : undefined} onRefresh={() => void refresh('manual')} onSelect={setRoundSelection} />;
  }
}
