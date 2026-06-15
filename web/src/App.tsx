import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useMemo, useRef, useState, useTransition } from 'react';
import {
  checkUpdateManual,
  chooseWorkspace,
  continueRun,
  createConversationRun,
  createTask,
  dismissUpdateAnnouncement,
  downloadAndInstallUpdate,
  getAgentRegistry,
  getConversationRun,
  getConversationRunMode,
  getConversationSidebar,
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
import { useTranslation } from 'react-i18next';
import { AgentManagementPage } from './pages/AgentManagementPage';
import { ContextManagementPage } from './pages/ContextManagementPage';
import { ConversationHomePage } from './pages/ConversationHomePage';
import { ConversationRunPage } from './pages/ConversationRunPage';
import { ConversationSearchDialog } from './components/conversation/ConversationSearchDialog';
import { RunModeManagementPage } from './pages/RunModeManagementPage';
import { RoundDetailPage } from './pages/RoundDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { TaskListPage } from './pages/TaskListPage';
import { WorkflowPage } from './pages/WorkflowPage';
import { WorkspaceSelectPage } from './pages/WorkspaceSelectPage';
import { pushRoute, replaceRoute, routeFromPath, taskListPage, conversationHomePage } from './routes';
import { applyFont, applyTheme } from './theme';
import { resolveWindowControlsPolicy, shouldApplyRuntimeWindowPolicy } from './lib/window-controls';
import type {
  AgentRegistryVm,
  AppBootstrapVm,
  AppConfigVm,
  AppInfoVm,
  ConversationPage,
  ConversationRunModeVm,
  ConversationRunVm,
  WorkflowTemplateStore,
  ConversationSidebarVm,
  CreateTaskInput,
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
} from './types';

const defaultPreferences: PreferencesVm = { theme: 'system', language: 'zh-cn', font: 'app-default', useLocalClaude: false };
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
  const [conversationSearchOpen, setConversationSearchOpen] = useState(false);
  const [conversationRunMode, setConversationRunMode] = useState<ConversationRunModeVm>({ mode: 'auto' });
  const [conversationRun, setConversationRun] = useState<ConversationRunVm | null>(null);
  const [conversationWorkflowTemplates, setConversationWorkflowTemplates] = useState<WorkflowTemplateStore | null>(null);
  const [, startTransition] = useTransition();

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(null);
  // Derive active workspace: persisted lastActiveWorkspaceId > explicit state > first workspace
  const effectiveWorkspaceId =
    activeWorkspaceId
    ?? conversationSidebar.lastActiveWorkspaceId
    ?? conversationSidebar.workspaces[0]?.projectId
    ?? 'default';
  const activeWorkspace = conversationSidebar.workspaces.find((w) => w.projectId === effectiveWorkspaceId)
    ?? conversationSidebar.workspaces[0];
  const defaultProjectId = activeWorkspace?.projectId ?? 'default';
  const defaultWorkspaceName = activeWorkspace?.name ?? 'Default Workspace';
  const [roundSelection, setRoundSelection] = useState<RoundSelection>({ kind: 'round' });
  const [agentRegistry, setAgentRegistry] = useState<AgentRegistryVm | null>(null);
  const [taskList, setTaskList] = useState<TaskListVm | null>(null);
  const [workflow, setWorkflow] = useState<WorkflowVm | null>(null);
  const [roundDetail, setRoundDetail] = useState<RoundDetailVm | null>(null);
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const [loading, setLoading] = useState<VisibleRefreshMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<{ downloaded: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updateAnnouncementOpen, setUpdateAnnouncementOpen] = useState(false);
  const backgroundRefreshInFlightRef = useRef(false);

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
    if (!isTauriRuntime() || !bootstrap?.platform) return;
    if (!shouldApplyRuntimeWindowPolicy(bootstrap.platform)) return;
    const currentWindow = getCurrentWindow();
    const policy = resolveWindowControlsPolicy(bootstrap.platform);

    currentWindow.setDecorations(policy.decorations).catch(() => {});
    if (policy.titleBarStyle) {
      currentWindow.setTitleBarStyle(policy.titleBarStyle).catch(() => {});
    }
  }, [bootstrap?.platform]);

  useEffect(() => {
    void i18n.changeLanguage(i18nLanguage(preferences.language));
  }, [preferences.language]);

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
      .then(setConversationSidebar)
      .catch(() => {}); // Silently fail - sidebar will show empty state
  }, [bootstrap, uiMode]);

  useEffect(() => {
    if (!bootstrap || uiMode !== 'conversation') return;
    getAgentRegistry().then(setAgentRegistry).catch(() => {});
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
      .then(setConversationRun)
      .catch(() => setConversationRun(null));
  }, [bootstrap, uiMode, conversationPage]);

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
      const saved = await saveTaskWorkflow(taskId, workflow);
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

  const onSavePreferences = async (theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean) => {
    setBusy(true);
    try {
      const saved = await saveDesktopPreferences(theme, language, font, useLocalClaude);
      setBootstrap((current) => current ? { ...current, preferences: saved } : {
        repoRoot: '',
        recentWorkspaces: [],
        preferences: saved,
        updaterSettings: defaultUpdaterSettings,
        updateStatus: defaultUpdateStatus,
        updateBadges: defaultUpdateBadges,
        metricsSettings: defaultMetricsSettings,
        clientVersion: '',
        platform: 'linux',
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
    setPrimaryModule('settings');
    pushRoute('settings', taskPage);
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

  const onToggleUiMode = () => {
    const nextMode: DesktopUiMode = uiMode === 'conversation' ? 'workbench' : 'conversation';
    setUiMode(nextMode);
    if (typeof localStorage !== 'undefined') localStorage.setItem('gold-band-ui-mode', nextMode);
    saveDesktopUiMode(nextMode).catch(() => {});
    if (nextMode === 'conversation' && bootstrap?.repoRoot) {
      syncConversationWorkspace(bootstrap.repoRoot).then(setConversationSidebar).catch(() => {});
    }
    if (nextMode === 'conversation') {
      pushRoute(primaryModule, taskPage, conversationPage);
    } else {
      pushRoute(primaryModule, taskPage);
    }
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
      appName={appInfo.appName}
      platform={bootstrap?.platform}
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
        setActiveWorkspaceId(null);
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
          .then(setConversationSidebar)
          .catch(() => {});
        if (conversationPage.kind === 'conversation-run' && conversationPage.projectId === projectId && conversationPage.taskId === taskId) {
          setConversationRun((prev) => prev ? { ...prev, title } : prev);
        }
      }}
      onConversationPinTask={(projectId, taskId) => {
        pinConversation(projectId, taskId).then(setConversationSidebar).catch(() => {});
      }}
      onConversationUnpinTask={(projectId, taskId) => {
        unpinConversation(projectId, taskId).then(setConversationSidebar).catch(() => {});
      }}
      onConversationNewInWorkspace={(projectId) => {
        setActiveWorkspaceId(projectId);
        setConversationPage({ kind: 'conversation-home' });
      }}
      onConversationAddWorkspace={() => {
        addConversationWorkspace().then(setConversationSidebar).catch(() => {});
      }}
      onConversationRemoveWorkspace={(projectId) => {
        removeConversationWorkspace(projectId).then((sidebar) => {
          setConversationSidebar(sidebar);
          if (activeWorkspaceId === projectId) {
            setActiveWorkspaceId(null);
          }
        }).catch(() => {});
      }}
    >
      {error ? <Alert variant="destructive" className="mx-8 mt-4"><AlertDescription>{error}</AlertDescription></Alert> : null}
      {shouldShowUpdateAnnouncement ? (
        <div className="pointer-events-none fixed left-1/2 top-13 z-50 -translate-x-1/2">
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
          busy={busy}
          onRunModeChange={updateConversationRunMode}
          onSubmit={(input) => {
            const nextMode: ConversationRunModeVm = input.runMode === 'auto'
              ? { mode: 'auto', autoConfig: input.autoConfig ?? conversationRunMode.autoConfig }
              : { mode: 'workflow', workflowTemplateId: input.workflowTemplateId ?? conversationRunMode.workflowTemplateId };
            setConversationRunMode(nextMode);
            setBusy(true);
            saveConversationRunMode(input.projectId, nextMode).catch(() => {});
            validateConversationCreate(input)
              .then(async (validation) => {
                if (!validation.valid) {
                  setError(validation.missingItems.map((m) => t(`conversation.validation.${m.code}`, { defaultValue: m.label || m.code })).join('\n'));
                  return;
                }
                const run = await createConversationRun(input);
                setConversationRun(run);
                getConversationSidebar().then(setConversationSidebar).catch(() => {});
                pushRoute('task-orchestration', taskListPage, {
                  kind: 'conversation-run',
                  projectId: run.projectId,
                  taskId: run.taskId,
                  runId: run.runId,
                });
              })
              .catch((err) => setError(displayAppError(t, err)))
              .finally(() => setBusy(false));
          }}
          onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
          onWorkspaceChange={(projectId) => {
            setActiveWorkspaceId(projectId);
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
                setConversationRun(run);
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
            const dsl = JSON.parse(json) as Parameters<typeof saveTaskWorkflow>[1];
            await saveTaskWorkflow(conversationPage.taskId, dsl);
            const refreshed = await getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId);
            setConversationRun(refreshed);
          }}
          onSelectSession={(leaf) => {
            const key = leaf.outerNodeId
              ? `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`
              : `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
            switchConversationSession(
              conversationPage.taskId,
              conversationPage.runId,
              leaf.roundId,
              leaf.nodeId,
              leaf.attemptId,
              leaf.outerNodeId,
              leaf.outerAttemptId,
            ).then((switched) => {
              startTransition(() => {
                setConversationRun((prev) => prev ? {
                  ...prev,
                  selectedSession: switched.selectedSession,
                  artifacts: switched.artifacts,
                  attachments: switched.attachments,
                  sessionTree: { ...prev.sessionTree, selectedSessionKey: key },
                } : prev);
              });
            }).catch(() => {});
          }}
          onSessionStopped={() => {}}
          onContinueRun={() => {
            continueRun(conversationPage.taskId, conversationPage.runId)
              .then(() => getConversationRun(conversationPage.projectId, conversationPage.taskId, conversationPage.runId))
              .then(setConversationRun)
              .catch((err) => setError(displayAppError(t, err)));
          }}
          onTitleChange={(title) => {
            setConversationRun((prev) => prev ? { ...prev, title } : prev);
            updateTaskMetadata(conversationPage.projectId, conversationPage.taskId, title)
              .then(() => getConversationSidebar())
              .then(setConversationSidebar)
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
          busy={busy}
      onRunModeChange={updateConversationRunMode}
      onSubmit={(_input) => {}}
      onOpenRunModeSettings={() => setConversationPage({ kind: 'run-mode-management' })}
      onWorkspaceChange={(projectId) => {
        setActiveWorkspaceId(projectId);
        getConversationRunMode(projectId).then((mode) => { if (mode) setConversationRunMode(mode); }).catch(() => {});
      }}
    />;
  }

  function renderTaskContent() {
    const pageBreadcrumbs = <Breadcrumbs page={taskPage} onNavigate={navigate} />;
    if (taskPage.kind === 'task-list') {
      return <TaskListPage vm={taskList} loading={loading} breadcrumbs={pageBreadcrumbs} onNavigate={navigate} onRefresh={() => void refresh('manual')} onCreateTask={onCreateTask} onOpenProfileManagement={openProfileManagement} />;
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
          onContinueRun={(taskId, runId) => void runAction(() => continueRun(taskId, runId))}
          onKillRun={onKillRun}
          onSaveWorkflow={onSaveTaskWorkflow}
          onOpenProfileManagement={openProfileManagement}
        />
      );
    }
    return <RoundDetailPage vm={roundDetail} breadcrumbs={pageBreadcrumbs} selection={roundSelection} refreshing={loading === 'manual'} busy={busy} appConfig={appConfig} onRefresh={() => void refresh('manual')} onSelect={setRoundSelection} onContinueRun={(taskId, runId, promptId) => runAction(() => continueRun(taskId, runId, promptId))} />;
  }
}
