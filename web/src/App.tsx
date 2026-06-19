import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useMemo, useRef, useState, useTransition } from 'react';
import {
  checkUpdateManual,
  chooseWorkspace,
  continueRun,
  createConversationRun,
  createTask,
  deleteConversationTask,
  dismissUpdateAnnouncement,
  downloadAndInstallUpdate,
  getAgentRegistry,
  getConversationRun,
  getConversationRunMode,
  getConversationSidebar,
  getProfiles,
  getWorkflowTemplates,
  switchConversationSession,
  markSettingsAdvancedUpdateSeen,
  markSettingsUpdateSeen,
  getAppBootstrap,
  getRoundDetail,
  getTaskList,
  getWorkflow,
  killRun,
  pinConversation,
  rerunConversationTask,
  saveDesktopPreferences,
  saveUpdaterSettings,
  saveTaskWorkflow,
  selectRecentWorkspace,
  startRun,
  unpinConversation,
  updateTaskMetadata,
  validateConversationCreate,
  addConversationWorkspace,
  removeConversationWorkspace,
  syncConversationWorkspace,
  saveDesktopUiMode,
  saveConversationRunMode,
  saveLastConversationWorkspace,
  subscribeAcpSessionUpdates,
} from './api';
import { isTauriRuntime } from './api/shared';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Breadcrumbs } from './components/Breadcrumbs';
import { Button } from '@/components/ui/button';
import { X } from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Markdown } from '@/components/prompt-kit/markdown';
import { Shell } from './components/Shell';
import i18n, { displayAppError, i18nLanguage } from './i18n';
import {
  planConversationAcpRunUpdate,
  resolveConversationEventSelectedSessionKey,
  resolveConversationRefreshSelectedSessionKey,
  type ConversationSessionFollowMode,
  type ConversationSessionFollowState,
} from '@/lib/conversation-session-follow';
import {
  applyConversationBackgroundSessionRuntimeSnapshot,
  applyConversationSelectedSessionSnapshot,
  conversationSessionKeyFromParts,
  mergeConversationRunSnapshot,
  type ConversationRunSnapshotSource,
} from '@/lib/conversation-run-snapshot';
import { useTranslation } from 'react-i18next';
import { AgentManagementPage } from './pages/AgentManagementPage';
import { ContextManagementPage } from './pages/ContextManagementPage';
import { ConversationHomePage } from './pages/ConversationHomePage';
import { ConversationRunPage } from './pages/ConversationRunPage';
import { ConversationSearchDialog } from './components/conversation/ConversationSearchDialog';
import { prioritizeConversationSidebarWorkspace } from './components/conversation/ConversationSidebar';
import { RunModeManagementPage } from './pages/RunModeManagementPage';
import { RoundDetailPage } from './pages/RoundDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { createInitialCreateTaskDraft, TaskListPage, type CreateTaskDraftState } from './pages/TaskListPage';
import { WorkflowPage } from './pages/WorkflowPage';
import { WorkspaceSelectPage } from './pages/WorkspaceSelectPage';
import { pushRoute, replaceRoute, routeFromPath, taskListPage, conversationHomePage } from './routes';
import { applyFont, applyTheme } from './theme';
import { useInterventionNotifications } from './lib/use-intervention-notifications';
import type {
  AgentRegistryVm,
  AppBootstrapVm,
  AppConfigVm,
  AppInfoVm,
  ConversationPage,
  ConversationRunModeVm,
  ConversationRunVm,
  ConversationSessionLeafVm,
  ConversationSessionTreeVm,
  ConversationTreeNodeVm,
  WorkflowTemplateStore,
  ConversationSidebarVm,
  CreateTaskInput,
  ProfileVm,
  DesktopFontPreference,
  DesktopLanguage,
  DesktopThemePreference,
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
  WorkflowVm,
  InterventionNavigateEventVm,
} from './types';

