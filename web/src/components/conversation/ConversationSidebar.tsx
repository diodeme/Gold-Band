import { Pin, PinOff, MessageSquare, Search, Bot, Library, Route, AlarmClock, Settings, ChevronDown, Loader2, Pencil, Plus, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { memo, useEffect, useMemo, useRef, useState } from 'react';
import type { ConversationPage, ConversationSidebarVm, ConversationTaskRowVm, ConversationWorkspaceVm } from '../../types';
import { saveConversationPreference } from '../../api';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { conversationRunIdentityKey, conversationTaskIdentityKey } from '@/lib/conversation-run-identity';
import { formatCompactRelativeTime } from '@/lib/datetime';

export const conversationSidebarActivityIconClass = 'motion-safe:animate-pulse';

export function conversationSidebarTerminalResultDotClass(
  kind: NonNullable<ConversationTaskRowVm['unreadTerminalResult']>['kind'],
) {
  if (kind === 'completed') return 'bg-gold-success';
  if (kind === 'stopped') return 'bg-gold-warning';
  return 'bg-gold-danger';
}

export type ConversationSidebarNavigationKey =
  | 'quick-chat'
  | 'agents'
  | 'contexts'
  | 'run-mode-management'
  | 'scheduled-tasks'
  | null;

export function conversationSidebarNavigationKey(page: ConversationPage): ConversationSidebarNavigationKey {
  switch (page.kind) {
    case 'conversation-home':
    case 'scheduled-task-create':
      return 'quick-chat';
    case 'agents':
    case 'contexts':
    case 'run-mode-management':
    case 'scheduled-tasks':
      return page.kind;
    case 'scheduled-task-detail':
      return 'scheduled-tasks';
    case 'conversation-run':
    case 'settings':
      return null;
  }
}

interface ConversationSidebarProps {
  vm: ConversationSidebarVm;
  active: ConversationPage;
  defaultExpandedWorkspaceId?: string | null;
  workspaceRevealRequest?: ConversationSidebarWorkspaceRevealRequest | null;
  onSelect: (page: ConversationPage) => void;
  onNewConversation: () => void;
  onSearch: () => void;
  onSelectTask: (projectId: string, taskId: string, taskUuid?: string | null) => void;
  onSelectRun: (projectId: string, taskId: string, taskUuid: string | null | undefined, runId: string) => void;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string, taskUuid?: string | null) => void;
  onPauseRun?: (projectId: string, taskId: string, runId: string) => void | Promise<void>;
  onNewConversationInWorkspace?: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (projectId: string) => Promise<void>;
  onRetryBootstrap: () => void;
  onRequestWorkspaceTasks: (projectId: string, cursor?: string | null) => void;
  onRequestPinnedTasks: (cursor?: string | null) => void;
  onRequestTaskRuns: (task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>, cursor?: string | null) => void;
}

