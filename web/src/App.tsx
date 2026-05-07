import { useCallback, useEffect, useState } from 'react';
import {
  chooseWorkspace,
  continueRun,
  getAppBootstrap,
  getRoundDetail,
  getTaskList,
  getWorkflow,
  killRun,
  saveDesktopPreferences,
  selectRecentWorkspace,
  startRun,
} from './api';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Breadcrumbs } from './components/Breadcrumbs';
import { Shell } from './components/Shell';
import i18n, { i18nLanguage } from './i18n';
import { useTranslation } from 'react-i18next';
import { RoundDetailPage } from './pages/RoundDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { TaskListPage } from './pages/TaskListPage';
import { WorkflowPage } from './pages/WorkflowPage';
import { WorkspaceSelectPage } from './pages/WorkspaceSelectPage';
import { pushRoute, replaceRoute, routeFromPath } from './routes';
import { applyTheme } from './theme';
import type {
  AppBootstrapVm,
  DesktopLanguage,
  DesktopThemePreference,
  PreferencesVm,
  PrimaryModule,
  RoundDetailVm,
  RoundSelection,
  TaskListVm,
  TaskPage,
  WorkflowVm,
} from './types';

const defaultPreferences: PreferencesVm = { theme: 'system', language: 'zh-cn' };
type RefreshMode = 'initial' | 'manual' | 'background';
type VisibleRefreshMode = Exclude<RefreshMode, 'background'>;

export function App() {
  const initialRoute = routeFromPath(window.location.pathname);
  const [bootstrap, setBootstrap] = useState<AppBootstrapVm | null>(null);
  const [primaryModule, setPrimaryModule] = useState<PrimaryModule>(initialRoute.module);
  const [taskPage, setTaskPage] = useState<TaskPage>(initialRoute.taskPage);
  const [roundSelection, setRoundSelection] = useState<RoundSelection>({ kind: 'round' });
  const [taskList, setTaskList] = useState<TaskListVm | null>(null);
  const [workflow, setWorkflow] = useState<WorkflowVm | null>(null);
  const [roundDetail, setRoundDetail] = useState<RoundDetailVm | null>(null);
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const [loading, setLoading] = useState<VisibleRefreshMode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const preferences = bootstrap?.preferences ?? defaultPreferences;
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
      .then(setBootstrap)
      .catch((err) => setError(String(err)));
  }, []);

  const resetWorkspaceViews = () => {
    setTaskPage({ kind: 'task-list' });
    setRoundSelection({ kind: 'round' });
    setTaskList(null);
    setWorkflow(null);
    setRoundDetail(null);
    setPrimaryModule('task-orchestration');
    setWorkspacePickerOpen(false);
  };

  const hasPageData = taskPage.kind === 'task-list'
    ? taskList !== null
    : taskPage.kind === 'workflow'
      ? workflow !== null
      : roundDetail !== null;

  const refresh = useCallback(async (mode: RefreshMode = 'manual') => {
    if (!bootstrap) return;
    if (mode !== 'background') setLoading(mode);
    setError(null);
    try {
      if (taskPage.kind === 'task-list') {
        setTaskList(await getTaskList());
      } else if (taskPage.kind === 'workflow') {
        setWorkflow(await getWorkflow(taskPage.taskId));
      } else if (taskPage.kind === 'round-detail') {
        setRoundDetail(await getRoundDetail(taskPage.taskId, taskPage.runId, taskPage.roundId, roundSelection));
      }
    } catch (err) {
      setError(String(err));
    } finally {
      if (mode !== 'background') setLoading(null);
    }
  }, [bootstrap, roundSelection, taskPage]);

  useEffect(() => {
    void refresh(hasPageData ? 'background' : 'initial');
  }, [hasPageData, refresh]);

  useEffect(() => {
    const active = taskList?.tasks.some((task) => task.latestRun?.status === 'running')
      || workflow?.runs.some((group) => group.run.status === 'running')
      || roundDetail?.run.status === 'running'
      || roundDetail?.round.status === 'running'
      || roundDetail?.graph.nodes.some((node) => node.status === 'running');
    if (!active) return undefined;
    const interval = window.setInterval(() => void refresh('background'), 1000);
    return () => window.clearInterval(interval);
  }, [refresh, taskList, workflow, roundDetail]);

  const navigate = (page: TaskPage) => {
    setPrimaryModule('task-orchestration');
    setWorkspacePickerOpen(false);
    setTaskPage(page);
    setRoundSelection({ kind: 'round' });
    pushRoute('task-orchestration', page);
  };

  const runAction = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refresh('background');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onKillRun = (taskId: string, runId: string) => {
    if (window.confirm(t('common.confirmKill'))) {
      void runAction(() => killRun(taskId, runId));
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
      setError(String(err));
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
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const onSavePreferences = async (theme: DesktopThemePreference, language: DesktopLanguage) => {
    setBusy(true);
    try {
      const saved = await saveDesktopPreferences(theme, language);
      setBootstrap((current) => current ? { ...current, preferences: saved } : { repoRoot: '', recentWorkspaces: [], preferences: saved });
      setTaskList(null);
      setWorkflow(null);
      setRoundDetail(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const content = workspacePickerOpen
    ? (
      <WorkspaceSelectPage
        bootstrap={bootstrap}
        busy={busy}
        onChooseWorkspace={onChooseWorkspace}
        onSelectRecentWorkspace={onSelectRecentWorkspace}
      />
    )
    : primaryModule === 'settings'
      ? <SettingsPage preferences={preferences} onSave={onSavePreferences} />
      : renderTaskContent();

  return (
    <Shell
      active={primaryModule}
      repoRoot={bootstrap?.repoRoot}
      onSelect={(module) => {
        setWorkspacePickerOpen(false);
        setPrimaryModule(module);
        pushRoute(module, taskPage);
      }}
      onChooseWorkspace={() => setWorkspacePickerOpen(true)}
    >
      {!workspacePickerOpen && primaryModule === 'task-orchestration' && taskPage.kind !== 'task-list' ? <Breadcrumbs page={taskPage} onNavigate={navigate} /> : null}
      {error ? <Alert variant="destructive" className="mx-8 mt-4"><AlertDescription>{error}</AlertDescription></Alert> : null}
      {content}
    </Shell>
  );

  function renderTaskContent() {
    if (taskPage.kind === 'task-list') {
      return <TaskListPage vm={taskList} loading={loading} onNavigate={navigate} onRefresh={() => void refresh('manual')} />;
    }
    if (taskPage.kind === 'workflow') {
      return (
        <WorkflowPage
          vm={workflow}
          busy={busy}
          onNavigate={navigate}
          onStartRun={(taskId) => void runAction(() => startRun(taskId))}
          onContinueRun={(taskId, runId) => void runAction(() => continueRun(taskId, runId))}
          onKillRun={onKillRun}
        />
      );
    }
    return <RoundDetailPage vm={roundDetail} selection={roundSelection} onSelect={setRoundSelection} />;
  }
}