const defaultPreferences: PreferencesVm = { theme: 'system', language: 'zh-cn', font: 'app-default', useLocalClaude: false, verboseLogging: false };
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
  appName: 'Gold Band',
  appKey: 'gold-band',
  configDirName: '.gold-band',
};
const defaultAppConfig: AppConfigVm = {
  acpSessionTitleRefreshEnabled: false,
  acpChatEventPageSize: 360,
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

function workspacePathForProject(sidebar: ConversationSidebarVm, projectId?: string | null) {
  if (!projectId) return undefined;
  return sidebar.workspaces.find((workspace) => workspace.projectId === projectId)?.workspacePath;
}

export function App() {
  const initialRoute = routeFromPath(window.location.pathname);
  const savedUiMode = (typeof localStorage !== 'undefined' && localStorage.getItem('gold-band-ui-mode')) as DesktopUiMode | null;
  const [uiMode, setUiMode] = useState<DesktopUiMode>(savedUiMode ?? initialRoute.uiMode);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => typeof localStorage !== 'undefined' && localStorage.getItem('gold-band-sidebar-collapsed') === 'true');
  const [bootstrap, setBootstrap] = useState<AppBootstrapVm | null>(null);
  const [primaryModule, setPrimaryModule] = useState<PrimaryModule>(initialRoute.module);
  const [taskPage, setTaskPage] = useState<TaskPage>(initialRoute.taskPage);
  const [conversationPage, setConversationPage] = useState<ConversationPage>(initialRoute.conversationPage);
  const [conversationSidebar, setConversationSidebar] = useState<ConversationSidebarVm>({ workspaces: [], pinnedTasks: [], tasksByWorkspace: {} });
  const conversationSidebarRef = useRef<ConversationSidebarVm>({ workspaces: [], pinnedTasks: [], tasksByWorkspace: {} });
  const [conversationSearchOpen, setConversationSearchOpen] = useState(false);
  const [conversationRunMode, setConversationRunMode] = useState<ConversationRunModeVm>({ mode: 'auto' });
  const [conversationRun, setConversationRun] = useState<ConversationRunVm | null>(null);
  const conversationRunRef = useRef<ConversationRunVm | null>(null);
  const conversationSessionFollowRef = useRef<ConversationSessionFollowState>({
    mode: 'auto',
    selectedSessionKey: null,
    version: 0,
  });
  const conversationSelectedSessionKeyRef = useRef<string | null>(null);
  const [forceSettingsTab, setForceSettingsTab] = useState<'advanced' | null>(null);
  const [conversationWorkflowTemplates, setConversationWorkflowTemplates] = useState<WorkflowTemplateStore | null>(null);
  const [, startTransition] = useTransition();

  const updateConversationSessionFollow = useCallback((mode: ConversationSessionFollowMode, selectedSessionKey?: string | null) => {
    conversationSessionFollowRef.current = {
      mode,
      selectedSessionKey: selectedSessionKey ?? conversationSelectedSessionKeyRef.current ?? null,
      version: conversationSessionFollowRef.current.version + 1,
    };
  }, []);

  const applyConversationRunSnapshot = useCallback((
    snapshot: ConversationRunVm,
    source: ConversationRunSnapshotSource,
    options?: { selectedSessionKey?: string | null; preserveSelectedSession?: boolean },
  ) => {
    setConversationRun((current) => {
      const merged = mergeConversationRunSnapshot(current, snapshot, source, options);
      conversationRunRef.current = merged;
      conversationSelectedSessionKeyRef.current = merged.sessionTree.selectedSessionKey ?? null;
      conversationSessionFollowRef.current = {
        ...conversationSessionFollowRef.current,
        selectedSessionKey: merged.sessionTree.selectedSessionKey ?? null,
      };
      return merged;
    });
  }, []);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(null);
  const activeWorkspaceIdRef = useRef<string | null>(null);
  const [draftConversationWorkspaceId, setDraftConversationWorkspaceId] = useState<string | null>(null);

  const applyConversationSidebar = useCallback((sidebar: ConversationSidebarVm, projectId?: string | null) => {
    const activeProjectId = projectId ?? activeWorkspaceIdRef.current ?? sidebar.lastActiveWorkspaceId ?? null;
    const nextSidebar = prioritizeConversationSidebarWorkspace(sidebar, activeProjectId);
    conversationSidebarRef.current = nextSidebar;
    setConversationSidebar(nextSidebar);
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
  const sidebarFocusWorkspaceId = conversationPage.kind === 'conversation-run'
    ? conversationPage.projectId
    : (draftConversationWorkspaceId ?? effectiveWorkspaceId);
  const defaultProjectId = draftWorkspace?.projectId ?? 'default';
  const defaultWorkspaceName = draftWorkspace?.name ?? 'Default Workspace';
  const [roundSelection, setRoundSelection] = useState<RoundSelection>({ kind: 'round' });
  const [agentRegistry, setAgentRegistry] = useState<AgentRegistryVm | null>(null);
  const [profiles, setProfiles] = useState<ProfileVm[]>([]);
  const [taskList, setTaskList] = useState<TaskListVm | null>(null);
  const [createTaskDraft, setCreateTaskDraft] = useState<CreateTaskDraftState>(() => createInitialCreateTaskDraft());
  const [workflow, setWorkflow] = useState<WorkflowVm | null>(null);
  const [roundDetail, setRoundDetail] = useState<RoundDetailVm | null>(null);
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const [loading, setLoading] = useState<VisibleRefreshMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<{ downloaded: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updateAnnouncementOpen, setUpdateAnnouncementOpen] = useState(false);
  const backgroundRefreshInFlightRef = useRef(false);

  useEffect(() => {
    conversationRunRef.current = conversationRun;
    conversationSelectedSessionKeyRef.current = conversationRun?.sessionTree.selectedSessionKey ?? null;
  }, [conversationRun]);

  useEffect(() => {
    if (conversationPage.kind !== 'conversation-run') return;
    conversationSelectedSessionKeyRef.current = null;
    updateConversationSessionFollow('auto', null);
  }, [conversationPage]);

  const handleConversationAutoFollowChange = useCallback((enabled: boolean) => {
    if (conversationPage.kind !== 'conversation-run') return;
    const mode: ConversationSessionFollowMode = enabled ? 'auto' : 'manual';
    updateConversationSessionFollow(mode, conversationSelectedSessionKeyRef.current);
  }, [conversationPage, updateConversationSessionFollow]);

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
  const shouldShowUpdateAnnouncement = useMemo(
    () => availableUpdateVersion !== null && updateBadges.announcementClosedVersion !== availableUpdateVersion,
    [availableUpdateVersion, updateBadges.announcementClosedVersion],
  );
  const { t } = useTranslation();

  useEffect(() => {
    applyTheme(preferences.theme);
  }, [preferences.theme]);

  useEffect(() => {
    if (preferences.theme !== 'system') return undefined;
    const colorScheme = window.matchMedia('(prefers-color-scheme: dark)');
    const syncSystemTheme = () => applyTheme('system');
    colorScheme.addEventListener('change', syncSystemTheme);
    return () => colorScheme.removeEventListener('change', syncSystemTheme);
  }, [preferences.theme]);

  useEffect(() => {
    applyFont(preferences.font);
  }, [preferences.font]);

  useEffect(() => {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem('gold-band-sidebar-collapsed', sidebarCollapsed ? 'true' : 'false');
  }, [sidebarCollapsed]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    getCurrentWindow().setDecorations(false).catch(() => {});
  }, []);

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
      if (savedUiMode) setUiMode(savedUiMode);
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
        if (bootstrap.needsWorkspace) {
          setWorkspacePickerOpen(true);
        }
      })
      .catch((err) => setError(displayAppError(t, err)));
  }, [t]);

  // Load conversation sidebar data when in conversation mode
  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    getConversationSidebar()
      .then((sidebar) => applyConversationSidebar(sidebar))
      .catch(() => {}); // Silently fail - sidebar will show empty state
  }, [applyConversationSidebar, bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    getAgentRegistry().then(setAgentRegistry).catch(() => {});
    getProfiles().then((result) => setProfiles(result.profiles)).catch(() => setProfiles([]));
    getWorkflowTemplates().then(setConversationWorkflowTemplates).catch(() => {});
  }, [bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || !defaultProjectId) return;
    getConversationRunMode(defaultProjectId)
      .then((mode) => { if (mode) setConversationRunMode(mode); })
      .catch(() => {});
  }, [bootstrap, uiMode, defaultProjectId]);

  // Load conversation run when navigating to a run page
  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || conversationPage.kind !== 'conversation-run') return;
    const { projectId, taskId, runId } = conversationPage;
    getConversationRun(projectId, taskId, runId)
      .then((run) => {
        applyConversationRunSnapshot(run, 'initial-load');
      })
      .catch(() => setConversationRun(null));
  }, [applyConversationRunSnapshot, bootstrap, uiMode, conversationPage]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation' || conversationPage.kind !== 'conversation-run') return undefined;
    let active = true;
    let refreshTimer: number | null = null;
    let pendingEventSessionKey: string | null = null;
    let stopListening: (() => void) | null = null;
    const { projectId, taskId, runId } = conversationPage;

    const refreshConversationRun = () => {
      refreshTimer = null;
      const followStateAtRequest = conversationSessionFollowRef.current;
      const currentSelectedKey = conversationSelectedSessionKeyRef.current
        ?? conversationRunRef.current?.sessionTree.selectedSessionKey
        ?? null;
      const selectedKey = resolveConversationRefreshSelectedSessionKey({
        followMode: followStateAtRequest.mode,
        pendingEventSessionKey,
        currentSelectedKey,
      });
      pendingEventSessionKey = null;
      getConversationRun(projectId, taskId, runId, selectedKey)
        .then((run) => {
          if (!active) return;
          const latestFollowState = conversationSessionFollowRef.current;
          const effectiveSelectedKey = latestFollowState.version === followStateAtRequest.version
            ? selectedKey
            : (latestFollowState.selectedSessionKey ?? conversationSelectedSessionKeyRef.current ?? selectedKey);
          applyConversationRunSnapshot(run, 'live-refresh', {
            selectedSessionKey: effectiveSelectedKey,
            preserveSelectedSession: latestFollowState.mode === 'manual',
          });
        })
        .catch(() => {});
      getConversationSidebar()
        .then((sidebar) => {
          if (active) applyConversationSidebar(sidebar);
        })
        .catch(() => {});
    };

    void subscribeAcpSessionUpdates((event) => {
      if (!active) return;
      if (event.taskId !== taskId || event.runId !== runId) return;
      if (event.projectId && event.projectId !== projectId) return;
      const sessionKey = conversationSessionKeyFromParts(event);
      const currentRun = conversationRunRef.current;
      const currentSelectedKey = conversationSelectedSessionKeyRef.current
        ?? currentRun?.sessionTree.selectedSessionKey
        ?? null;
      const treeHasSession = currentRun
        ? conversationTreeHasSessionKey(currentRun.sessionTree, sessionKey)
        : false;
      const alreadySelected = currentSelectedKey === sessionKey;
      const updatePlan = planConversationAcpRunUpdate({
        treeHasSession,
        alreadySelected,
        hasSessionSnapshot: Boolean(event.session),
        hasLiveEvent: Boolean(event.event),
        sessionStatus: event.session?.status,
        pendingPermissionCount: event.session?.pendingPermissions?.length ?? 0,
      });
      if (event.session && updatePlan.patchSelectedSession) {
        setConversationRun((current) => {
          const patched = applyConversationSelectedSessionSnapshot(current, event);
          conversationRunRef.current = patched;
          return patched;
        });
      }
      if (event.session && updatePlan.patchBackgroundSession) {
        setConversationRun((current) => {
          const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, event);
          conversationRunRef.current = patched;
          return patched;
        });
      }
      if (!updatePlan.queueRunRefresh) {
        return;
      }
      const followState = conversationSessionFollowRef.current;
      pendingEventSessionKey = resolveConversationEventSelectedSessionKey({
        currentSelectedKey,
        incomingSessionKey: sessionKey,
        followMode: followState.mode,
      });
      if (refreshTimer !== null) return;
      refreshTimer = window.setTimeout(refreshConversationRun, 120);
    })
      .then((dispose) => {
        if (active) {
          stopListening = dispose;
        } else {
          dispose();
        }
      })
      .catch(() => {});

    return () => {
      active = false;
      if (refreshTimer !== null) window.clearTimeout(refreshTimer);
      stopListening?.();
    };
  }, [applyConversationRunSnapshot, applyConversationSidebar, bootstrap, uiMode, conversationPage]);

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
    void refresh(hasPageData ? 'background' : 'initial');
  }, [hasPageData, refresh]);

  useEffect(() => {
    if (!bootstrap || !hasPageData) return undefined;
    let intervalId: number;
    const startInterval = (ms: number) => {
      window.clearInterval(intervalId);
      intervalId = window.setInterval(() => void refresh('background'), ms) as unknown as number;
    };
    startInterval(10000);
    const onVisibilityChange = () => {
      startInterval(document.hidden ? 30000 : 10000);
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      window.clearInterval(intervalId);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [bootstrap, hasPageData, refresh]);

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
    const projectId = conversationSidebarRef.current
      ? Object.entries(conversationSidebarRef.current.tasksByWorkspace)
          .find(([, tasks]) => tasks.some((task) => task.taskId === event.taskId))
          ?.[0]
      : undefined;
    const targetProjectId = projectId ?? effectiveWorkspaceId;
    const runPage: ConversationPage = { kind: 'conversation-run', projectId: targetProjectId, taskId: event.taskId, runId: event.runId };
    setPrimaryModule('task-orchestration');
    setConversationPage(runPage);
    pushRoute('task-orchestration', taskPage, runPage);

    let run = conversationRunRef.current
      && conversationRunRef.current.taskId === event.taskId
      && conversationRunRef.current.runId === event.runId
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

    const leaf = findSessionLeaf(run.sessionTree, event.roundId, event.nodeId, event.attemptId);
    if (!leaf) return;
    const key = conversationSessionKeyFromParts({
      roundId: leaf.roundId,
      nodeId: leaf.nodeId,
      attemptId: leaf.attemptId,
      outerNodeId: leaf.outerNodeId,
      outerAttemptId: leaf.outerAttemptId,
    });
    conversationSelectedSessionKeyRef.current = key;
    updateConversationSessionFollow('manual', key);
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
            artifacts: switched.artifacts,
            attachments: switched.attachments,
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
    effectiveWorkspaceId,
    taskPage,
    applyConversationRunSnapshot,
    updateConversationSessionFollow,
  ]);

  useInterventionNotifications(handleInterventionNavigate);

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
      if (options?.surfaceError !== false) {
        setError(displayAppError(t, err));
      }
      if (options?.rethrow) {
        throw err;
      }
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const updateConversationRunMode = (mode: ConversationRunModeVm) => {
    setConversationRunMode(mode);
    saveConversationRunMode(defaultProjectId, mode).catch(() => {});
  };

  const onKillRun = (taskId: string, runId: string) => {
    if (window.confirm(t('common.confirmKill'))) {
      void runAction(() => killRun(taskId, runId));
    }
  };

  const onCreateTask = async (input: CreateTaskInput) => {
    const created = await runAction(() => createTask(input), { surfaceError: false, rethrow: true });
    if (created) setWorkflow(created);
    return created;
  };

  const onSaveTaskWorkflow = async (taskId: string, workflow: WorkflowDsl) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await saveTaskWorkflow(undefined, taskId, workflow);
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

  const onSavePreferences = async (theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean, verboseLogging: boolean) => {
    setBusy(true);
    try {
      const saved = await saveDesktopPreferences(theme, language, font, useLocalClaude, verboseLogging);
      setBootstrap((current) => current ? { ...current, preferences: saved } : {
        repoRoot: '',
        recentWorkspaces: [],
        preferences: saved,
        updaterSettings: defaultUpdaterSettings,
        updateStatus: defaultUpdateStatus,
        updateBadges: defaultUpdateBadges,
        metricsSettings: defaultMetricsSettings,
        clientVersion: '',
        appInfo: defaultAppInfo,
        appConfig: defaultAppConfig,
        needsWorkspace: false,
      });
      setTaskList(null);
      setWorkflow(null);
      setRoundDetail(null);
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusy(false);
    }
  };

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

  const content = uiMode === 'conversation'
    ? renderConversationContent()
    : workspacePickerOpen
    ? (
      <WorkspaceSelectPage
        bootstrap={bootstrap}
        appInfo={appInfo}
        busy={busy}
        onChooseWorkspace={onChooseWorkspace}
        onSelectRecentWorkspace={onSelectRecentWorkspace}
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
          ? <ContextManagementPage />
          : renderTaskContent();

  const persistUiMode = (nextMode: DesktopUiMode) => {
    setUiMode(nextMode);
    if (typeof localStorage !== 'undefined') localStorage.setItem('gold-band-ui-mode', nextMode);
    saveDesktopUiMode(nextMode).catch(() => {});
  };

  const onToggleUiMode = () => {
    const nextMode: DesktopUiMode = uiMode === 'conversation' ? 'workbench' : 'conversation';
    if (nextMode === 'workbench') {
      const targetProjectId = activeWorkspaceIdRef.current ?? effectiveWorkspaceId;
      const targetWorkspace = workspacePathForProject(conversationSidebarRef.current, targetProjectId)
        ?? activeWorkspace?.workspacePath;
      if (targetWorkspace && targetWorkspace !== bootstrap?.repoRoot) {
        setBusy(true);
        setError(null);
        selectRecentWorkspace(targetWorkspace)
          .then((nextBootstrap) => {
            persistUiMode('workbench');
            applyWorkspace(nextBootstrap);
          })
          .catch((err) => setError(displayAppError(t, err)))
          .finally(() => setBusy(false));
        return;
      }
      persistUiMode('workbench');
      pushRoute(primaryModule, taskPage);
      return;
    }

    persistUiMode('conversation');
    if (bootstrap?.repoRoot) {
      syncConversationWorkspace(bootstrap.repoRoot)
        .then((sidebar) => {
          activeWorkspaceIdRef.current = sidebar.lastActiveWorkspaceId ?? null;
          setActiveWorkspaceId(sidebar.lastActiveWorkspaceId ?? null);
          applyConversationSidebar(sidebar, sidebar.lastActiveWorkspaceId);
        })
        .catch(() => {});
    }
    pushRoute(primaryModule, taskPage, conversationPage);
  };

  const onSelectConversation = (page: ConversationPage) => {
    setWorkspacePickerOpen(false);
    setConversationPage(page);
    if (page.kind === 'agents') {
      setPrimaryModule('agent-management');
    } else if (page.kind === 'contexts') {
      setPrimaryModule('knowledge-base');
    } else {
      setPrimaryModule('task-orchestration');
    }
    pushRoute(primaryModule, taskPage, page);
  };

  return (
    <Shell
      uiMode={uiMode}
      active={primaryModule}
      conversationPage={conversationPage}
      conversationSidebar={conversationSidebar}
      activeWorkspaceId={sidebarFocusWorkspaceId}
      appName={appInfo.appName}
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
      onToggleUiMode={onToggleUiMode}
      onToggleSidebar={() => setSidebarCollapsed((value) => !value)}
      onChooseWorkspace={() => setWorkspacePickerOpen(true)}
      onConversationNew={() => {
        const targetPid = conversationPage.kind === 'conversation-run'
          ? conversationPage.projectId
          : effectiveWorkspaceId;
        if (targetPid) setDraftConversationWorkspaceId(targetPid);
        setConversationPage({ kind: 'conversation-home' });
      }}
      onConversationSearch={() => setConversationSearchOpen(true)}
      onConversationSelectTask={(projectId, taskId) => {
        const tasks = conversationSidebar.tasksByWorkspace[projectId] ?? [];
        const task = tasks.find((t) => t.taskId === taskId);
        const runId = task?.latestRun?.runId;
        if (runId) {
          setConversationPage({ kind: 'conversation-run', projectId, taskId, runId });
        }
      }}
      onConversationSelectRun={(projectId, taskId, runId) => {
        setConversationPage({ kind: 'conversation-run', projectId, taskId, runId });
      }}
      onConversationRenameTask={(projectId, taskId, title) => {
        updateTaskMetadata(projectId, taskId, title)
          .then(() => getConversationSidebar())
          .then((sidebar) => applyConversationSidebar(sidebar))
          .catch(() => {});
        if (conversationPage.kind === 'conversation-run' && conversationPage.projectId === projectId && conversationPage.taskId === taskId) {
          setConversationRun((prev) => prev ? { ...prev, title } : prev);
        }
      }}
      onConversationDeleteTask={(projectId, taskId) => {
        deleteConversationTask(projectId, taskId)
          .then((sidebar) => {
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
        setConversationPage({ kind: 'conversation-home' });
      }}
      onConversationAddWorkspace={() => {
        addConversationWorkspace().then((sidebar) => applyConversationSidebar(sidebar)).catch(() => {});
      }}
      onConversationRemoveWorkspace={(projectId) => {
        removeConversationWorkspace(projectId).then((sidebar) => {
          if (activeWorkspaceIdRef.current === projectId) {
            activeWorkspaceIdRef.current = sidebar.lastActiveWorkspaceId ?? null;
            setActiveWorkspaceId(sidebar.lastActiveWorkspaceId ?? null);
          }
          setDraftConversationWorkspaceId((current) => current === projectId ? null : current);
          applyConversationSidebar(sidebar, sidebar.lastActiveWorkspaceId);
        }).catch(() => {});
      }}
    >
      {error ? <Alert variant="destructive" className="mx-8 mt-4"><AlertDescription>{error}</AlertDescription></Alert> : null}
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
          if (result.latestRun) {
            setConversationPage({ kind: 'conversation-run', projectId: result.projectId, taskId: result.taskId, runId: result.latestRun.runId });
          }
        }}
      />
    </Shell>
  );

  function renderConversationContent() {
    if (workspacePickerOpen) {
      return (
        <WorkspaceSelectPage
          bootstrap={bootstrap}
          appInfo={appInfo}
          busy={busy}
          onChooseWorkspace={onChooseWorkspace}
          onSelectRecentWorkspace={onSelectRecentWorkspace}
        />
      );
    }
    if (conversationPage.kind === 'agents') {
      return <AgentManagementPage vm={agentRegistry} loading={loading !== null} onRefresh={() => void refresh('manual')} onRegistryChange={setAgentRegistry} />;
    }
    if (conversationPage.kind === 'contexts') {
      return <ContextManagementPage />;
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
            onSaveUpdaterSettings={onSaveUpdaterSettings}
            onCheckUpdate={onCheckUpdate}
            onInstallUpdate={onInstallUpdate}
            onViewSettings={onMarkSettingsUpdateSeen}
            onViewAdvanced={onMarkSettingsAdvancedUpdateSeen}
          />
        </TooltipProvider>
      );
    }
    if (conversationPage.kind === 'conversation-home') {
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
          onRunModeChange={updateConversationRunMode}
          onSubmit={async (input) => {
            const nextMode: ConversationRunModeVm = input.runMode === 'auto'
              ? { mode: 'auto', autoConfig: input.autoConfig ?? conversationRunMode.autoConfig }
              : { mode: 'workflow', workflowTemplateId: input.workflowTemplateId ?? conversationRunMode.workflowTemplateId };
            setConversationRunMode(nextMode);
            setBusy(true);
            saveConversationRunMode(input.projectId, nextMode).catch(() => {});
            try {
              const validation = await validateConversationCreate(input);
              if (!validation.valid) {
                return validation.missingItems.map((m) => t(`conversation.validation.${m.code}`, { defaultValue: m.label || m.code })).join('\n');
              }
              const run = await createConversationRun(input);
              rememberConversationWorkspace(run.projectId);
              updateConversationSessionFollow('auto', run.sessionTree.selectedSessionKey ?? null);
              applyConversationRunSnapshot(run, 'create');
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
              return null;
            } catch (err) {
              return displayAppError(t, err);
            } finally {
              setBusy(false);
            }
          }}
          onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
          onWorkspaceChange={(projectId) => {
            setDraftConversationWorkspaceId(projectId);
            getConversationRunMode(projectId).then((mode) => { if (mode) setConversationRunMode(mode); }).catch(() => {});
          }}
        />
      );
    }
    if (conversationPage.kind === 'run-mode-management') {
      return (
        <RunModeManagementPage
          runMode={conversationRunMode}
          agentRegistry={agentRegistry}
          workflowTemplates={conversationWorkflowTemplates}
          onSave={updateConversationRunMode}
          onWorkflowTemplatesChange={setConversationWorkflowTemplates}
          onBack={() => setConversationPage({ kind: 'conversation-home' })}
        />
      );
    }
    if (conversationPage.kind === 'conversation-run') {
      if (!conversationRun || conversationRun.runId !== conversationPage.runId) {
        return (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {t('common.loading')}
          </div>
        );
      }
      return (
        <ConversationRunPage
          run={conversationRun}
          appConfig={appConfig}
          agentRegistry={agentRegistry}
          onRerun={() => {
            if (!conversationRun) return;
            rerunConversationTask(conversationRun.projectId, conversationRun.taskId)
              .then((run) => {
                rememberConversationWorkspace(run.projectId);
                updateConversationSessionFollow('auto', run.sessionTree.selectedSessionKey ?? null);
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
          onSaveWorkflow={async (json) => {
            const dsl = JSON.parse(json) as Parameters<typeof saveTaskWorkflow>[2];
            await saveTaskWorkflow(conversationPage.projectId, conversationPage.taskId, dsl);
            const refreshed = await getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId);
            applyConversationRunSnapshot(refreshed, 'workflow-save', {
              selectedSessionKey: conversationSelectedSessionKeyRef.current,
              preserveSelectedSession: conversationSessionFollowRef.current.mode === 'manual',
            });
          }}
          onSelectSession={(leaf, followActive) => {
            const key = leaf.outerNodeId
              ? `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`
              : `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
            const followMode: ConversationSessionFollowMode = followActive ? 'auto' : 'manual';
            conversationSelectedSessionKeyRef.current = key;
            updateConversationSessionFollow(followMode, key);
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
                    artifacts: switched.artifacts,
                    attachments: switched.attachments,
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
                  artifacts: switched.artifacts,
                  attachments: switched.attachments,
                  sessionTree: {
                    ...conversationRunRef.current.sessionTree,
                    selectedSessionKey: key,
                  },
                };
              }
            }).catch(() => {});
          }}
          onSessionStopped={() => {
            const selectedKey = conversationRunRef.current?.sessionTree.selectedSessionKey ?? null;
            getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId, selectedKey)
              .then((refreshed) => {
                applyConversationRunSnapshot(refreshed, 'session-stopped', {
                  selectedSessionKey: selectedKey,
                  preserveSelectedSession: conversationSessionFollowRef.current.mode === 'manual',
                });
                return getConversationSidebar();
              })
              .then((sidebar) => applyConversationSidebar(sidebar))
              .catch((err) => setError(displayAppError(t, err)));
          }}
          onAutoFollowChange={handleConversationAutoFollowChange}
          onContinueRun={async (promptId, prompt) => {
            try {
              await continueRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId, promptId, prompt);
              const selectedKey =
                conversationSelectedSessionKeyRef.current
                ?? conversationRunRef.current?.sessionTree.selectedSessionKey
                ?? null;
              const refreshed = await getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId, selectedKey);
              applyConversationRunSnapshot(refreshed, 'continue', {
                selectedSessionKey: selectedKey,
                preserveSelectedSession: conversationSessionFollowRef.current.mode === 'manual',
              });
            } catch (err) {
              setError(displayAppError(t, err));
              throw err;
            }
          }}
          onTitleChange={(title) => {
            setConversationRun((prev) => prev ? { ...prev, title } : prev);
            updateTaskMetadata(conversationPage.projectId, conversationPage.taskId, title)
              .then(() => getConversationSidebar())
              .then((sidebar) => applyConversationSidebar(sidebar))
              .catch(() => {});
          }}
        />
      );
    }
    return <ConversationHomePage
      projectId={defaultProjectId}
      workspaceName={defaultWorkspaceName}
      workspaces={conversationSidebar.workspaces}
          runMode={conversationRunMode}
          agentRegistry={agentRegistry}
          workflowTemplates={conversationWorkflowTemplates}
          profiles={profiles}
          busy={busy}
      onRunModeChange={updateConversationRunMode}
      onSubmit={(_input) => null}
      onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
      onWorkspaceChange={(projectId) => {
        setDraftConversationWorkspaceId(projectId);
        getConversationRunMode(projectId).then((mode) => { if (mode) setConversationRunMode(mode); }).catch(() => {});
      }}
    />;
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
        <WorkflowPage
          vm={workflow}
          busy={busy}
          refreshing={loading === 'manual'}
          breadcrumbs={pageBreadcrumbs}
          onNavigate={navigate}
          onRefresh={() => void refresh('manual')}
          onStartRun={(taskId) => runAction(() => startRun(taskId))}
          onContinueRun={(taskId, runId) => void runAction(() => continueRun(undefined, taskId, runId))}
          onKillRun={onKillRun}
          onSaveWorkflow={onSaveTaskWorkflow}
          onOpenProfileManagement={openProfileManagement}
        />
      );
    }
    return <RoundDetailPage vm={roundDetail} breadcrumbs={pageBreadcrumbs} selection={roundSelection} refreshing={loading === 'manual'} busy={busy} appConfig={appConfig} workspaceProjectId={bootstrap?.repoRoot ? bootstrap.repoRoot.toLowerCase().replace(/[^a-z0-9\-_]/g, '-') : undefined} onRefresh={() => void refresh('manual')} onSelect={setRoundSelection} onContinueRun={(taskId, runId, promptId) => runAction(() => continueRun(undefined, taskId, runId, promptId))} />;
  }
}
