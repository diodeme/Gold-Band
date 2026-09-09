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
import { Languages } from 'lucide-react';
import { ConversationComposerDraftBoundary } from '@/components/conversation/ConversationComposerDraftBoundary';
import { DemoFrame } from './DemoFrame';
import { demoAgentRegistry, demoProfiles, demoWorkflowTemplates } from './catalog';
import { browserApi } from './runtime';
import { demoSidebar, demoTitle, DEMO_TASKS, DEMO_PROJECT_ID } from './fixtures';
import { demoPageFromHash, demoLinkParameters, demoRunModeFromHash } from './routes';

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
  const sidebar = useMemo(() => ({ ...demoSidebar(preferences.language), preferences: layoutPreferences }), [preferences.language, layoutPreferences]);
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
    const hash = next.kind === 'conversation-run' ? next.taskId : next.kind;
    if (![...DEMO_TASKS, 'contexts', 'settings', 'run-mode-management', 'agents', 'conversation-home', 'multica-tasks', 'scheduled-tasks', 'scheduled-task-create', 'scheduled-task-detail'].includes(hash)) return;
    location.hash = next.kind === 'conversation-run' ? `${hash}?${new URLSearchParams({ run: next.runId })}` : next.kind === 'scheduled-task-detail' ? `${hash}?${new URLSearchParams({ id: next.scheduledTaskId })}` : hash;
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
      activeWorkspaceId={DEMO_PROJECT_ID} defaultExpandedWorkspaceId={DEMO_PROJECT_ID}
      conversationTaskUuid={page.kind === 'conversation-run' ? `demo-${page.taskId}` : null}
      conversationWorkspaceStore={store}
      sourceControlWorkspacePath="/default"
    >
      <Suspense fallback={<div className="p-5 text-sm text-muted-foreground">{t('common.loading')}</div>}>
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
          /> : page.kind === 'conversation-run' ? <DemoConversation key={`${page.taskId}:${page.runId}:${preferences.language}`} taskId={page.taskId} runId={page.runId} roundId={linkParameters.get('round')} nodeId={linkParameters.get('node')} bootstrap={bootstrap} language={preferences.language} /> : null}
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

function DemoConversation({ taskId, runId, roundId, nodeId, bootstrap, language }: { taskId: string; runId: string; roundId: string | null; nodeId: string | null; bootstrap: AppBootstrapVm; language: PreferencesVm['language'] }) {
  const { t } = useTranslation();
  const [run, setRun] = useState<ConversationRunVm | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let active = true;
    setRun(null);
    setFailed(false);
    browserApi.getConversationRun(DEMO_PROJECT_ID, taskId, runId).then(async (next) => {
      const selectedRound = next.sessionTree.rounds.find((round) => round.roundId === roundId) ?? next.sessionTree.rounds.at(-1)!;
      const leaf = selectedRound.nodes.flatMap((node) => node.attempts).find((leaf) => leaf.nodeId === (nodeId ?? next.selectedSession?.nodeId));
      if (leaf) {
        next.selectedSession = await browserApi.getAcpSession(DEMO_PROJECT_ID, taskId, runId, leaf.roundId, leaf.nodeId, leaf.attemptId);
        next.sessionTree.selectedSessionKey = `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
      }
      if (active) setRun(next);
    }).catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [taskId, runId, roundId, nodeId]);
  if (!run) return <div className="p-5 text-sm text-muted-foreground">{t(failed ? 'common.error' : 'common.loading')}</div>;
  return <ConversationRunPage run={run} taskTitle={demoTitle(taskId, language)} appConfig={bootstrap.appConfig}
    agentRegistry={demoAgentRegistry} onRerun={noop} onEditWorkflow={noop} onSelectSession={(leaf) => { location.hash = `${taskId}?${new URLSearchParams({ run: runId, round: leaf.roundId, node: leaf.nodeId })}`; }}
    followMode="manual" initialSessionTreeExpansion={emptyExpansion} onSessionTreeExpansionChange={noop} />;
}
