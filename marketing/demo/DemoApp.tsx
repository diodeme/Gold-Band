import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { WorkspaceShell } from '@/components/workspace/WorkspaceShell';
import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import { workspaceLayoutProfileForPage, WORKSPACE_SIDEBAR_DEFAULT_WIDTH } from '@/components/workspace/workspace-layout';
import { ConversationWorkspaceStore } from '@/components/workspace/right-workspace-context';
import { ConversationRunPage } from '@/pages/ConversationRunPage';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { AvatarPreferencesProvider } from '@/components/avatar/AvatarPreferencesContext';
import { applyAppearance, applyPersonalization } from '@/theme';
import i18n, { i18nLanguage } from '@/i18n';
import type { AppBootstrapVm, ConversationPage, ConversationRunVm, ConversationRunModeVm, PreferencesVm } from '@/types';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Languages, AlertCircle, RotateCw } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { ConversationComposerDraftBoundary } from '@/components/conversation/ConversationComposerDraftBoundary';
import { DemoFrame } from './DemoFrame';
import { demoAgentRegistry, demoProfiles, demoWorkflowTemplates } from './catalog';
import { browserApi } from './runtime';
import { demoSidebar, demoTitle, DEMO_PROJECT_ID } from './fixtures';
import { demoPageFromHash, demoLinkParameters, demoRunModeFromHash, demoHashForPage } from './routes';
import { createDatasetReader, type DemoDataset } from './dataset';
import { missing } from './history-query';

const ContextManagementPage = lazy(() => import('@/pages/ContextManagementPage').then((m) => ({ default: m.ContextManagementPage })));
const SettingsPage = lazy(() => import('@/pages/SettingsPage').then((m) => ({ default: m.SettingsPage })));
const AgentManagementPage = lazy(() => import('@/pages/AgentManagementPage').then((m) => ({ default: m.AgentManagementPage })));
const ConversationHomePage = lazy(() => import('@/pages/ConversationHomePage').then((m) => ({ default: m.ConversationHomePage })));
const RunModeManagementPage = lazy(() => import('@/pages/RunModeManagementPage').then((m) => ({ default: m.RunModeManagementPage })));
const MulticaTaskManagementPage = lazy(() => import('@/pages/MulticaTaskManagementPage').then((m) => ({ default: m.MulticaTaskManagementPage })));
const ScheduledTaskManagementPage = lazy(() => import('@/pages/ScheduledTaskManagementPage').then((m) => ({ default: m.ScheduledTaskManagementPage })));
const ScheduledTaskDetailPage = lazy(() => import('@/pages/ScheduledTaskDetailPage').then((m) => ({ default: m.ScheduledTaskDetailPage })));
const noop = () => {};
const unusedAction = async () => undefined;
const emptyExpansion = {};
const demoWorkspaces = [{ projectId: DEMO_PROJECT_ID, workspacePath: '/default', name: 'Gold Band' }];
const historyReader = createDatasetReader(`${import.meta.env.BASE_URL}data/ji-history/`);

function pageFromHash(): ConversationPage {
  return demoPageFromHash(location.hash);
}

