import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  checkUpdateManual,
  chooseWorkspace,
  continueRun,
  createTask,
  dismissUpdateAnnouncement,
  downloadAndInstallUpdate,
  getAgentRegistry,
  markSettingsAdvancedUpdateSeen,
  markSettingsUpdateSeen,
  getAppBootstrap,
  getRoundDetail,
  getTaskList,
  getWorkflow,
  killRun,
  saveDesktopPreferences,
  saveUpdaterSettings,
  saveTaskWorkflow,
  selectRecentWorkspace,
  startRun,
} from './api';
import { isTauriRuntime } from './api/shared';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Breadcrumbs } from './components/Breadcrumbs';
import { Button } from '@/components/ui/button';
import { X } from 'lucide-react';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Markdown } from '@/components/prompt-kit/markdown';
import { Shell } from './components/Shell';
import i18n, { displayAppError, i18nLanguage } from './i18n';
import { useTranslation } from 'react-i18next';
import { AgentManagementPage } from './pages/AgentManagementPage';
import { ContextManagementPage } from './pages/ContextManagementPage';
import { RoundDetailPage } from './pages/RoundDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { TaskListPage } from './pages/TaskListPage';
import { WorkflowPage } from './pages/WorkflowPage';
import { WorkspaceSelectPage } from './pages/WorkspaceSelectPage';
import { pushRoute, replaceRoute, routeFromPath, taskListPage } from './routes';
import { applyFont, applyTheme } from './theme';
import { StartupSplash, type SplashPhase } from './components/StartupSplash';
import type {
  AgentRegistryVm,
  AppBootstrapVm,
  AppInfoVm,
  CreateTaskInput,
  DesktopFontPreference,
  DesktopLanguage,
  DesktopThemePreference,
  PreferencesVm,
  UpdateBadgeStateVm,
  PrimaryModule,
  RoundDetailVm,
  RoundSelection,
  StartupCheckResult,
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
type RefreshMode = 'initial' | 'manual' | 'background';
type VisibleRefreshMode = Exclude<RefreshMode, 'background'>;

export function App() {
  const initialRoute = routeFromPath(window.location.pathname);
  const [bootstrap, setBootstrap] = useState<AppBootstrapVm | null>(null);
  const [primaryModule, setPrimaryModule] = useState<PrimaryModule>(initialRoute.module);
  const [taskPage, setTaskPage] = useState<TaskPage>(initialRoute.taskPage);
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
  const [startupPhase, setStartupPhase] = useState<SplashPhase>('checking');
  const [splashProgress, setSplashProgress] = useState({ downloaded: 0, total: null as number | null });
  const [splashUpdateVersion, setSplashUpdateVersion] = useState<string | null>(null);
  const backgroundRefreshInFlightRef = useRef(false);

  const preferences = bootstrap?.preferences ?? defaultPreferences;
  const updaterSettings = bootstrap?.updaterSettings ?? defaultUpdaterSettings;
  const updateStatus = bootstrap?.updateStatus ?? defaultUpdateStatus;
  const updateBadges = bootstrap?.updateBadges ?? defaultUpdateBadges;
  const persistedAvailableUpdate = bootstrap?.persistedAvailableUpdate ?? null;
  const effectiveAvailableUpdate = updateStatus.update ?? persistedAvailableUpdate;
  const availableUpdateVersion = effectiveAvailableUpdate?.version ?? null;
  const showSettingsUpdateDot = availableUpdateVersion !== null && updateBadges.settingsEntrySeenVersion !== availableUpdateVersion;
  const showSettingsAdvancedUpdateDot = availableUpdateVersion !== null && updateBadges.settingsAdvancedSeenVersion !== availableUpdateVersion;
  const showUpdatesSectionDot = availableUpdateVersion !== null;
  const appInfo = bootstrap?.appInfo ?? defaultAppInfo;
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
    void i18n.changeLanguage(i18nLanguage(preferences.language));
  }, [preferences.language]);

  useEffect(() => {
    replaceRoute(primaryModule, taskPage);
    const onPopState = () => {
      const nextRoute = routeFromPath(window.location.pathname);
      setPrimaryModule(nextRoute.module);
      setTaskPage(nextRoute.taskPage);
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
    void listen<StartupCheckResult>('gold-band://startup-update-check', (event) => {
      if (!active) return;
      if (event.payload.critical) {
        setStartupPhase('downloading');
      } else {
        setStartupPhase('done');
      }
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
      setSplashProgress(event.payload);
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
    const interval = window.setInterval(() => void refresh('background'), 10000);
    return () => window.clearInterval(interval);
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

  const runAction = async <T,>(action: () => Promise<T>) => {
    setBusy(true);
    setError(null);
    try {
      const result = await action();
      await refresh('background');
      return result;
    } catch (err) {
      setError(displayAppError(t, err));
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const onKillRun = (taskId: string, runId: string) => {
    if (window.confirm(t('common.confirmKill'))) {
      void runAction(() => killRun(taskId, runId));
    }
  };

  const onCreateTask = async (input: CreateTaskInput) => {
    const created = await runAction(() => createTask(input));
    if (created) setWorkflow(created);
    return created;
  };

  const onSaveTaskWorkflow = async (taskId: string, workflow: WorkflowDsl) => {
    const saved = await runAction(() => saveTaskWorkflow(taskId, workflow));
    if (saved) setWorkflow(saved);
    return saved;
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
        clientVersion: '',
        appInfo: defaultAppInfo,
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

  useEffect(() => {
    if (startupPhase !== 'downloading') return;
    const { downloaded, total } = splashProgress;
    if (total !== null && total > 0 && downloaded >= total) {
      setStartupPhase('installing');
    }
  }, [startupPhase, splashProgress]);

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

  const content = workspacePickerOpen
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

  if (startupPhase !== 'done') {
    return (
      <StartupSplash
        phase={startupPhase}
        progress={splashProgress}
        version={splashUpdateVersion}
      />
    );
  }

  return (
    <Shell
      active={primaryModule}
      appName={appInfo.appName}
      repoRoot={bootstrap?.repoRoot}
      needsWorkspace={bootstrap?.needsWorkspace}
      showSettingsUpdateDot={showSettingsUpdateDot}
      onSelect={(module) => {
        const nextTaskPage = module === 'task-orchestration' ? taskListPage : taskPage;
        setWorkspacePickerOpen(false);
        setPrimaryModule(module);
        setTaskPage(nextTaskPage);
        pushRoute(module, nextTaskPage);
      }}
      onChooseWorkspace={() => setWorkspacePickerOpen(true)}
    >
      {error ? <Alert variant="destructive" className="mx-8 mt-4"><AlertDescription>{error}</AlertDescription></Alert> : null}
      {shouldShowUpdateAnnouncement ? (
        <div className="pointer-events-none fixed left-1/2 top-1 z-50 -translate-x-1/2">
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
    </Shell>
  );

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
    return <RoundDetailPage vm={roundDetail} breadcrumbs={pageBreadcrumbs} selection={roundSelection} refreshing={loading === 'manual'} busy={busy} onRefresh={() => void refresh('manual')} onSelect={setRoundSelection} onContinueRun={(taskId, runId, promptId) => runAction(() => continueRun(taskId, runId, promptId))} />;
  }
}
