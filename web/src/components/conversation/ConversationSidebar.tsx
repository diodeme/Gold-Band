import { ContextMenu, ContextMenuTrigger, ContextMenuContent, ContextMenuItem } from "../ui/context-menu";
import { Pin, PinOff, MessageSquare, Search, Bot, Library, Route, AlarmClock, Globe, Settings, BookOpen, ChevronDown, Ellipsis, Loader2, Pencil, Plus, Trash2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useStableCallbacks } from '@/hooks/use-stable-callbacks';
import type { ConversationPage, ConversationSidebarVm, ConversationTaskRowVm, ConversationWorkspaceVm } from '../../types';
import { saveConversationPreference } from '../../api';
import { CONVERSATION_TASK_PAGE_SIZE } from '@/lib/conversation-sidebar-loading';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { conversationRunIdentityKey, conversationTaskIdentityKey } from '@/lib/conversation-run-identity';
import { formatCompactRelativeTime } from '@/lib/datetime';
import { useConversationTaskActive } from '@/lib/conversation-sidebar-activity';
import { AcpProcessingSpinner } from '@/components/acp/AcpProcessingSpinner';
import { ResidentPin } from '@/components/conversation/ResidentPin';

export const conversationSidebarActivityIconClass = 'animate-pulse';

// Manual retry/load-more buttons already render the section's error state, so
// manual requests swallow rejections instead of surfacing unhandled ones.
function requestSidebarPage(request: () => unknown) {
  Promise.resolve(request()).catch(() => {});
}

export function conversationSidebarTerminalResultDotClass(
  kind: NonNullable<ConversationTaskRowVm['unreadTerminalResult']>['kind'],
) {
  if (kind === 'completed') return 'bg-gold-success';
  if (kind === 'new-message') return 'bg-gold-attention';
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
    case 'personal-analytics':
    case 'settings':
    case 'remote-tasks':
      return null;
  }
}

export function isConversationSidebarMoreNavigationActive(page: ConversationPage): boolean {
  return page.kind === 'remote-tasks'
    || page.kind === 'scheduled-tasks'
    || page.kind === 'scheduled-task-detail';
}

interface ConversationSidebarProps {
  vm: ConversationSidebarVm;
  active: ConversationPage;
  showRemoteTaskManagement?: boolean;
  defaultExpandedWorkspaceId?: string | null;
  workspaceRevealRequest?: ConversationSidebarWorkspaceRevealRequest | null;
  onRetryReveal?: () => void;
  onRevealHandled?: (requestId: number) => void;
  onSelect: (page: ConversationPage) => void;
  onNewConversation: () => void;
  onSearch: () => void;
  onSetResident?: (projectId: string, taskId: string, resident: boolean) => Promise<void>;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string, taskUuid?: string | null) => void;
  onPauseRun?: (projectId: string, taskId: string, runId: string) => void | Promise<void>;
  onNewConversationInWorkspace?: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onOpenProjectMemory?: (workspace: { projectId: string; name: string }) => void;
  onRemoveWorkspace?: (projectId: string) => Promise<void>;
  onRetryBootstrap: () => void;
  /** Collapsing a list resets it to its default first page so reopening shows
   *  the default page size again; the next expand re-requests page one. */
  onResetWorkspaceTaskPage?: (projectId: string) => void;
  onResetRecentTaskPage?: () => void;
  onRequestWorkspaceTasks: (projectId: string, cursor?: string | null) => void | Promise<unknown>;
  onRequestPinnedTasks: (cursor?: string | null) => void;
  onRequestRecentTasks: (cursor?: string | null) => void | Promise<unknown>;
  onRequestTaskRuns: (task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>, cursor?: string | null) => void;
}