export function DemoApp({ bootstrap, layoutPreferences }: { bootstrap: AppBootstrapVm; layoutPreferences: Record<string, unknown> }) {
  const { t } = useTranslation();
  const [page, setPage] = useState(pageFromHash);
  const [hash, setHash] = useState(location.hash);
  const linkParameters = useMemo(() => demoLinkParameters(hash), [hash]);
  const [preferences, setPreferences] = useState(bootstrap.preferences);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const clientRef = useRef<HTMLDivElement>(null);
  const [runMode, setRunMode] = useState<ConversationRunModeVm>(() => demoRunModeFromHash(location.hash));
  const [store] = useState(() => new ConversationWorkspaceStore());
  const [historyState, setHistoryState] = useState<{ status: 'loading' | 'error' } | { status: 'ready'; data: DemoDataset }>({ status: 'loading' });
  const [historyRequest, setHistoryRequest] = useState(0);
  const history = historyState.status === 'ready' ? historyState.data : null;
  useEffect(() => {
    let active = true;
    setHistoryState({ status: 'loading' });
    void historyReader.dataset().then((data) => { if (active) setHistoryState({ status: 'ready', data }); }).catch(() => { if (active) setHistoryState({ status: 'error' }); });
    return () => { active = false; };
  }, [historyRequest]);
  const sidebar = useMemo(() => {
    const sidebar = { ...demoSidebar(preferences.language), preferences: layoutPreferences };
    if (history) {
      const summary = { runId: history.runId, status: history.source.status, outcome: history.source.outcome, resumable: false, startedAt: history.source.startedAt ?? '', updatedAt: history.source.updatedAt ?? '' };
      sidebar.workspaces.push({ projectId: history.projectId, workspacePath: '/export/workspace', name: 'JI' });
      sidebar.tasksByWorkspace[history.projectId] = [{ projectId: history.projectId, taskId: history.taskId, taskUuid: history.taskUuid, title: history.title, autoTitle: false, runMode: 'workflow', latestRun: summary, runs: [summary], runHistoryStatus: 'ready', pinned: false }];
      sidebar.workspaceTaskPages[history.projectId] = { status: 'ready' };
    }
    return sidebar;
  }, [preferences.language, layoutPreferences, history]);
  useEffect(() => {
    const changed = () => {
      setPage(pageFromHash());
      setHash(location.hash);
      const params = demoLinkParameters(location.hash);
      const mode = params.get('mode');
      const template = params.get('template');
      if (mode === 'auto' || mode === 'workflow' || mode === 'direct' || template) {
        setRunMode(demoRunModeFromHash(location.hash));
      }
    };
    window.addEventListener('hashchange', changed);
    return () => window.removeEventListener('hashchange', changed);
  }, []);
  function navigate(next: ConversationPage) {
    location.hash = demoHashForPage(next);
    setPage(pageFromHash());
    setNavigationOpen(false);
  }
  function toggleNavigation() {
    const profile = workspaceLayoutProfileForPage(page, bootstrap.appConfig.workspaceLayout);
    if ((clientRef.current?.clientWidth ?? window.innerWidth) < profile.centerAutoCollapseWidth + WORKSPACE_SIDEBAR_DEFAULT_WIDTH) {
      setNavigationOpen((open) => !open);
    } else setSidebarCollapsed((collapsed) => !collapsed);
  }
  function updatePreferences(next: PreferencesVm) {
    applyAppearance(next.appearance);
    applyPersonalization(next.personalization);
    void i18n.changeLanguage(i18nLanguage(next.language));
    document.documentElement.lang = i18nLanguage(next.language);
    setPreferences(next);
  }
  return <AvatarPreferencesProvider preferences={preferences.avatars}>
    <DemoFrame clientRef={clientRef}>
    <ConversationComposerDraftBoundary><WorkspaceShell
      titleBarTrailingContent={<Select value={preferences.language} onValueChange={(language) => {
        void browserApi.saveDesktopPreferences(preferences.appearance, preferences.personalization, language as PreferencesVm['language'], preferences.useLocalClaude, preferences.verboseLogging).then(updatePreferences);
      }}><SelectTrigger aria-label={t('settings.language')} className="demo-language mr-2 h-7 w-auto gap-2 border-0 bg-transparent shadow-none"><Languages className="size-3.5" /><SelectValue /></SelectTrigger><SelectContent><SelectItem value="zh-cn">简体中文</SelectItem><SelectItem value="en">English</SelectItem></SelectContent></Select>}
      appName="Gold Band" platform={null} windowFrameStyle="native-compositor"
      feedbackEnabled={false} appConfig={bootstrap.appConfig} vm={sidebar} active={page}
      sidebarCollapsed={sidebarCollapsed} onToggleSidebar={toggleNavigation}
      onSelect={navigate} onOpenPersonalAnalytics={noop} onNewConversation={() => navigate({ kind: 'conversation-home' })} onSearch={noop}
      onPinTask={noop} onUnpinTask={noop} onRenameTask={noop} onDeleteTask={noop}
      onNewConversationInWorkspace={noop} onRetryBootstrap={noop} onRequestWorkspaceTasks={noop}
      onRequestPinnedTasks={noop} onRequestTaskRuns={noop}
      activeWorkspaceId={page.kind === 'conversation-run' ? page.projectId : DEMO_PROJECT_ID} defaultExpandedWorkspaceId={page.kind === 'conversation-run' ? page.projectId : DEMO_PROJECT_ID}
      conversationTaskUuid={page.kind === 'conversation-run' ? page.projectId === history?.projectId ? history.taskUuid : `demo-${page.taskId}` : null}
      conversationWorkspaceStore={store}
      sourceControlWorkspacePath={page.kind === 'conversation-run' && page.projectId !== DEMO_PROJECT_ID ? undefined : '/default'}
    >
      <Suspense fallback={<div className="p-5 text-sm text-muted-foreground">{t('common.loading')}</div>}>
        {historyState.status === 'error' ? <Alert variant="destructive" className="rounded-none border-0"><AlertCircle /><AlertDescription className="flex flex-wrap items-center gap-2">{t('demo.historyLoadFailed')}<Button variant="ghost" size="sm" onClick={() => setHistoryRequest((value) => value + 1)}><RotateCw className="size-4" />{t('common.retry')}</Button></AlertDescription></Alert> : null}
        {page.kind === 'contexts' ? <ContextManagementPage key={linkParameters.get('tab')} initialTab={linkParameters.get('tab') === 'mcp' ? 'mcp' : linkParameters.get('tab') === 'skills' ? 'skills' : 'profiles'} agentRegistry={demoAgentRegistry} onAgentRegistryChange={noop} />
          : page.kind === 'agents' ? <AgentManagementPage vm={demoAgentRegistry} loading={false} onRefresh={noop} onRegistryChange={noop} />
          : page.kind === 'multica-tasks' ? <MulticaTaskManagementPage key={preferences.language} onSelectRun={(projectId, taskId, runId) => navigate({ kind: 'conversation-run', projectId, taskId, runId })} onPrepareMulticaTask={() => navigate({ kind: 'conversation-home' })} />
          : page.kind === 'scheduled-tasks' ? <ScheduledTaskManagementPage key={preferences.language} onCreate={() => navigate({ kind: 'scheduled-task-create' })} onOpenDetail={(task) => navigate({ kind: 'scheduled-task-detail', projectId: task.projectId, scheduledTaskId: task.id })} />
          : page.kind === 'scheduled-task-detail' ? <ScheduledTaskDetailPage key={`${page.scheduledTaskId}:${preferences.language}`} projectId={page.projectId} scheduledTaskId={page.scheduledTaskId} onBack={() => navigate({ kind: 'scheduled-tasks' })} onOpenOccurrence={navigate} />
          : page.kind === 'conversation-home' || page.kind === 'scheduled-task-create' ? <ConversationHomePage
            initialScheduledMode={page.kind === 'scheduled-task-create'}
            onScheduledModeExit={() => navigate({ kind: 'conversation-home' })}
            projectId={DEMO_PROJECT_ID} workspaceName="Gold Band" workspaces={demoWorkspaces}
            runMode={runMode} onRunModeChange={setRunMode} agentRegistry={demoAgentRegistry}
            workflowTemplates={demoWorkflowTemplates} profiles={demoProfiles(preferences.language)}
            busy={false} inlineContentMaxBytes={0} workLocation="main"
            onLoadProfiles={async () => (await browserApi.getProfiles()).profiles} onSubmit={unusedAction}
            onCreateScheduledTask={unusedAction}
            onOpenAgentManagement={() => navigate({ kind: 'agents' })} onOpenScheduledTasks={() => navigate({ kind: 'scheduled-tasks' })}
            onOpenRunModeSettings={() => navigate({ kind: 'run-mode-management' })}
            onWorkspaceChange={noop} onWorkLocationChange={noop} />
          : page.kind === 'run-mode-management' ? <RunModeManagementPage key={preferences.language}
            projectId={DEMO_PROJECT_ID} workspaceName="Gold Band" workspaces={demoWorkspaces}
            runMode={runMode} agentRegistry={demoAgentRegistry} workflowTemplates={demoWorkflowTemplates}
            onProjectChange={noop} onSave={setRunMode} />
          : page.kind === 'settings' ? <SettingsPage
            preferences={preferences} appInfo={bootstrap.appInfo} updaterSettings={bootstrap.updaterSettings}
            updateStatus={bootstrap.updateStatus} showAdvancedUpdateDot={false} showUpdatesSectionDot={false}
            downloadProgress={null} clientVersion={bootstrap.clientVersion} busy={false}
            onSave={(...args) => { void browserApi.saveDesktopPreferences(...args).then(updatePreferences); }}
            onSaveAvatar={unusedAction} onSelectRecentAvatar={unusedAction} onSaveAvatarShape={unusedAction}
            onClearAvatar={unusedAction} onImportWallpaper={unusedAction} onSelectRecentWallpaper={unusedAction}
            onSaveWallpaperOpacity={unusedAction} onRestoreThemeWallpaper={unusedAction}
            onSaveUpdaterSettings={unusedAction} onCheckUpdate={unusedAction} onInstallUpdate={async () => {}}
            onViewSettings={noop} onViewAdvanced={noop}
          /> : page.kind === 'conversation-run' ? <DemoConversation key={`${page.projectId}:${page.taskId}:${page.runId}:${preferences.language}`} page={page} parameters={linkParameters} bootstrap={bootstrap} language={preferences.language} title={page.projectId === history?.projectId ? history.title : demoTitle(page.taskId, preferences.language)} /> : null}
      </Suspense>
    </WorkspaceShell></ConversationComposerDraftBoundary>
    </DemoFrame>
    <Sheet open={navigationOpen} onOpenChange={setNavigationOpen}>
      <SheetContent side="left" className="w-[min(85vw,320px)] gap-0 p-0" closeLabel={t('common.close')}>
        <SheetHeader className="px-4 py-3"><SheetTitle>Gold Band</SheetTitle></SheetHeader>
        <ConversationSidebar vm={sidebar} active={page} defaultExpandedWorkspaceId={DEMO_PROJECT_ID}
          onSelect={navigate} onNewConversation={() => navigate({ kind: 'conversation-home' })} onSearch={noop} onPinTask={noop} onUnpinTask={noop}
          onRenameTask={noop} onDeleteTask={noop} onRetryBootstrap={noop} onRequestWorkspaceTasks={noop}
          onRequestPinnedTasks={noop} onRequestTaskRuns={noop} />
      </SheetContent>
    </Sheet>
  </AvatarPreferencesProvider>;
}

