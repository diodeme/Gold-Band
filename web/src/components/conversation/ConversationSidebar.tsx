import { Pin, PinOff, MessageSquare, Search, Bot, Boxes, Workflow, Settings, ChevronDown, Pencil, Plus, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useEffect, useRef, useState } from 'react';
import type { ConversationPage, ConversationSidebarVm, ConversationTaskRowVm } from '../../types';
import { saveConversationPreference } from '../../api';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ConversationSidebarProps {
  vm: ConversationSidebarVm;
  active: ConversationPage;
  onSelect: (page: ConversationPage) => void;
  onToggleUiMode: () => void;
  onNewConversation: () => void;
  onSearch: () => void;
  onSelectTask: (projectId: string, taskId: string) => void;
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string) => void;
  onNewConversationInWorkspace?: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (projectId: string) => void;
}

export function ConversationSidebar({
  vm,
  active,
  onSelect,
  onToggleUiMode: _onToggleUiMode,
  onNewConversation,
  onSearch,
  onSelectTask,
  onSelectRun,
  onPinTask,
  onUnpinTask,
  onRenameTask,
  onDeleteTask,
  onNewConversationInWorkspace,
  onAddWorkspace,
  onRemoveWorkspace,
}: ConversationSidebarProps) {
  const { t } = useTranslation();
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Record<string, boolean>>({});
  const [pinnedCollapsed, setPinnedCollapsed] = useState(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') return pref;
    return false;
  });
  const [collapsedPinnedWorkspaces, setCollapsedPinnedWorkspaces] = useState<Record<string, boolean>>({});

  // Sync pinned collapse from persisted preferences when sidebar VM reloads
  useEffect(() => {
    const pref = vm.preferences?.['pinned.collapsed'];
    if (typeof pref === 'boolean') setPinnedCollapsed(pref);
  }, [vm.preferences]);

  useEffect(() => {
    const targetWorkspaceId = active.kind === 'conversation-run'
      ? active.projectId
      : vm.lastActiveWorkspaceId;
    setExpandedWorkspaces((prev) => {
      const next: Record<string, boolean> = {};
      vm.workspaces.forEach((ws) => {
        if (prev[ws.projectId] != null) {
          next[ws.projectId] = prev[ws.projectId];
          return;
        }
        next[ws.projectId] = ws.projectId === targetWorkspaceId || targetWorkspaceId == null;
      });
      if (targetWorkspaceId && next[targetWorkspaceId] === false) {
        next[targetWorkspaceId] = true;
      }
      return next;
    });
  }, [active, vm.workspaces, vm.lastActiveWorkspaceId]);

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

  const activeRunId = active.kind === 'conversation-run' ? active.runId : null;

  const toggleWorkspace = (projectId: string) => {
    setExpandedWorkspaces((prev) => ({ ...prev, [projectId]: !prev[projectId] }));
  };

  return (
    <TooltipProvider>
      <aside className="flex min-h-0 h-full flex-col gap-0.5 bg-sidebar px-3 py-3 text-sidebar-foreground">
        {/* Quick actions */}
        <div className="flex flex-col gap-0.5">
          <SidebarButton
            active={active.kind === 'conversation-home'}
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

        <Separator className="mx-1 my-0 opacity-45" />

        {/* Navigation */}
        <div className="flex flex-col gap-1">
          <SidebarButton
            compact
            active={active.kind === 'agents'}
            icon={<Bot />}
            label={t('conversation.sidebar.agentManagement')}
            onClick={() => onSelect({ kind: 'agents' })}
          />
          <SidebarButton
            compact
            active={active.kind === 'contexts'}
            icon={<Boxes />}
            label={t('conversation.sidebar.contextManagement')}
            onClick={() => onSelect({ kind: 'contexts' })}
          />
          <SidebarButton
            compact
            active={active.kind === 'run-mode-management'}
            icon={<Workflow />}
            label={t('conversation.sidebar.runModeManagement')}
            onClick={() => onSelect({ kind: 'run-mode-management' })}
          />
        </div>

        {/* Pinned section — fixed, collapsible, outside scroll */}
        {vm.pinnedTasks.length > 0 ? (
          <div className="shrink-0 border-y border-border/55 py-1">
            <button
              type="button"
              className="flex w-full items-center gap-1.5 px-1 py-0.75 text-left text-[14px] font-medium text-muted-foreground hover:text-sidebar-accent-foreground"
              onClick={togglePinnedCollapsed}
            >
              <ChevronDown className={cn('size-3 transition-transform', pinnedCollapsed && '-rotate-90')} />
              {t('conversation.sidebar.pinned')}
            </button>
            {!pinnedCollapsed ? (
              <div>
                {Object.entries(
                  vm.pinnedTasks.reduce<Record<string, ConversationTaskRowVm[]>>((acc, task) => {
                    (acc[task.projectId] ??= []).push(task);
                    return acc;
                  }, {}),
                ).map(([projectId, tasks]) => {
                  const ws = vm.workspaces.find((w) => w.projectId === projectId);
                  const isWsCollapsed = collapsedPinnedWorkspaces[projectId] ?? false;
                  return (
                    <div key={`pinned-ws-${projectId}`}>
                      <button
                        type="button"
                        className="flex w-full items-center gap-1.5 px-1 py-0.75 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-muted-foreground hover:text-sidebar-accent-foreground"
                        onClick={() => togglePinnedWorkspace(projectId)}
                      >
                        <ChevronDown className={cn('size-3 shrink-0 transition-transform', isWsCollapsed && '-rotate-90')} />
                        <span className="truncate">{ws?.name ?? projectId}</span>
                      </button>
                      {!isWsCollapsed ? (
                        <div className="space-y-1">
                          {tasks.map((task) => (
                            <TaskRow
                              key={`pinned-${task.projectId}-${task.taskId}`}
                              task={task}
                              pinned
                              isActive={active.kind === 'conversation-run' && active.projectId === task.projectId && active.taskId === task.taskId}
                              activeRunId={activeRunId}
                              onSelect={() => onSelectTask(task.projectId, task.taskId)}
                              onSelectRun={(runId) => onSelectRun(task.projectId, task.taskId, runId)}
                              onUnpin={() => onUnpinTask(task.projectId, task.taskId)}
                              onRename={(title) => onRenameTask(task.projectId, task.taskId, title)}
                              onDelete={() => onDeleteTask(task.projectId, task.taskId)}
                              t={t}
                            />
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            ) : null}
          </div>
        ) : (
          <Separator className="mx-1 my-0.75 opacity-45" />
        )}

        {/* Workspace sections — scrollable with sticky headers */}
        <ScrollArea className="min-h-0 flex-1">
          <div className="pt-2.5">
            {vm.workspaces.map((ws) => (
              <div key={ws.projectId} className="mb-2.5">
                <div className="group sticky top-0 z-[1] flex w-full items-center gap-1.5 bg-sidebar px-1 py-0.75">
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-1.5 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-muted-foreground hover:text-sidebar-accent-foreground group-hover:pr-11"
                    onClick={() => toggleWorkspace(ws.projectId)}
                  >
                    <ChevronDown className={cn('size-3 shrink-0 transition-transform', !expandedWorkspaces[ws.projectId] && '-rotate-90')} />
                    <span className="truncate">{ws.name}</span>
                  </button>
                  <span className="pointer-events-none absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity group-hover:pointer-events-auto group-hover:opacity-100">
                    {onNewConversationInWorkspace ? (
                      <Button variant="ghost" size="icon" className="size-5 active:scale-90 transition-transform" onClick={(e) => { e.stopPropagation(); onNewConversationInWorkspace(ws.projectId); }}>
                        <Plus className="size-3" />
                      </Button>
                    ) : null}
                    {onRemoveWorkspace ? (
                      <Button variant="ghost" size="icon" className="size-5 text-muted-foreground hover:text-destructive active:scale-90 transition-transform" onClick={(e) => { e.stopPropagation(); onRemoveWorkspace(ws.projectId); }}>
                        <Trash2 className="size-3" />
                      </Button>
                    ) : null}
                  </span>
                </div>
                {expandedWorkspaces[ws.projectId] ? (
                  <div className="space-y-1">
                    {(vm.tasksByWorkspace[ws.projectId] ?? []).map((task) => (
                      <TaskRow
                        key={`${task.projectId}-${task.taskId}`}
                        task={task}
                        pinned={vm.pinnedTasks.some((p) => p.projectId === task.projectId && p.taskId === task.taskId)}
                        isActive={active.kind === 'conversation-run' && active.projectId === task.projectId && active.taskId === task.taskId}
                        activeRunId={activeRunId}
                        onSelect={() => onSelectTask(task.projectId, task.taskId)}
                        onSelectRun={(runId) => onSelectRun(task.projectId, task.taskId, runId)}
                        onPin={() => onPinTask(task.projectId, task.taskId)}
                        onUnpin={() => onUnpinTask(task.projectId, task.taskId)}
                        onRename={(title) => onRenameTask(task.projectId, task.taskId, title)}
                        onDelete={() => onDeleteTask(task.projectId, task.taskId)}
                        t={t}
                      />
                    ))}
                    {(!vm.tasksByWorkspace[ws.projectId] || vm.tasksByWorkspace[ws.projectId].length === 0) ? (
                      <div className="px-3 py-2 text-xs text-muted-foreground">{t('conversation.noConversations')}</div>
                    ) : null}
                  </div>
                ) : null}
              </div>
            ))}

            {/* Add workspace button */}
            {onAddWorkspace ? (
              <button
                type="button"
                className="mt-1.5 flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[14px] text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                onClick={onAddWorkspace}
              >
                <Plus className="size-3.5" />
                <span>{t('conversation.sidebar.addWorkspace')}</span>
              </button>
            ) : null}

            {vm.workspaces.length === 0 ? (
              <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                {t('conversation.sidebar.noPinned')}
              </div>
            ) : null}
          </div>
        </ScrollArea>


        {/* Settings */}
        <Separator className="mx-1 my-0.75 opacity-45" />
        <SidebarButton icon={<Settings />} label={t('conversation.sidebar.settings')} onClick={() => onSelect({ kind: 'settings' })} />
      </aside>
    </TooltipProvider>
  );
}

// ── Task Row ──

function runStatusColor(run: ConversationTaskRowVm['runs'][0]) {
  if (run.outcome === 'success') return 'bg-emerald-500/50';
  if (run.outcome === 'failure' || run.outcome === 'killed') return 'bg-red-500/50';
  if (run.status === 'running') return 'bg-transparent';
  return 'bg-yellow-500/50';
}

function TaskRow({
  task,
  pinned,
  isActive,
  activeRunId,
  onSelect,
  onSelectRun,
  onPin,
  onUnpin,
  onRename,
  onDelete,
  t,
}: {
  task: ConversationTaskRowVm;
  pinned: boolean;
  isActive: boolean;
  activeRunId?: string | null;
  onSelect: () => void;
  onSelectRun?: (runId: string) => void;
  onPin?: () => void;
  onUnpin?: () => void;
  onRename?: (title: string) => void;
  onDelete?: () => void;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [editing, setEditing] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [editValue, setEditValue] = useState(task.title);
  const editInputRef = useRef<HTMLInputElement>(null);
  const hasMultipleRuns = task.runs.length > 1;

  const latestColor = task.latestRun ? runStatusColor(task.latestRun) : 'bg-muted-foreground/30';
  const relativeTime = task.latestRun && task.latestRun.status !== 'running'
    ? formatRelativeTime(task.latestRun.updatedAt, t)
    : null;

  const handleRowClick = () => {
    if (hasMultipleRuns) {
      if (isActive) {
        // Already viewing a run of this task — just toggle expand, don't re-navigate
        setExpanded((prev) => !prev);
        return;
      }
      setExpanded(true);
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

  return (
    <>
    <div className={cn(expanded && hasMultipleRuns && 'space-y-1')}>
      <div
        className={cn(
          'group relative flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1.5 cursor-pointer',
          isActive ? 'bg-sidebar-accent/70' : 'hover:bg-sidebar-accent',
        )}
        onClick={handleRowClick}
      >
        <span className={cn('size-1.5 shrink-0 rounded-full', latestColor, task.latestRun?.status === 'running' && 'border border-muted-foreground/40')} />
        <div className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden group-hover:pr-20">
          {editing ? (
            <input
              ref={editInputRef}
              className="min-w-0 flex-1 rounded border border-primary/40 bg-background px-1 py-0 text-[13px] outline-none"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={commitRename}
              onKeyDown={handleRenameKeyDown}
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <span className="min-w-0 flex-1 truncate text-[13px]">{task.title}</span>
          )}
          {relativeTime ? (
            <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">{relativeTime}</span>
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
      {expanded && hasMultipleRuns ? (
        <div className="ml-4 mt-1 space-y-1 border-l border-border/60 pl-3">
          {task.runs.map((run) => {
            const color = runStatusColor(run);
            const runTime = run.status !== 'running'
              ? formatRelativeTime(run.updatedAt, t)
              : null;
            return (
              <div
                key={run.runId}
                className={cn(
                  'flex items-center gap-2 rounded-md px-2 py-1 cursor-pointer text-xs',
                  activeRunId === run.runId ? 'bg-sidebar-accent text-sidebar-primary' : 'hover:bg-sidebar-accent',
                )}
                onClick={() => onSelectRun?.(run.runId)}
              >
                <span className={cn('size-1.5 shrink-0 rounded-full', color, run.status === 'running' && 'border border-muted-foreground/40')} />
                <span className="min-w-0 flex-1 truncate text-muted-foreground">{run.runId}</span>
                {runTime ? (
                  <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">{runTime}</span>
                ) : null}
              </div>
            );
          })}
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
        compact ? 'h-7 gap-2 justify-start rounded-md px-2 text-[14px] text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground'
          : 'h-7 justify-start gap-2.5 rounded-lg px-2.5 text-[14px] text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
        active && 'bg-sidebar-accent text-sidebar-primary',
      )}
      onClick={onClick}
    >
      <span className={cn(compact ? '[&_svg]:size-3.5' : '[&_svg]:size-4')}>{icon}</span>
      <span>{label}</span>
    </Button>
  );
}

// ── Helpers ──

function formatRelativeTime(isoString: string, t: (key: string, options?: Record<string, unknown>) => string): string {
  const now = Date.now();
  // Handle Unix timestamp format "1749331234Z" used internally
  let then: number;
  if (/^\d+Z?$/.test(isoString)) {
    then = parseInt(isoString, 10) * 1000;
  } else {
    then = new Date(isoString).getTime();
  }
  if (isNaN(then) || then <= 0) return '';
  const diffMs = now - then;
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 1) return t('conversation.runtime.justNow') ?? 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 4) return `${weeks}w`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.floor(days / 365)}y`;
}