export const ConversationSidebar = memo(function ConversationSidebar({
  vm,
  active,
  showRemoteTaskManagement = true,
  defaultExpandedWorkspaceId,
  workspaceRevealRequest,
  onRetryReveal,
  onRevealHandled,
  onSelect,
  onNewConversation,
  onSearch,
  onSetResident,
  onPinTask,
  onUnpinTask,
  onRenameTask,
  onDeleteTask,
  onPauseRun,
  onNewConversationInWorkspace,
  onAddWorkspace,
  onOpenProjectMemory,
  onRemoveWorkspace,
  onRetryBootstrap,
  onResetWorkspaceTaskPage,
  onResetRecentTaskPage,
  onRequestWorkspaceTasks,
  onRequestPinnedTasks,
  onRequestRecentTasks,
  onRequestTaskRuns,
}: ConversationSidebarProps) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Record<string, boolean>>({});
  const [expandedTaskKeys, setExpandedTaskKeys] = useState<ConversationSidebarExpandedTaskKeys>({ pinned: null, workspace: null, recent: null });
  const [activeRunListScope, setActiveRunListScope] = useState<ConversationSidebarRunListScope>('workspace');
  const [recentCollapsed, setRecentCollapsed] = useState(false);
  const [pinnedCollapsed, setPinnedCollapsed] = useState(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') return pref;
    return false;
  });
  // The workspaces group collapses by default (2026-11-05 IA: first level is
  // groups — pinned, workspaces, recent — and the group header owns the
  // add-workspace entry so it stays at the first level).
  const [workspacesCollapsed, setWorkspacesCollapsed] = useState(() => {
    const pref = vm.preferences?.['workspaces.collapsed'];
    if (typeof pref === 'boolean') return pref;
    return true;
  });
  const [collapsedPinnedWorkspaces, setCollapsedPinnedWorkspaces] = useState<Record<string, boolean>>({});
  const [workspaceToRemove, setWorkspaceToRemove] = useState<ConversationWorkspaceVm | null>(null);
  const [workspaceRemovalPending, setWorkspaceRemovalPending] = useState(false);
  const moreNavigationActive = isConversationSidebarMoreNavigationActive(active)
    && (active.kind !== 'remote-tasks' || showRemoteTaskManagement);
  const [moreNavigationOpen, setMoreNavigationOpen] = useState(moreNavigationActive);
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

  useEffect(() => {
    if (moreNavigationActive) setMoreNavigationOpen(true);
  }, [moreNavigationActive]);

  // Sync pinned collapse from persisted preferences when sidebar VM reloads
  useEffect(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') setPinnedCollapsed(pref);
  }, [vm.preferences]);

  // Sync workspaces group collapse from persisted preferences when the sidebar
  // VM reloads. This component is the only writer of the preference, so an
  // echo of our own save (bootstrap re-applied with a fresh preferences
  // object) must not override a transient reveal expansion: only a value that
  // differs from what we last saved counts as an external change.
  const lastSavedWorkspacesCollapsedRef = useRef<boolean | null>(null);
  useEffect(() => {
    const pref = vm.preferences?.['workspaces.collapsed'];
    if (typeof pref !== 'boolean') return;
    if (pref === lastSavedWorkspacesCollapsedRef.current) return;
    lastSavedWorkspacesCollapsedRef.current = pref;
    setWorkspacesCollapsed(pref);
  }, [vm.preferences]);

  const workspaceExpansionInitializedRef = useRef(false);
  const handledWorkspaceRevealRequestRef = useRef<number | null>(null);
  const pendingTaskRevealRef = useRef<number | null>(null);
  const conversationViewportRef = useRef<HTMLDivElement>(null);
  const runListInteractionRef = useRef<{ scope: ConversationSidebarRunListScope; taskKey: string } | null>(null);
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

  useLayoutEffect(() => {
    if (!workspaceRevealRequest
      || handledWorkspaceRevealRequestRef.current === workspaceRevealRequest.requestId) return;
    handledWorkspaceRevealRequestRef.current = workspaceRevealRequest.requestId;
    pendingTaskRevealRef.current = workspaceRevealRequest.taskId && !workspaceRevealRequest.scrollHandled ? workspaceRevealRequest.requestId : null;
    if (workspaceRevealRequest.taskId) setActiveRunListScope('workspace');
    // A reveal is an explicit navigation intent: expand the workspaces group
    // for this turn without overwriting the persisted collapse preference.
    setWorkspacesCollapsed(false);
    setExpandedWorkspaces((current) => current[workspaceRevealRequest.projectId]
      ? current
      : { ...current, [workspaceRevealRequest.projectId]: true });
  }, [workspaceRevealRequest]);

  useLayoutEffect(() => {
    const request = workspaceRevealRequest;
    const viewport = conversationViewportRef.current;
    if (!request?.taskId || !viewport || pendingTaskRevealRef.current !== request.requestId
      || workspacesCollapsed || !expandedWorkspaces[request.projectId] || active.kind !== 'conversation-run'
      || active.projectId !== request.projectId || active.taskId !== request.taskId) return;
    const row = [...viewport.querySelectorAll<HTMLElement>('[data-conversation-task-row="workspace"]')]
      .find((element) => element.dataset.projectId === request.projectId && element.dataset.taskId === request.taskId
        && (!request.taskUuid || !element.dataset.taskUuid || element.dataset.taskUuid === request.taskUuid));
    if (!row) return;
    const bounds = viewport.getBoundingClientRect();
    const rowBounds = row.getBoundingClientRect();
    const header = row.closest('[data-conversation-workspace-group]')?.querySelector('[data-conversation-workspace-header]');
    const visibleTop = bounds.top + (header?.getBoundingClientRect().height ?? 0);
    if (rowBounds.top < visibleTop) viewport.scrollTop += rowBounds.top - visibleTop;
    else if (rowBounds.bottom > bounds.bottom) viewport.scrollTop += rowBounds.bottom - bounds.bottom;
    pendingTaskRevealRef.current = null;
    onRevealHandled?.(request.requestId);
    // workspacesCollapsed is a dependency because the reveal first expands the
    // group in a sibling effect: this scroll pass then runs against a DOM
    // without the target row and must re-run once the group opens.
  }, [workspaceRevealRequest, workspacesCollapsed, expandedWorkspaces, vm.tasksByWorkspace, active, onRevealHandled]);

  const cancelPendingTaskReveal = () => {
    const requestId = pendingTaskRevealRef.current;
    pendingTaskRevealRef.current = null;
    if (requestId != null) onRevealHandled?.(requestId);
  };

  const togglePinnedCollapsed = () => {
    setPinnedCollapsed((prev) => {
      const next = !prev;
      saveConversationPreference('pinned.collapsed', next).catch(() => {});
      return next;
    });
  };

  const toggleWorkspacesCollapsed = () => {
    const next = !workspacesCollapsed;
    setWorkspacesCollapsed(next);
    lastSavedWorkspacesCollapsedRef.current = next;
    saveConversationPreference('workspaces.collapsed', next).catch(() => {});
    if (next) {
      // Collapsing the group folds every expanded workspace away: reset their
      // task pages so reopening shows the default first page instead of
      // everything the user paged through last time, and drop the section's
      // expanded-run key so a remembered task cannot come back expanded with
      // an empty (not-loaded) run list.
      setExpandedTaskKeys((prev) => (prev.workspace ? { ...prev, workspace: null } : prev));
      for (const [projectId, expanded] of Object.entries(expandedWorkspaces)) {
        if (expanded) onResetWorkspaceTaskPage?.(projectId);
      }
    } else {
      // Reopening restores remembered-expanded workspaces whose pages were
      // reset on collapse: re-request the default first page for each.
      for (const [projectId, expanded] of Object.entries(expandedWorkspaces)) {
        if (!expanded) continue;
        const status = vm.workspaceTaskPages[projectId]?.status ?? 'not-loaded';
        if (status === 'not-loaded' || status === 'error') {
          requestSidebarPage(() => onRequestWorkspaceTasks(projectId));
        }
      }
    }
  };

  const toggleRecentCollapsed = () => {
    const next = !recentCollapsed;
    setRecentCollapsed(next);
    if (next) {
      onResetRecentTaskPage?.();
      setExpandedTaskKeys((prev) => (prev.recent ? { ...prev, recent: null } : prev));
    } else {
      const status = vm.recentTaskPage?.status ?? 'not-loaded';
      if (status === 'not-loaded' || status === 'error') {
        requestSidebarPage(() => onRequestRecentTasks());
      }
    }
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
    // A user interaction owns the highlight scope for the task it activated.
    // Data refreshes for the SAME task (run-history loads, page loads,
    // activity updates) re-run this effect; they must not be re-classified
    // as external activation and steal the scope back to the workspace copy.
    const interaction = runListInteractionRef.current;
    if (interaction && interaction.taskKey === activeTaskKey
      && (interaction.scope === 'pinned' || interaction.scope === 'recent')) return;
    // External activation (search dialog, deep link) of another task owns the
    // workspace copy again: a previous recent/pinned interaction must not
    // leave the highlight scope stuck there with an expanded-but-unhighlighted
    // row.
    setActiveRunListScope('workspace');
    if (workspaceRevealRequest?.taskId && active.kind === 'conversation-run'
      && workspaceRevealRequest.projectId === active.projectId && workspaceRevealRequest.taskId === active.taskId) return;
    setExpandedTaskKeys((prev) => prev.workspace === activeTaskKey
      ? prev
      : updateConversationSidebarExpandedTaskKeys(prev, 'workspace', activeTaskKey, 'expand'));
    const task = Object.values(vm.tasksByWorkspace).flat()
      .find((candidate) => conversationSidebarTaskKey(candidate.projectId, candidate.taskId, candidate.taskUuid) === activeTaskKey);
    if (task && (task.runHistoryStatus === 'not-loaded' || task.runHistoryStatus === 'error')) {
      onRequestTaskRuns(task);
    }
  }, [activeTaskKey, onRequestTaskRuns, vm.tasksByWorkspace, workspaceRevealRequest]);

  const toggleWorkspace = (projectId: string) => {
    cancelPendingTaskReveal();
    const expanding = !expandedWorkspaces[projectId];
    if (expanding) {
      const status = vm.workspaceTaskPages[projectId]?.status ?? 'not-loaded';
      if (status === 'not-loaded' || status === 'error') {
        requestSidebarPage(() => onRequestWorkspaceTasks(projectId));
      }
    } else {
      // Collapsing resets to the default first page; the next expand
      // re-requests page one (see the expanding branch above). The section's
      // expanded-run key drops with it so reopening cannot render an expanded
      // task with an empty not-loaded run list.
      onResetWorkspaceTaskPage?.(projectId);
      setExpandedTaskKeys((prev) => (prev.workspace ? { ...prev, workspace: null } : prev));
    }
    setExpandedWorkspaces((prev) => ({ ...prev, [projectId]: expanding }));
  };

  const markRunListInteraction = (
    scope: ConversationSidebarRunListScope,
    task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
  ) => {
    runListInteractionRef.current = {
      scope,
      taskKey: conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid),
    };
    setActiveRunListScope(scope);
  };

  const selectTaskRun = (
    scope: ConversationSidebarRunListScope,
    task: ConversationTaskRowVm,
    runId: string,
  ) => {
    markRunListInteraction(scope, task);
    onSelect({
      kind: 'conversation-run',
      projectId: task.projectId,
      taskId: task.taskId,
      taskUuid: task.taskUuid,
      runId,
    });
  };

  const selectTask = (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => {
    if (!task.latestRun) return;
    selectTaskRun(scope, task, task.latestRun.runId);
  };

  const toggleTaskRuns = (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => {
    const taskKey = conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid);
    markRunListInteraction(scope, task);
    if (task.runHistoryStatus === 'not-loaded' || task.runHistoryStatus === 'error') {
      onRequestTaskRuns(task);
    }
    setExpandedTaskKeys((prev) => updateConversationSidebarExpandedTaskKeys(prev, scope, taskKey, 'toggle'));
  };

  const expandTaskRuns = (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => {
    markRunListInteraction(scope, task);
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

  // One identity for every row: memoized TaskRows re-render only when their own data changes.
  const rowCommands = useStableCallbacks<TaskRowCommands>({
    select: selectTask,
    selectRun: selectTaskRun,
    toggleRuns: toggleTaskRuns,
    expandRuns: expandTaskRuns,
    loadMoreRuns: (task) => onRequestTaskRuns(task, task.runsNextCursor),
    pin: (task) => onPinTask(task.projectId, task.taskId),
    unpin: (task) => onUnpinTask(task.projectId, task.taskId),
    setResident: async (task, resident) => { await onSetResident?.(task.projectId, task.taskId, resident); },
    rename: (task, title) => onRenameTask(task.projectId, task.taskId, title),
    remove: (task) => onDeleteTask(task.projectId, task.taskId, task.taskUuid),
    pauseRun: (task, runId) => onPauseRun?.(task.projectId, task.taskId, runId),
  });

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
            icon={<MessageSquare className="size-4" />}
            label={t('conversation.sidebar.newChat')}
            onClick={onNewConversation}
          />
          {!readOnly && <SidebarButton
            icon={<Search className="size-4" />}
            label={t('conversation.sidebar.search')}
            onClick={onSearch}
          />}
        </div>

        <Separator className="mx-1 my-1 opacity-45" />

        {/* Navigation */}
        <div className="flex flex-col gap-0.5">
          <SidebarButton
            compact
            active={activeNavigationKey === 'agents'}
            icon={<Bot className="size-3.5" />}
            label={t('conversation.sidebar.agentManagement')}
            onClick={() => onSelect({ kind: 'agents' })}
          />
          <SidebarButton
            compact
            active={activeNavigationKey === 'contexts'}
            icon={<Library className="size-3.5" />}
            label={t('conversation.sidebar.contextManagement')}
            onClick={() => onSelect({ kind: 'contexts' })}
          />
          <SidebarButton
            compact
            active={activeNavigationKey === 'run-mode-management'}
            icon={<Route className="size-3.5" />}
            label={t('conversation.sidebar.runModeManagement')}
            onClick={() => onSelect({ kind: 'run-mode-management' })}
          />
          <Collapsible open={moreNavigationOpen} onOpenChange={setMoreNavigationOpen}>
            <CollapsibleTrigger asChild>
              <Button
                variant="ghost"
                className={cn(
                  'h-6.5 w-full justify-start gap-2 rounded-md px-2 text-sm text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
                  moreNavigationActive && !moreNavigationOpen && 'bg-sidebar-accent text-sidebar-accent-foreground',
                )}
              >
                {/* Spans keep both svgs out of direct-child position: the Button
                    base's `has-[>svg]:px-3` otherwise overrides px-2 and shifts
                    the whole row 4px right of the nav column. */}
                <span><Ellipsis className="size-3.5" /></span>
                <span className="min-w-0 flex-1 truncate text-left">{t('conversation.sidebar.more')}</span>
                <span className="shrink-0"><ChevronDown className={cn('size-3.5 transition-transform', moreNavigationOpen && 'rotate-180')} /></span>
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent
              data-conversation-sidebar-more-content
              className="flex flex-col gap-0.5 overflow-hidden pt-0.5 data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down"
            >
              {showRemoteTaskManagement ? (
                <SidebarButton
                  compact
                  active={active.kind === 'remote-tasks'}
                  icon={<Globe className="size-3.5" />}
                  label={t('conversation.sidebar.remoteTaskManagement')}
                  onClick={() => {
                    setMoreNavigationOpen(true);
                    onSelect({ kind: 'remote-tasks' });
                  }}
                />
              ) : null}
              <SidebarButton
                compact
                active={activeNavigationKey === 'scheduled-tasks'}
                icon={<AlarmClock className="size-3.5" />}
                label={t('scheduled.management.title')}
                onClick={() => {
                  setMoreNavigationOpen(true);
                  onSelect({ kind: 'scheduled-tasks' });
                }}
              />
            </CollapsibleContent>
          </Collapsible>
        </div>
        </div>

        {/* The fixed navigation and the conversation tree are two zones: keep a
            structural separator between them (lost in the 2026-11-05 IA pass). */}
        <Separator data-conversation-sidebar-separator="navigation" className="mx-1 my-1.5 opacity-45" />

        {/* Pinned and workspace sections share the conversation scroll region. */}
        <ScrollArea
          data-conversation-sidebar-region="scrollable-conversations"
          className="min-h-0 flex-1"
          viewportRef={conversationViewportRef}
          onWheelCapture={cancelPendingTaskReveal}
          onPointerDownCapture={cancelPendingTaskReveal}
          onKeyDownCapture={(event) => {
            if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key)) cancelPendingTaskReveal();
          }}
        >
        {vm.pinRefs.length > 0 || vm.pinnedTasks.length > 0 ? (
          <>
          <div className="py-2">
            <button
              type="button"
              data-conversation-sidebar-heading="pinned"
              aria-expanded={!pinnedCollapsed}
              className="sticky top-0 z-[1] flex w-full items-center gap-2 rounded-md bg-sidebar px-2 py-1 text-left text-sm font-medium text-sidebar-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
              onClick={togglePinnedCollapsed}
            >
              <ChevronDown className={cn('size-3.5 shrink-0 transition-transform', pinnedCollapsed && '-rotate-90')} />
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
                            className="flex w-full items-center gap-1.5 py-1 pl-5 pr-1 text-left text-sm font-medium leading-5 text-sidebar-foreground/80 hover:text-sidebar-accent-foreground"
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
                              scope="pinned"
                              commands={rowCommands}
                              residencyEditable={Boolean(onSetResident)}
                              t={t}
                            />
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
                {vm.pinnedTaskPage.status === 'loading' ? (
                  <div className="flex items-center gap-2 pl-5 pr-2 py-1 text-xs text-muted-foreground">
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
          </>
        ) : null}

        {/* Workspaces group (2026-11-05 IA): one sidebar tree — pinned,
            workspaces, recent — with no structural separators. The group
            collapses by default; its header owns the add-workspace action so
            the entry point stays at the first level, mirroring the
            per-workspace new-conversation button one level down. */}
          <div data-conversation-sidebar-region="workspaces" className="pt-1">
            <div className="group sticky top-0 z-[1] flex w-full items-center gap-2 rounded-md bg-sidebar px-2 py-1 text-sm font-medium text-sidebar-foreground hover:bg-sidebar-accent/60">
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    data-conversation-sidebar-heading="workspaces"
                    aria-expanded={!workspacesCollapsed}
                    className="flex min-w-0 flex-1 items-center gap-2 rounded-md text-left hover:text-sidebar-accent-foreground group-hover:pr-18"
                    onClick={toggleWorkspacesCollapsed}
                  >
                    <ChevronDown className={cn('size-3.5 shrink-0 transition-transform', workspacesCollapsed && '-rotate-90')} />
                    <span className="truncate">{t('conversation.sidebar.workspaces')}</span>
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right">{t('conversation.sidebar.workspaces')}</TooltipContent>
              </Tooltip>
              {!readOnly && onAddWorkspace ? (
                <span className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-5 shrink-0 active:scale-90 transition-transform"
                        aria-label={t('conversation.sidebar.addWorkspace')}
                        onClick={onAddWorkspace}
                      >
                        <Plus className="size-3.5" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>{t('conversation.sidebar.addWorkspace')}</TooltipContent>
                  </Tooltip>
                </span>
              ) : null}
            </div>
            {!workspacesCollapsed ? (
              <>
            {vm.loadStatus === 'not-loaded' || vm.loadStatus === 'loading' ? (
              <div className="flex items-center gap-2 pl-5 pr-3 py-3 text-xs text-muted-foreground">
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
                <div data-conversation-workspace-header className="group sticky top-0 z-[1] flex w-full items-center gap-1.5 rounded-lg bg-sidebar py-1 pl-5 pr-1 hover:bg-sidebar-accent">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        data-conversation-workspace-id={ws.projectId}
                        aria-expanded={Boolean(expandedWorkspaces[ws.projectId])}
                        className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-sm font-medium leading-5 text-sidebar-foreground/80 hover:text-sidebar-accent-foreground group-hover:pr-18"
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
                    {!readOnly && onOpenProjectMemory ? (
                      <Tooltip>
                        <TooltipTrigger asChild>
<Button variant="ghost" size="icon" className="size-5 shrink-0" aria-label={t('memory.title')} onClick={(event) => { event.stopPropagation(); onOpenProjectMemory({ projectId: ws.projectId, name: ws.name }); }}>
                            <BookOpen className="size-3" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>{t('memory.title')}</TooltipContent>
                      </Tooltip>
                    ) : null}
                    {!readOnly && onNewConversationInWorkspace ? (
                      <Button variant="ghost" size="icon" className="size-5 active:scale-90 transition-transform" onClick={(e) => { e.stopPropagation(); onNewConversationInWorkspace(ws.projectId); }}>
                        <Plus className="size-3.5" />
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
                    {workspaceRevealRequest?.projectId === ws.projectId && workspaceRevealRequest.status === 'loading' ? (
                      <div role="status" className="flex items-center gap-2 pl-7 pr-3 py-2 text-xs text-muted-foreground"><Loader2 className="size-3 animate-spin" />{t('conversation.sidebar.loadingConversations')}</div>
                    ) : null}
                    {workspaceRevealRequest?.projectId === ws.projectId && workspaceRevealRequest.status === 'error' ? (
                      <Button variant="ghost" size="sm" className="h-7 w-full justify-start pl-7 pr-3 text-xs text-muted-foreground" onClick={onRetryReveal}>{t('conversation.sidebar.retryConversations')}</Button>
                    ) : null}
                    {(vm.tasksByWorkspace[ws.projectId] ?? []).map((task) => (
                      <TaskRow
                        key={conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                        task={task}
                        pinned={pinnedTaskKeys.has(conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid))}
                        isActive={isConversationSidebarRunListScopeActive('workspace', activeRunListScope) && active.kind === 'conversation-run' && active.projectId === task.projectId && active.taskId === task.taskId && (!active.taskUuid || !task.taskUuid || active.taskUuid === task.taskUuid)}
                        activeRunKey={isConversationSidebarRunListScopeActive('workspace', activeRunListScope) ? activeRunKey : null}
                        expanded={expandedTaskKeys.workspace === conversationSidebarTaskKey(task.projectId, task.taskId, task.taskUuid)}
                        scope="workspace"
                        commands={rowCommands}
                        residencyEditable={Boolean(onSetResident)}
                        t={t}
                      />
                    ))}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'loading' ? (
                      <div className="flex items-center gap-2 pl-7 pr-3 py-2 text-xs text-muted-foreground">
                        <Loader2 className="size-3 animate-spin" />
                        {t('conversation.sidebar.loadingConversations')}
                      </div>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'error' ? (
                      <Button variant="ghost" size="sm" className="h-7 w-full justify-start pl-7 pr-3 text-xs text-muted-foreground" onClick={() => requestSidebarPage(() => onRequestWorkspaceTasks(ws.projectId))}>
                        {t('conversation.sidebar.retryConversations')}
                      </Button>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'ready'
                      && !vm.workspaceTaskPages[ws.projectId]?.nextCursor
                      && (vm.tasksByWorkspace[ws.projectId] ?? []).length === 0
                      && !(workspaceRevealRequest?.projectId === ws.projectId && workspaceRevealRequest.status) ? (
                      <div className="pl-7 pr-3 py-2 text-xs text-muted-foreground">{t('conversation.noConversations')}</div>
                    ) : null}
                    {(vm.workspaceTaskPages[ws.projectId]?.status === 'ready' && vm.workspaceTaskPages[ws.projectId]?.nextCursor)
                      || ((vm.workspaceTaskPages[ws.projectId]?.status ?? 'not-loaded') === 'not-loaded' && vm.tasksByWorkspace[ws.projectId]?.length) ? (
                      <Button variant="ghost" size="sm" className="h-7 w-full justify-start pl-7 pr-3 text-xs text-muted-foreground" onClick={() => requestSidebarPage(() => onRequestWorkspaceTasks(ws.projectId, vm.workspaceTaskPages[ws.projectId]?.nextCursor))}>
                        {t('conversation.sidebar.loadMoreConversations')}
                      </Button>
                    ) : null}
                    {vm.workspaceTaskPages[ws.projectId]?.status === 'ready'
                      && !vm.workspaceTaskPages[ws.projectId]?.nextCursor
                      && (vm.tasksByWorkspace[ws.projectId] ?? []).length >= CONVERSATION_TASK_PAGE_SIZE ? (
                      <div className="pl-7 pr-3 py-1 text-xs text-muted-foreground/70">{t('conversation.sidebar.allConversationsShown')}</div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ))}

            {vm.loadStatus === 'ready' && vm.workspaces.length === 0 ? (
              <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                {t('conversation.sidebar.noWorkspaces')}
              </div>
            ) : null}
              </>
            ) : null}
          </div>

        {/* Recent group (2026-11-05 IA): part of the shared scroll tree —
            no structural separator and no fixed-height tail; workspaces and
            recent rows share one scrolling column. */}
        <div data-conversation-sidebar-region="recent" className="pt-1 pb-2">
            <button
              type="button"
              data-conversation-sidebar-heading="recent"
              aria-expanded={!recentCollapsed}
              className="sticky top-0 z-[1] flex w-full items-center gap-2 rounded-md bg-sidebar px-2 py-1 text-left text-sm font-medium text-sidebar-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
              onClick={toggleRecentCollapsed}
            >
              <ChevronDown className={cn('size-3.5 shrink-0 transition-transform', recentCollapsed && '-rotate-90')} />
              {t('conversation.sidebar.recent')}
            </button>
            {!recentCollapsed ? (
              <div className="mt-1 space-y-0.5">
                {(vm.recentTasks ?? []).map((row) => (
                  <TaskRow
                    key={`recent-${conversationSidebarTaskKey(row.projectId, row.taskId, row.taskUuid)}`}
                    task={row}
                    workspaceName={row.workspaceName}
                    pinned={pinnedTaskKeys.has(conversationSidebarTaskKey(row.projectId, row.taskId, row.taskUuid))}
                    isActive={isConversationSidebarRunListScopeActive('recent', activeRunListScope) && active.kind === 'conversation-run' && active.projectId === row.projectId && active.taskId === row.taskId && (!active.taskUuid || !row.taskUuid || active.taskUuid === row.taskUuid)}
                    activeRunKey={isConversationSidebarRunListScopeActive('recent', activeRunListScope) ? activeRunKey : null}
                    expanded={expandedTaskKeys.recent === conversationSidebarTaskKey(row.projectId, row.taskId, row.taskUuid)}
                    scope="recent"
                    commands={rowCommands}
                    residencyEditable={Boolean(onSetResident)}
                    t={t}
                  />
                ))}
                {(vm.recentTaskPage?.status ?? 'not-loaded') === 'loading' ? (
                  <div className="flex items-center gap-2 pl-5 pr-2 py-1 text-xs text-muted-foreground">
                    <Loader2 className="size-3 animate-spin" />
                    {t('conversation.sidebar.loadingRecent')}
                  </div>
                ) : null}
                {(vm.recentTaskPage?.status ?? 'not-loaded') === 'error' ? (
                  <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={() => requestSidebarPage(() => onRequestRecentTasks())}>
                    {t('conversation.sidebar.retryRecent')}
                  </Button>
                ) : null}
                {(vm.recentTaskPage?.status ?? 'not-loaded') === 'ready' && (vm.recentTasks ?? []).length === 0 && !vm.recentTaskPage?.nextCursor ? (
                  <div className="pl-5 pr-2 py-1 text-xs text-muted-foreground">{t('conversation.sidebar.noRecent')}</div>
                ) : null}
                {(vm.recentTaskPage?.status ?? 'not-loaded') === 'ready' && vm.recentTaskPage?.nextCursor ? (
                  <Button variant="ghost" size="sm" className="h-7 w-full justify-start text-xs text-muted-foreground" onClick={() => requestSidebarPage(() => onRequestRecentTasks(vm.recentTaskPage?.nextCursor))}>
                    {t('conversation.sidebar.loadMoreRecent')}
                  </Button>
                ) : null}
                {(vm.recentTaskPage?.status ?? 'not-loaded') === 'ready' && !vm.recentTaskPage?.nextCursor
                  && (vm.recentTasks ?? []).length >= CONVERSATION_TASK_PAGE_SIZE ? (
                  <div className="pl-5 pr-2 py-1 text-xs text-muted-foreground/70">{t('conversation.sidebar.allRecentShown')}</div>
                ) : null}
              </div>
            ) : null}
        </div>
        </ScrollArea>


        {/* Settings */}
        <Separator className="mx-1 my-1.5 opacity-45" />
        <SidebarButton icon={<Settings className="size-4" />} label={t('conversation.sidebar.settings')} onClick={() => onSelect({ kind: 'settings' })} />
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
  if (run.status === 'running') return 'bg-gold-running animate-pulse';
  if (run.status === 'paused') return 'bg-yellow-500/50';
  if (run.outcome === 'success') return 'bg-emerald-500/50';
  if (run.outcome === 'failure' || run.outcome === 'killed') return 'bg-red-500/50';
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

/** Direct Agent rows show activity (breathing icon + spinner) while the task is active. */
export function shouldShowConversationSidebarActivity(
  task: Pick<ConversationTaskRowVm, 'runMode' | 'agentIdentity'>,
  taskActive: boolean,
) {
  return task.runMode === 'direct' && Boolean(task.agentIdentity) && taskActive;
}

export type ConversationSidebarRunListScope = 'pinned' | 'workspace' | 'recent';

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

/** Row actions shared by every TaskRow; identity-stable so the memoized rows can skip renders. */
interface TaskRowCommands {
  select: (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => void;
  selectRun: (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm, runId: string) => void;
  toggleRuns: (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => void;
  expandRuns: (scope: ConversationSidebarRunListScope, task: ConversationTaskRowVm) => void;
  loadMoreRuns: (task: ConversationTaskRowVm) => void;
  pin: (task: ConversationTaskRowVm) => void;
  unpin: (task: ConversationTaskRowVm) => void;
  setResident: (task: ConversationTaskRowVm, resident: boolean) => Promise<void>;
  rename: (task: ConversationTaskRowVm, title: string) => void;
  remove: (task: ConversationTaskRowVm) => void;
  pauseRun: (task: ConversationTaskRowVm, runId: string) => void | Promise<void>;
}

const TaskRow = memo(function TaskRow({
  task,
  scope,
  pinned,
  workspaceName,
  isActive,
  activeRunKey,
  expanded,
  commands,
  residencyEditable,
  t,
}: {
  task: ConversationTaskRowVm;
  scope: ConversationSidebarRunListScope;
  pinned: boolean;
  /** Cross-workspace recent rows carry their workspace name next to the time. */
  workspaceName?: string;
  isActive: boolean;
  activeRunKey?: string | null;
  expanded: boolean;
  commands: TaskRowCommands;
  residencyEditable: boolean;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const onSelect = () => commands.select(scope, task);
  const onSelectRun = (runId: string) => commands.selectRun(scope, task, runId);
  const onToggleRuns = () => commands.toggleRuns(scope, task);
  const onExpandRuns = () => commands.expandRuns(scope, task);
  const onLoadMoreRuns = () => commands.loadMoreRuns(task);
  // Pinned-section rows only offer unpin; workspace and recent rows choose by the pinned flag.
  const onPin = scope === 'pinned' ? undefined : () => commands.pin(task);
  const onUnpin = () => commands.unpin(task);
  const onSetResident = residencyEditable ? (resident: boolean) => commands.setResident(task, resident) : undefined;
  const onRename = (title: string) => commands.rename(task, title);
  const onDelete = () => commands.remove(task);
  const onPauseRun = (runId: string) => commands.pauseRun(task, runId);
  const [editing, setEditing] = useState(false);
  const [residencyPending, setResidencyPending] = useState(false);
  const readOnly = useReadOnlyExperience();
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [openRunMenuId, setOpenRunMenuId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState(task.title);
  const editInputRef = useRef<HTMLInputElement>(null);
  const hasRuns = shouldShowConversationSidebarRunList(task);
  const runListOwnsSelection = doesConversationSidebarRunListOwnSelection(task, activeRunKey, expanded && hasRuns);

  const latestRun = task.latestRun;
  const isDirect = task.runMode === 'direct';
  const useAgentIdentity = conversationSidebarIdentityKind(task) === 'agent-icon';
  const taskActive = useConversationTaskActive(task.activity, `${task.projectId}:${task.taskId}`);
  const showActivity = shouldShowConversationSidebarActivity(task, taskActive);
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

  // Recent rows without a run have nowhere to navigate: render them as static
  // rows without hover/cursor affordances or hover actions (parity with the
  // former RecentTaskRow, commit cd214297).
  const deadRecentRow = scope === 'recent' && !task.latestRun;
  const taskRow = (
    <div
      data-conversation-task-row={scope}
      data-project-id={task.projectId}
      data-task-id={task.taskId}
      data-task-uuid={task.taskUuid ?? undefined}
      className={cn(
        'group relative flex w-full min-w-0 items-center gap-2 rounded-lg py-1.5 pl-7 pr-2 text-sidebar-foreground',
        deadRecentRow ? 'text-sidebar-foreground/70' : 'cursor-pointer',
        // Selection sinks to the active run row while it is visible in the
        // expanded run list; the task row keeps only its weight then, so
        // exactly one row carries the accent at any time.
        isActive
          ? cn('font-medium text-sidebar-accent-foreground', !runListOwnsSelection && 'bg-sidebar-accent/70')
          : !deadRecentRow && 'hover:bg-sidebar-accent',
      )}
      onClick={deadRecentRow ? undefined : handleRowClick}
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
          <span data-task-status-project={task.projectId} className={cn('size-1.5 rounded-full', latestColor)} />
        )}
      </span>
      <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
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
            {residencyPending ? <Loader2 className="size-3 shrink-0 animate-spin" aria-label={t("common.loading")} /> : null}
            {task.runMode === "direct" && task.resident ? <Tooltip><TooltipTrigger asChild><span className="inline-flex shrink-0" aria-label={t("conversation.sidebar.resident")}><ResidentPin /></span></TooltipTrigger><TooltipContent>{t("conversation.sidebar.resident")}</TooltipContent></Tooltip> : null}
          </span>
        )}
        {showActivity ? (
          <span className="inline-flex h-4 shrink-0 items-center" role="status" aria-label={t('status.running')}>
            <AcpProcessingSpinner className="size-3" />
          </span>
        ) : relativeTime || workspaceName ? (
          <span className="shrink-0 text-ui-caption font-normal leading-4 tabular-nums text-muted-foreground/55">
            {workspaceName && relativeTime ? `${workspaceName} · ${relativeTime}` : (workspaceName ?? relativeTime)}
          </span>
        ) : null}
      </div>
      {deadRecentRow ? null : (
      <span data-demo-task-actions={readOnly || undefined} className="hidden shrink-0 items-center gap-1 group-hover:flex group-focus-within:flex">
        {(
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <Button disabled={readOnly} aria-label={t('conversation.sidebar.rename')} variant="ghost" size="icon" className="size-5 shrink-0" onClick={startRename}>
                  <Pencil className="size-3" />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent>{t('conversation.sidebar.rename')}</TooltipContent>
          </Tooltip>
        )}
        {pinned && onUnpin ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <Button disabled={readOnly} aria-label={t('conversation.sidebar.unpin')} variant="ghost" size="icon" className="size-5 shrink-0" onClick={(e) => { e.stopPropagation(); onUnpin(); }}>
                  <PinOff className="size-3" />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent>{t('conversation.sidebar.unpin')}</TooltipContent>
          </Tooltip>
        ) : onPin ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <Button disabled={readOnly} aria-label={t('conversation.sidebar.pinToTop')} variant="ghost" size="icon" className="size-5 shrink-0" onClick={(e) => { e.stopPropagation(); onPin(); }}>
                  <Pin className="size-3" />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent>{t('conversation.sidebar.pinToTop')}</TooltipContent>
          </Tooltip>
        ) : null}
        {(
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex">
                <Button disabled={readOnly} aria-label={t('common.delete')} variant="ghost" size="icon" className="size-5 shrink-0 text-muted-foreground hover:text-destructive" onClick={openDeleteDialog}>
                  <Trash2 className="size-3" />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent>{t('common.delete')}</TooltipContent>
          </Tooltip>
        )}
      </span>
      )}
    </div>
  );

  return (
    <>
    <div className={cn(expanded && hasRuns && 'space-y-1')}>
      {task.runMode === "direct" && onSetResident ? (
        <ContextMenu><ContextMenuTrigger asChild>{taskRow}</ContextMenuTrigger>
          <ContextMenuContent><ContextMenuItem disabled={readOnly || residencyPending} onSelect={() => {
            setResidencyPending(true);
            void onSetResident(!task.resident).finally(() => setResidencyPending(false));
          }}>
            {residencyPending ? <Loader2 className="size-4 animate-spin" /> : task.resident ? <PinOff className="size-4" /> : <Pin className="size-4" />}
            {t(task.resident ? "conversation.sidebar.releaseResident" : "conversation.sidebar.keepResident")}
          </ContextMenuItem></ContextMenuContent>
        </ContextMenu>
      ) : taskRow}
      {expanded && hasRuns ? (
        <div className="ml-7 mt-1 space-y-1 border-l border-border/60">
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
                    // Runs nest one level under the task row: the guide border
                    // sits at the task text indent (28px) and the row stretches
                    // back to full width so hover/active shading matches the
                    // parent task row instead of floating indented.
                    'flex items-center gap-2 rounded-md -ml-7 pl-9 pr-2 py-1 cursor-pointer text-xs leading-4',
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
            <div className="flex items-center gap-2 pl-2 pr-2 py-1 text-xs text-muted-foreground">
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
});

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
      {/* The span keeps svgs out of direct-child position so the Button base's
          `has-[>svg]:px-3` cannot override the row's px-2 padding. Icon sizes
          are set by an explicit size-* class on each icon at the call site:
          span-level `[&_svg]:size-*` loses the specificity race to the Button
          base's `[&_svg:not([class*='size-'])]:size-4` and never applied. */}
      <span>{icon}</span>
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
  taskId?: string;
  taskUuid?: string | null;
  status?: 'loading' | 'error';
  scrollHandled?: boolean;
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

/** The expanded run list owns the selection accent while the active run row is
 *  actually rendered in it; otherwise (collapsed list, run history not yet
 *  loaded, or the active run sits beyond the loaded page) the task row keeps
 *  the accent so exactly one row stays highlighted at any time. */
export function doesConversationSidebarRunListOwnSelection(
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid' | 'runs'>,
  activeRunKey: string | null | undefined,
  runListExpanded: boolean,
) {
  if (activeRunKey == null || !runListExpanded) return false;
  return task.runs.some((run) => isConversationSidebarRunActive(activeRunKey, task.projectId, task.taskId, run.runId, task.taskUuid));
}