function DemoConversation({ page, parameters, bootstrap, title }: { page: Extract<ConversationPage, { kind: 'conversation-run' }>; parameters: URLSearchParams; bootstrap: AppBootstrapVm; language: PreferencesVm['language']; title: string }) {
  const { projectId, taskId, runId } = page;
  const { t } = useTranslation();
  const [run, setRun] = useState<ConversationRunVm | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    setRun(null);
    setFailed(false);
    browserApi.getConversationRun(projectId, taskId, runId).then(async (next) => {
      const leaves = next.sessionTree.rounds.flatMap((round) => round.nodes.flatMap((node) => [...node.attempts, ...(node.outerNodes ?? []).flatMap((outer) => outer.attempts)]));
      const explicit = ['round', 'node', 'attempt', 'outerNode', 'outerAttempt'].some((key) => parameters.has(key));
      const key = (leaf: typeof leaves[number]) => leaf.outerNodeId ? `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}` : `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
      const leaf = explicit ? leaves.find((leaf) => (!parameters.has('round') || leaf.roundId === parameters.get('round')) && (!parameters.has('node') || leaf.nodeId === parameters.get('node')) && (!parameters.has('attempt') || leaf.attemptId === parameters.get('attempt')) && (leaf.outerNodeId ?? null) === parameters.get('outerNode') && (leaf.outerAttemptId ?? null) === parameters.get('outerAttempt')) : leaves.find((leaf) => key(leaf) === next.sessionTree.selectedSessionKey);
      if (!leaf) missing({ taskId, runId });
      if (leaf) {
        next.selectedSession = await browserApi.getAcpSession(projectId, taskId, runId, leaf.roundId, leaf.nodeId, leaf.attemptId, undefined, undefined, leaf.outerNodeId, leaf.outerAttemptId);
        next.sessionTree.selectedSessionKey = key(leaf);
      }
      if (active) setRun(next);
    }).catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [projectId, taskId, runId, parameters]);
  if (!run) return <div role={failed ? 'alert' : 'status'} className="p-5 text-sm text-muted-foreground">{t(failed ? 'common.operationFailed' : 'common.loading')}</div>;
  return <ConversationRunPage run={run} taskTitle={title} appConfig={bootstrap.appConfig}
    agentRegistry={demoAgentRegistry} onRerun={noop} onEditWorkflow={noop} onSelectSession={(leaf) => { location.hash = demoHashForPage(page, leaf); }}
    followMode="manual" initialSessionTreeExpansion={emptyExpansion} onSessionTreeExpansionChange={noop} />;
}