export const ConversationSidebar = memo(function ConversationSidebar({
  vm,
  active,
  defaultExpandedWorkspaceId,
  workspaceRevealRequest,
  onSelect,
  onNewConversation,
  onSearch,
  onSelectTask,
  onSelectRun,
  onPinTask,
  onUnpinTask,
  onRenameTask,
  onDeleteTask,
  onPauseRun,
  onNewConversationInWorkspace,
  onAddWorkspace,
  onRemoveWorkspace,
  onRetryBootstrap,
  onRequestWorkspaceTasks,
  onRequestPinnedTasks,
  onRequestTaskRuns,
}: ConversationSidebarProps) {
  const { t } = useTranslation();
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Record<string, boolean>>({});
  const [expandedTaskKeys, setExpandedTaskKeys] = useState<ConversationSidebarExpandedTaskKeys>({ pinned: null, workspace: null });
  const [activeRunListScope, setActiveRunListScope] = useState<ConversationSidebarRunListScope>('workspace');
  const [pinnedCollapsed, setPinnedCollapsed] = useState(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') return pref;
    return false;
  });
  const [collapsedPinnedWorkspaces, setCollapsedPinnedWorkspaces] = useState<Record<string, boolean>>({});
  const [workspaceToRemove, setWorkspaceToRemove] = useState<ConversationWorkspaceVm | null>(null);
  const [workspaceRemovalPending, setWorkspaceRemovalPending] = useState(false);
  const pinnedTasksByWorkspace = useMemo(() => vm.pinnedTasks.reduce<Record<string, ConversationTaskRowVm[]>>((acc, task) => {
    (acc[task.projectId] ??= []).push(task);
    return acc;
  }, {}), [vm.pinnedTasks]);
  const pinnedTaskKeys = useMemo(
    () => new Set(vm.pinnedTasks.map((task) => conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid))),
    [vm.pinnedTasks],
  );
  const workspacesByProjectId = useMemo(
    () => new Map(vm.workspaces.map((workspace) => [workspace.projectId, workspace])),
    [vm.workspaces],
  );
  const activeNavigationKey = conversationSidebarNavigationKey(active);

  // Sync pinned collapse from persisted preferences when sidebar VM reloads
  useEffect(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') setPinnedCollapsed(pref);
  }, [vm.preferences]);

  const workspaceExpansionInitializedRef = useRef(false);
  const handledWorkspaceRevealRequestRef = useRef(workspaceRevealRequest?.requestId ?? null);
  const runListInteractionScopeRef = useRef<ConversationSidebarRunListScope | null>(null);
  const initialWorkspaceId = active.kind === 'conversation-run'
    ? active.projectId
    : (defaultExpandedWorkspaceId ?? vm.lastActiveWorkspaceId ?? null);

  useEffect(() => {
    const initialize = !workspaceExpansionInitializedRef.current && vm.workspaces.length > 0;
    setExpandedWorkspaces((current) => reconcileConversationSidebarExpandedWorkspaces(
      current,
      vm.workspaces.map((workspace) => workspace.projectId),
      initialize ? initialWorkspaceId : null,
    ));
    if (initialize) workspaceExpansionInitializedRef.current = true;
  }, [initialWorkspaceId, vm.workspaces]);

  useEffect(() => {
    if (!workspaceRevealRequest
      || handledWorkspaceRevealRequestRef.current === workspaceRevealRequest.requestId) return;
    handledWorkspaceRevealRequestRef.current = workspaceRevealRequest.requestId;
    setExpandedWorkspaces((current) => current[workspaceRevealRequest.projectId]
      ? current
      : { ...current, [workspaceRevealRequest.projectId]: true });
  }, [workspaceRevealRequest]);

  const togglePinnedCollapsed = () => {
    setPinnedCollapsed((prev) => {
      const next = !prev;
      saveConversationPreference('pinned.collapsed', next).catch(() => {});
      return next;
    });
  };

  const togglePinnedWorkspace = (projectId: string) => {
    setCollapsedPinnedWorkspaces((prev) => ({ ...prev, [projectId]: !prev[projectId] }));
  };

  const activeTaskKey = active.kind === 'conversation-run'
    ? conversationSidebarTaskKey(active.projectId, active.taskId, active.taskUuid)
    : null;
  const activeRunKey = active.kind === 'conversation-run'
    ? conversationSidebarRunKey(active.projectId, active.taskId, active.runId, active.taskUuid)
    : null;

  useEffect(() => {
    if (!activeTaskKey) return;
    const interactionScope = runListInteractionScopeRef.current;
    runListInteractionScopeRef.current = null;
    if (interactionScope === 'pinned') return;
    setExpandedTaskKeys((prev) => prev.workspace === activeTaskKey
      ? prev
      : updateConversationSidebarExpandedTaskKeys(prev, 'workspace', activeTaskKey, 'expand'));
    const task = Object.values(vm.tasksByWorkspace).flat()
      .find((candidate) => conversationSidebarTaskKey(candidate.projectId, candidate.taskId, candidate.taskUuid) === activeTaskKey);
    if (task && (task.runHistoryStatus === 'not-loaded' || task.runHistoryStatus === 'error')) {
      onRequestTaskRuns(task);
    }
  }, [activeTaskKey, onRequestTaskRuns, vm.tasksByWorkspace]);

  const toggleWorkspace = (projectId: string) => {
    setExpandedWorkspaces((prev) => {
      const expanding = !prev[projectId];
      if (expanding) {
        const status = vm.workspaceTaskPages[projectId]?.status ?? 'not-loaded';
        if (status === 'not-loaded' || status === 'error') onRequestWorkspaceTasks(projectId);
      }
      return { ...prev, [projectId]: expanding };
    });
  };

  const markRunListInteraction = (scope: ConversationSidebarRunListScope) => {
    runListInteractionScopeRef.current = scope;
    setActiveRunListScope(scope);
  };

  const toggleTaskRuns = (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => {
    const taskKey = conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid);
    markRunListInteraction(scope);
    if (task.runHistoryStatus === 'not-loaded' || task.runHistoryStatus === 'error') {
      onRequestTaskRuns(task);
    }
    setExpandedTaskKeys((prev) => updateConversationSidebarExpandedTaskKeys(prev, scope, taskKey, 'toggle'));
  };

  const expandTaskRuns = (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => {
    markRunListInteraction(scope);
    if (task.runHistoryStatus === 'not-loaded' || task.runHistoryStatus === 'error') {
      onRequestTaskRuns(task);
    }
    setExpandedTaskKeys((prev) => updateConversationSidebarExpandedTaskKeys(
      prev,
      scope,
      conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid),
      'expand',
    ));
  };

  const confirmWorkspaceRemoval = async (event: React.MouseEvent<HTMLButtonElement>) => {
    event.preventDefault();
    if (!workspaceToRemove || !onRemoveWorkspace || workspaceRemovalPending) return;
    setWorkspaceRemovalPending(true);
    try {
      await onRemoveWorkspace(workspaceToRemove.projectId);
      setWorkspaceToRemove(null);
    } catch {
      // App owns the user-facing error dialog; keep this confirmation open for retry.
    } finally {
      setWorkspaceRemovalPending(false);
    }
  };

  return (
    <TooltipProvider>
      <>
        <aside className="flex min-h-0 h-full flex-col gap-0.5 bg-sidebar px-3 py-2.5 text-sidebar-foreground">
        <div data-conversation-sidebar-region="fixed-navigation" className="shrink-0">
        {/* Quick actions */}
        <div className="flex flex-col gap-0.5">
          <SidebarButton
            active={activeNavigationKey === 'quick-chat'}
            icon={<MessageSquare />}
            label={t('conversation.sidebar.newChat')}
            onClick={onNewConversation}
          />
          <SidebarButton
            icon={<Search />}
            label={t('conversation.sidebar.search')}
            onClick={onSearch}
          />
        </div>

        <Separator className="mx-1 my-1 opacity-45" />

        {/* Navigation */}
        <div className="flex flex-col gap-0.5">
          <SidebarButton
            compact
            active={activeNavigationKey === 'agents'}
            icon={<Bot />}
            label={t('conversation.sidebar.agentManagement')}
            onClick={() => onSelect({ kind: 'agents' })}
          />
          <SidebarButton
            compact
            active={activeNavigationKey === 'contexts'}
            icon={<Library />}
            label={t('conversation.sidebar.contextManagement')}
            onClick={() => onSelect({ kind: 'contexts' })}
          />
          <SidebarButton
            compact
            active={activeNavigationKey === 'run-mode-management'}
            icon={<Route />}
            label={t('conversation.sidebar.runModeManagement')}
            onClick={() => onSelect({ kind: 'run-mode-management' })}
          />
          <SidebarButton
            compact
            active={activeNavigationKey === 'scheduled-tasks'}
            icon={<AlarmClock />}
            label={t('scheduled.management.title')}
            onClick={() => onSelect({ kind: 'scheduled-tasks' })}
          />
        </div>
        </div>

        {/* Pinned and workspace sections share the conversation scroll region. */}
        <ScrollArea
          data-conversation-sidebar-region="scrollable-conversations"
          className="min-h-0 flex-1"
        >
        {vm.pinRefs.length > 0 || vm.pinnedTasks.length > 0 ? (
          <div className="my-1.5 border-y border-border/55 py-2">
            <button
              type="button"
              data-conversation-sidebar-heading="pinned"
              className="sticky top-0 z-[1] flex w-full items-center gap-1.5 bg-sidebar px-1 py-1 text-left text-sm font-medium text-sidebar-foreground hover:text-sidebar-accent-foreground"
              onClick={togglePinnedCollapsed}
            >
              <ChevronDown className={cn('size-3 transition-transform', pinnedCollapsed && '-rotate-90')} />
              {t('conversation.sidebar.pinned')}
            </button>
            {!pinnedCollapsed ? (
              <div className="mt-1 space-y-3">
                {Object.entries(pinnedTasksByWorkspace).map(([projectId, tasks]) => {
                  const ws = workspacesByProjectId.get(projectId);
                  const isWsCollapsed = collapsedPinnedWorkspaces[projectId] ?? false;
                  return (
                    <div key={`pinned-ws-${projectId}`}>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <button
                            type="button"
                            data-conversation-pinned-workspace-id={projectId}
                            className="flex w-full items-center gap-1.5 px-1 py-1 text-left text-sm font-semibold leading-5 text-sidebar-foreground/80 hover:text-sidebar-accent-foreground"
                            onClick={() => togglePinnedWorkspace(projectId)}
                          >
                            <ChevronDown className={cn('size-3 shrink-0 transition-transform', isWsCollapsed && '-rotate-90')} />
                            <span className="truncate">{ws?.name ?? projectId}</span>
                          </button>
                        </TooltipTrigger>
                        <TooltipContent side="right" className="max-w-[min(36rem,calc(100vw-2rem))] break-all">
                          {ws?.workspacePath ?? projectId}
                        </TooltipContent>
                      </Tooltip>
                      {!isWsCollapsed ? (
                        <div className="space-y-0.5">
                          {tasks.map((task) => (
                            <TaskRow
                              key={`pinned-${conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}`}
                              task={task}
                              pinned
                              isActive={isConversationSidebarRunListScopeActive('pinned', activeRunListScope) && activeTaskKey === conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                              activeRunKey={isConversationSidebarRunListScopeActive('pinned', activeRunListScope) ? activeRunKey : null}
                              expanded={expandedTaskKeys.pinned === conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                              onSelect={() => onSelectTask(task.projectId, task.taskId, task.taskUuid)}
                              onSelectRun={(runId) => {
                                markRunListInteraction('pinned');
                                onSelectRun(task.projectId, task.taskId, task.taskUuid, runId);
                              }}
                              onToggleRuns={() => toggleTaskRuns('pinned', task)}
                              onExpandRuns={() => expandTaskRuns('pinned', task)}
                              onLoadMoreRuns={() => onRequestTaskRuns(task, task.runsNextCursor)}
                              onUnpin={() => onUnpinTask(task.projectId, task.taskId)}
                              onRename={(title) => onRenameTask(task.projectId, task.taskId, title)}
                              onDelete={() => onDeleteTask(task.projectId, task.taskId, task.taskUuid)}
                              onPauseRun={(runId) => onPauseRun?.(task.projectId, task.taskId, runId)}
                              t={t}
                            />
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
                {vm.pinnedTaskPage.status === 'loading' ? (
                  <div className="flex items-center gap-2 px-2 py-1 text-xs text-muted-foreground">
                    <Loader2 className="size-3 animate-spin" />
                    {t('conversation.sidebar.loadingPinned')}
                  </div>
                ) : null}
                {vm.pinnedTaskPage.status === 'error' ? (
                  <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={() => onRequestPinnedTasks()}>
                    {t('conversation.sidebar.retryPinned')}
                  </Button>
                ) : null}
                {vm.pinnedTaskPage.status === 'ready' && vm.pinnedTaskPage.nextCursor ? (
                  <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={() => onRequestPinnedTasks(vm.pinnedTaskPage.nextCursor)}>
                    {t('conversation.sidebar.loadMorePinned')}
                  </Button>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : (
          <Separator className="mx-1 my-1.5 opacity-45" />
        )}

        {/* Workspace sections — scrollable with sticky headers */}
          <div className="pt-2">
            {vm.loadStatus === 'not-loaded' || vm.loadStatus === 'loading' ? (
              <div className="flex items-center gap-2 px-3 py-3 text-xs text-muted-foreground">
                <Loader2 className="size-3.5 animate-spin" />
                {t('conversation.sidebar.loadingWorkspaces')}
              </div>
            ) : null}
            {vm.loadStatus === 'error' ? (
              <Button variant="ghost" size="sm" className="h-8 w-full justify-start text-xs text-muted-foreground" onClick={onRetryBootstrap}>
                {t('conversation.sidebar.retryWorkspaces')}
              </Button>
            ) : null}
            {vm.workspaces.map((ws) => (
              <div
                key={ws.projectId}
                data-conversation-workspace-group={ws.projectId}
                className="mb-2"
              >
                <div className="group sticky top-0 z-[1] flex w-full items-center gap-1.5 bg-sidebar px-1 py-1">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        data-conversation-workspace-id={ws.projectId}
                        aria-expanded={Boolean(expandedWorkspaces[ws.projectId])}
                        className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-sm font-semibold leading-5 text-sidebar-foreground/80 hover:text-sidebar-accent-foreground group-hover:pr-11"
                        onClick={() => toggleWorkspace(ws.projectId)}
                      >
                        <ChevronDown className={cn('size-3 shrink-0 transition-transform', !expandedWorkspaces[ws.projectId] && '-rotate-90')} />
                        <span className="truncate">{ws.name}</span>
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="right" className="max-w-[min(36rem,calc(100vw-2rem))] break-all">
                      {ws.workspacePath}
                    </TooltipContent>
                  </Tooltip>
                  <span className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100">
                    {onNewConversationInWorkspace ? (
                      <Button variant="ghost" size="icon" className="size-5 active:scale-90 transition-transform" onClick={(e) => { e.stopPropagation(); onNewConversationInWorkspace(ws.projectId); }}>
                        <Plus className="size-3" />
                      </Button>
                    ) : null}
                    {onRemoveWorkspace ? (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-5 text-muted-foreground transition-transform hover:text-destructive active:scale-90"
                        aria-label={t('conversation.sidebar.removeWorkspaceNamed', { name: ws.name })}
                        onClick={(event) => {
                          event.stopPropagation();
                          setWorkspaceRemovalPending(false);
                          setWorkspaceToRemove(ws);
                        }}
                      >
                        <Trash2 className="size-3" />
                      </Button>
                    ) : null}
                  </span>
                </div>
                {expandedWorkspaces[ws.projectId] ? (
                  <div className="space-y-0.5">
                    {(vm.tasksByWorkspace[ws.projectId] ?? []).map((task) => (
                      <TaskRow
                        key={conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                        task={task}
                        pinned={pinnedTaskKeys.has(conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid))}
                        isActive={isConversationSidebarRunListScopeActive('workspace', activeRunListScope) && activeTaskKey === conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                        activeRunKey={isConversationSidebarRunListScopeActive('workspace', activeRunListScope) ? activeRunKey : null}
                        expanded={expandedTaskKeys.workspace === conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                        onSelect={() => onSelectTask(task.projectId, task.taskId, task.taskUuid)}
                        onSelectRun={(runId) => {
                          markRunListInteraction('workspace');
                          onSelectRun(task.projectId, task.taskId, task.taskUuid, runId);
                        }}
                        onToggleRuns={() => toggleTaskRuns('workspace', task)}
                        onExpandRuns={() => expandTaskRuns('workspace', task)}
                        onLoadMoreRuns={() => onRequestTaskRuns(task, task.runsNextCursor)}
                        onPin={() => onPinTask(task.projectId, task.taskId)}
                        onUnpin={() => onUnpinTask(task.projectId, task.taskId)}
                        onRename={(title) => onRenameTask(task.projectId, task.taskId, title)}
                        onDelete={() => onDeleteTask(task.projectId, task.taskId, task.taskUuid)}
                        onPauseRun={(runId) => onPauseRun?.(task.projectId, task.taskId, runId)}
                        t={t}
                      />
                    ))}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'loading' ? (
                      <div className="flex items-center gap-2 px-3 py-2 text-xs text-muted-foreground">
                        <Loader2 className="size-3 animate-spin" />
                        {t('conversation.sidebar.loadingConversations')}
                      </div>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'error' ? (
                      <Button variant="ghost" size="sm" className="h-7 w-full justify-start px-3 text-xs text-muted-foreground" onClick={() => onRequestWorkspaceTasks(ws.projectId)}>
                        {t('conversation.sidebar.retryConversations')}
                      </Button>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'ready-empty' ? (
                      <div className="px-3 py-2 text-xs text-muted-foreground">{t('conversation.noConversations')}</div>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'ready' && vm.workspaceTaskPages[ws.projectId]?.nextCursor ? (
                      <Button variant="ghost" size="sm" className="h-7 w-full justify-start px-3 text-xs text-muted-foreground" onClick={() => onRequestWorkspaceTasks(ws.projectId, vm.workspaceTaskPages[ws.projectId]?.nextCursor)}>
                        {t('conversation.sidebar.loadMoreConversations')}
                      </Button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ))}

            {/* Add workspace button */}
            {onAddWorkspace ? (
              <button
                type="button"
                className="mt-1.5 flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                onClick={onAddWorkspace}
              >
                <Plus className="size-3.5" />
                <span>{t('conversation.sidebar.addWorkspace')}</span>
              </button>
            ) : null}

            {vm.loadStatus === 'ready-empty' ? (
              <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                {t('conversation.sidebar.noWorkspaces')}
              </div>
            ) : null}
          </div>
        </ScrollArea>


        {/* Settings */}
        <Separator className="mx-1 my-1.5 opacity-45" />
        <SidebarButton icon={<Settings />} label={t('conversation.sidebar.settings')} onClick={() => onSelect({ kind: 'settings' })} />
        </aside>

        <AlertDialog
          open={workspaceToRemove != null}
          onOpenChange={(open) => {
            if (!open && !workspaceRemovalPending) setWorkspaceToRemove(null);
          }}
        >
          <AlertDialogContent className="max-w-[480px] gap-5 rounded-3xl border-border/60 p-6 shadow-2xl">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="absolute right-4 top-4 size-7 rounded-full text-muted-foreground hover:text-foreground"
              aria-label={t('common.close')}
              disabled={workspaceRemovalPending}
              onClick={() => setWorkspaceToRemove(null)}
            >
              <X className="size-4" />
            </Button>
            <AlertDialogHeader className="gap-1.5 pr-8">
              <AlertDialogTitle className="text-lg font-semibold tracking-tight">
                {t('conversation.sidebar.removeWorkspaceTitle', { name: workspaceToRemove?.name ?? '' })}
              </AlertDialogTitle>
              <AlertDialogDescription className="text-sm leading-6">
                {t('conversation.sidebar.removeWorkspaceDescription')}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter className="gap-1.5">
              <AlertDialogCancel variant="ghost" size="sm" disabled={workspaceRemovalPending}>
                {t('conversation.sidebar.removeWorkspaceCancel')}
              </AlertDialogCancel>
              <AlertDialogAction
                variant="ghost"
                size="sm"
                className="bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
                disabled={workspaceRemovalPending}
                onClick={confirmWorkspaceRemoval}
              >
                {workspaceRemovalPending ? <Loader2 className="size-4 animate-spin" /> : null}
                {t(workspaceRemovalPending
                  ? 'conversation.sidebar.removingWorkspace'
                  : 'conversation.sidebar.removeWorkspaceConfirm')}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </>
    </TooltipProvider>
  );
});

// ── Task Row ──

export function conversationSidebarRunStatusClass(run: ConversationTaskRowVm['runs'][0]) {
  if (run.outcome === 'success') return 'bg-emerald-500/50';
  if (run.outcome === 'failure' || run.outcome === 'killed') return 'bg-red-500/50';
  if (run.status === 'running') return 'bg-gold-running motion-safe:animate-pulse';
  return 'bg-yellow-500/50';
}

export function canPauseConversationSidebarRun(run: { status: string }) {
  return run.status === 'running';
}

export function selectConversationSidebarRunPauseAction(
  run: { runId: string; status: string },
  onPauseRun?: (runId: string) => void | Promise<void>,
) {
  if (!canPauseConversationSidebarRun(run)) return false;
  onPauseRun?.(run.runId);
  return true;
}

export function canOpenConversationSidebarRunMenu(scope: 'task' | 'run') {
  return scope === 'run';
}

export function shouldShowConversationSidebarRunList(
  task: Pick<ConversationTaskRowVm, 'runMode' | 'runs' | 'latestRun'>,
) {
  return task.runMode !== 'direct' && Boolean(task.latestRun || task.runs.length >= 1);
}

export function conversationSidebarIdentityKind(task: Pick<ConversationTaskRowVm, 'runMode' | 'agentIdentity'>) {
  return task.runMode === 'direct' && task.agentIdentity ? 'agent-icon' : 'runtime-status';
}

export function shouldShowConversationSidebarActivity(
  task: Pick<ConversationTaskRowVm, 'runMode' | 'agentIdentity' | 'activity'>,
) {
  return task.runMode === 'direct' && Boolean(task.agentIdentity && task.activity);
}

export type ConversationSidebarRunListScope = 'pinned' | 'workspace';

export type ConversationSidebarExpandedTaskKeys = Record<ConversationSidebarRunListScope, string | null>;

export function updateConversationSidebarExpandedTaskKeys(
  current: ConversationSidebarExpandedTaskKeys,
  scope: ConversationSidebarRunListScope,
  taskKey: string,
  mode: 'expand' | 'toggle',
): ConversationSidebarExpandedTaskKeys {
  const nextKey = mode === 'toggle' && current[scope] === taskKey ? null : taskKey;
  return { ...current, [scope]: nextKey };
}

export function isConversationSidebarRunListScopeActive(
  scope: ConversationSidebarRunListScope,
  activeScope: ConversationSidebarRunListScope,
) {
  return scope === activeScope;
}

function RunStopMenu({
  run,
  open,
  children,
  onOpenChange,
  onPauseRun,
  t,
}: {
  run: { runId: string; status: string };
  open: boolean;
  children: React.ReactNode;
  onOpenChange: (open: boolean) => void;
  onPauseRun?: (runId: string) => void | Promise<void>;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const canPauseRun = canPauseConversationSidebarRun(run);
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange}>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent align="start" onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
      }}>
        <DropdownMenuItem
          variant="destructive"
          disabled={!canPauseRun}
          onSelect={() => {
            if (!canPauseRun) return;
            onOpenChange(false);
            void onPauseRun?.(run.runId);
          }}
        >
          {t('common.stopRun')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function TaskRow({
  task,
  pinned,
  isActive,
  activeRunKey,
  expanded,
  onSelect,
  onSelectRun,
  onToggleRuns,
  onExpandRuns,
  onLoadMoreRuns,
  onPin,
  onUnpin,
  onRename,
  onDelete,
  onPauseRun,
  t,
}: {
  task: ConversationTaskRowVm;
  pinned: boolean;
  isActive: boolean;
  activeRunKey?: string | null;
  expanded: boolean;
  onSelect: () => void;
  onSelectRun?: (runId: string) => void;
  onToggleRuns: () => void;
  onExpandRuns: () => void;
  onLoadMoreRuns: () => void;
  onPin?: () => void;
  onUnpin?: () => void;
  onRename?: (title: string) => void;
  onDelete?: () => void;
  onPauseRun?: (runId: string) => void | Promise<void>;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const [editing, setEditing] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [openRunMenuId, setOpenRunMenuId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState(task.title);
  const editInputRef = useRef<HTMLInputElement>(null);
  const hasRuns = shouldShowConversationSidebarRunList(task);

  const latestRun = task.latestRun;
  const isDirect = task.runMode === 'direct';
  const useAgentIdentity = conversationSidebarIdentityKind(task) === 'agent-icon';
  const showActivity = shouldShowConversationSidebarActivity(task);
  const unreadTerminalResult = isDirect ? task.unreadTerminalResult ?? null : null;
  const unreadTerminalResultLabel = unreadTerminalResult
    ? t(`conversation.sidebar.terminalResult.${unreadTerminalResult.kind}`)
    : null;
  const latestColor = latestRun ? conversationSidebarRunStatusClass(latestRun) : 'bg-muted-foreground/30';
  const relativeTimeSource = task.lastActivityAt;
  const relativeTime = relativeTimeSource && (isDirect || latestRun?.status !== 'running')
    ? formatCompactRelativeTime(relativeTimeSource, t('conversation.runtime.justNow'))
    : null;

  const handleRowClick = () => {
    if (hasRuns) {
      if (isActive) {
        // Already viewing a run of this task — just toggle expand, don't re-navigate
        onToggleRuns();
        return;
      }
      onExpandRuns();
    }
    onSelect();
  };

  const startRename = (e: React.MouseEvent) => {
    e.stopPropagation();
    setEditValue(task.title);
    setEditing(true);
    requestAnimationFrame(() => editInputRef.current?.select());
  };

  const commitRename = () => {
    setEditing(false);
    const trimmed = editValue.trim();
    if (trimmed && trimmed !== task.title) {
      onRename?.(trimmed);
    }
  };

  const handleRenameKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
    if (e.key === 'Escape') { setEditValue(task.title); setEditing(false); }
  };

  const openDeleteDialog = (e: React.MouseEvent) => {
    e.stopPropagation();
    setDeleteOpen(true);
  };

  const confirmDelete = () => {
    setDeleteOpen(false);
    onDelete?.();
  };

  const taskRow = (
    <div
      className={cn(
        'group relative flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 text-sidebar-foreground cursor-pointer',
        isActive ? 'bg-sidebar-accent/70 font-medium text-sidebar-accent-foreground' : 'hover:bg-sidebar-accent',
      )}
      onClick={handleRowClick}
    >
      <span className="flex size-4 shrink-0 items-center justify-center">
        {useAgentIdentity && task.agentIdentity ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                className="relative flex size-4 items-center justify-center"
                data-conversation-activity={showActivity ? task.activity?.phase : undefined}
                data-conversation-terminal-result={unreadTerminalResult?.kind}
                aria-label={unreadTerminalResultLabel
                  ? `${task.agentIdentity.displayName} · ${unreadTerminalResultLabel}`
                  : task.agentIdentity.displayName}
              >
                <img
                  src={agentIconSrc(task.agentIdentity.iconKey)}
                  alt=""
                  className={agentIconClass(task.agentIdentity.iconKey, cn('size-3', showActivity && conversationSidebarActivityIconClass))}
                />
                {unreadTerminalResult ? (
                  <span
                    aria-hidden="true"
                    className={cn(
                      'absolute -right-0.5 -top-0.5 size-2 rounded-full ring-1 ring-sidebar',
                      conversationSidebarTerminalResultDotClass(unreadTerminalResult.kind),
                    )}
                  />
                ) : null}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              {unreadTerminalResultLabel
                ? `${task.agentIdentity.displayName} · ${unreadTerminalResultLabel}`
                : task.agentIdentity.displayName}
            </TooltipContent>
          </Tooltip>
        ) : (
          <span className={cn('size-1.5 rounded-full', latestColor)} />
        )}
      </span>
      <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden group-hover:pr-20">
        {editing ? (
          <input
            ref={editInputRef}
            className="min-w-0 flex-1 rounded border border-primary/40 bg-background px-1 py-0 text-sm outline-none"
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onBlur={commitRename}
            onKeyDown={handleRenameKeyDown}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span className="flex min-w-0 flex-1 items-center gap-1.5 truncate text-sm">
            {task.scheduledTaskId ? <AlarmClock className="size-3 shrink-0 text-foreground" aria-label={t('scheduled.conversationMarker')} /> : null}
            <span className="truncate">{task.title}</span>
          </span>
        )}
        {relativeTime ? (
          <span className="shrink-0 text-ui-caption font-normal leading-4 tabular-nums text-muted-foreground/55">{relativeTime}</span>
        ) : null}
      </div>
      <span className="pointer-events-none absolute right-2 top-1/2 hidden -translate-y-1/2 items-center gap-1 group-hover:flex group-hover:pointer-events-auto">
        {onRename ? (
          <Button variant="ghost" size="icon" className="size-5 shrink-0" onClick={startRename}>
            <Pencil className="size-3" />
          </Button>
        ) : null}
        {pinned && onUnpin ? (
          <Button variant="ghost" size="icon" className="size-5 shrink-0" onClick={(e) => { e.stopPropagation(); onUnpin(); }}>
            <PinOff className="size-3" />
          </Button>
        ) : onPin ? (
          <Button variant="ghost" size="icon" className="size-5 shrink-0" onClick={(e) => { e.stopPropagation(); onPin(); }}>
            <Pin className="size-3" />
          </Button>
        ) : null}
        {onDelete ? (
          <Button variant="ghost" size="icon" className="size-5 shrink-0 text-muted-foreground hover:text-destructive" onClick={openDeleteDialog}>
            <Trash2 className="size-3" />
          </Button>
        ) : null}
      </span>
    </div>
  );

  return (
    <>
    <div className={cn(expanded && hasRuns && 'space-y-1')}>
      {taskRow}
      {expanded && hasRuns ? (
        <div className="ml-4 mt-1 space-y-1 border-l border-border/60 pl-3">
          {task.runs.map((run) => {
            const color = conversationSidebarRunStatusClass(run);
            const runTime = run.status !== 'running'
              ? formatCompactRelativeTime(run.updatedAt, t('conversation.runtime.justNow'))
              : null;
            return (
              <RunStopMenu
                key={run.runId}
                run={run}
                open={openRunMenuId === run.runId}
                onOpenChange={(open) => setOpenRunMenuId(open ? run.runId : null)}
                onPauseRun={onPauseRun}
                t={t}
              >
                <div
                  className={cn(
                    'flex items-center gap-2 rounded-md px-2 py-1 cursor-pointer text-xs leading-4',
                    isConversationSidebarRunActive(activeRunKey, task.projectId, task.taskId, run.runId, task.taskUuid)
                      ? 'bg-sidebar-accent text-sidebar-accent-foreground'
                      : 'hover:bg-sidebar-accent',
                  )}
                  onPointerDown={(event) => event.preventDefault()}
                  onClick={() => onSelectRun?.(run.runId)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    if (canOpenConversationSidebarRunMenu('run')) {
                      setOpenRunMenuId(run.runId);
                    }
                  }}
                >
                  <span className={cn('size-1.5 shrink-0 rounded-full', color)} />
                  <span className="min-w-0 flex-1 truncate text-muted-foreground/75">{run.runId}</span>
                  {runTime ? (
                    <span className="shrink-0 tabular-nums text-muted-foreground/55">{runTime}</span>
                  ) : null}
                </div>
              </RunStopMenu>
            );
          })}
          {task.runHistoryStatus === 'loading' ? (
            <div className="flex items-center gap-2 px-2 py-1 text-xs text-muted-foreground">
              <Loader2 className="size-3 animate-spin" />
              {t('conversation.sidebar.loadingRuns')}
            </div>
          ) : null}
          {task.runHistoryStatus === 'error' ? (
            <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={onExpandRuns}>
              {t('conversation.sidebar.retryRuns')}
            </Button>
          ) : null}
          {task.runHistoryStatus === 'ready' && task.runsNextCursor ? (
            <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={onLoadMoreRuns}>
              {t('conversation.sidebar.loadMoreRuns')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
    <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('conversation.sidebar.deleteConfirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('conversation.sidebar.deleteConfirmDescription', { title: task.title })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
          <AlertDialogAction className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={confirmDelete}>
            {t('conversation.sidebar.deleteConfirmAction')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
    </>
  );
}

// ── Sidebar Button ──

function SidebarButton({
  active,
  compact,
  icon,
  label,
  onClick,
}: {
  active?: boolean;
  compact?: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button
      variant="ghost"
      className={cn(
        compact ? 'h-6.5 gap-2 justify-start rounded-md px-2 text-sm text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground'
          : 'h-6.5 justify-start gap-2.5 rounded-lg px-2.5 text-sm text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
        active && 'bg-sidebar-accent text-sidebar-accent-foreground',
      )}
      onClick={onClick}
    >
      <span className={cn(compact ? '[&_svg]:size-3.5' : '[&_svg]:size-4')}>{icon}</span>
      <span>{label}</span>
    </Button>
  );
}

export function prioritizeConversationSidebarWorkspace(sidebar: ConversationSidebarVm, projectId?: string | null): ConversationSidebarVm {
  if (!projectId) return sidebar;
  const workspaceIndex = sidebar.workspaces.findIndex((workspace) => workspace.projectId === projectId);
  if (workspaceIndex < 0) return sidebar;
  const workspaces = [
    sidebar.workspaces[workspaceIndex],
    ...sidebar.workspaces.slice(0, workspaceIndex),
    ...sidebar.workspaces.slice(workspaceIndex + 1),
  ];
  return { ...sidebar, workspaces, lastActiveWorkspaceId: projectId };
}

export interface ConversationSidebarWorkspaceRevealRequest {
  projectId: string;
  requestId: number;
}

export function reconcileConversationSidebarExpandedWorkspaces(
  current: Record<string, boolean>,
  projectIds: string[],
  initialWorkspaceId: string | null,
): Record<string, boolean> {
  const next: Record<string, boolean> = {};
  let changed = Object.keys(current).length !== projectIds.length;
  for (const projectId of projectIds) {
    const expanded = Object.prototype.hasOwnProperty.call(current, projectId)
      ? current[projectId]
      : initialWorkspaceId == null || projectId === initialWorkspaceId;
    next[projectId] = expanded;
    if (current[projectId] !== expanded) changed = true;
  }
  return changed ? next : current;
}

export function conversationSidebarTaskKey(projectId: string, taskId: string, taskUuid?: string | null) {
  return conversationTaskIdentityKey({ projectId, taskId, taskUuid })
    ?? JSON.stringify(['invalid-task-identity', projectId, taskId]);
}

export function conversationSidebarRunKey(
  projectId: string,
  taskId: string,
  runId: string,
  taskUuid?: string | null,
) {
  return conversationRunIdentityKey({ projectId, taskId, taskUuid, runId })
    ?? JSON.stringify(['invalid-run-identity', projectId, taskId, runId]);
}

export function isConversationSidebarRunActive(
  activeRunKey: string | null | undefined,
  projectId: string,
  taskId: string,
  runId: string,
  taskUuid?: string | null,
) {
  return activeRunKey === conversationSidebarRunKey(projectId, taskId, runId, taskUuid);
}
