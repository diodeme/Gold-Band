import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { AlarmClock, MoreHorizontal, Pause, Play, Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { deleteScheduledTask, getScheduledTask, listScheduledTasks, runScheduledTaskNow, setScheduledTaskEnabled, subscribeScheduledTaskUpdates, updateScheduledTask } from '@/api';
import { Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { ScheduledTaskDialog, type ScheduledTaskInitialConfig } from '@/components/conversation/ScheduledTaskDialog';
import { formatScheduledSchedule, scheduledScheduleTimezone } from '@/lib/scheduled-task-formatting';
import type { ScheduledTaskEditVm, ScheduledTaskVm } from '@/types';

type StatusFilter = 'all' | 'enabled' | 'disabled';
type TaskAction = 'enable' | 'run' | 'edit' | 'delete';

export const scheduledWorkspaceFilterTriggerClassName = 'w-28 text-xs';

const modeLabels: Record<string, string> = {
  direct: 'Direct',
  workflow: 'Workflow',
  auto: 'AUTO',
};

export function scheduledTaskStatusLabel(t: TFunction, status: string) {
  return t(`scheduled.status.${status}`, { defaultValue: status });
}

export function formatTimestamp(value?: string | null) {
  if (!value) return '--';
  try {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' }).format(new Date(value));
  } catch {
    return value;
  }
}

export function ScheduledTaskManagementPage({ projectId: _projectId, onCreate, onOpenDetail }: { projectId?: string; onCreate?: () => void; onOpenDetail?: (task: ScheduledTaskVm) => void }) {
  const { t } = useTranslation();
  const [tasks, setTasks] = useState<ScheduledTaskVm[]>([]);
  const [filter, setFilter] = useState<StatusFilter>('all');
  const [workspaceFilter, setWorkspaceFilter] = useState('all');
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [pendingTaskActions, setPendingTaskActions] = useState<Record<string, TaskAction>>({});
  const pendingTaskActionsRef = useRef<Record<string, TaskAction>>({});
  const taskListRequestIdRef = useRef(0);
  const taskMutationGenerationRef = useRef(0);
  const [editing, setEditing] = useState<{ task: ScheduledTaskVm; definition: ScheduledTaskEditVm } | null>(null);
  const [editLoading, setEditLoading] = useState(false);
  const [deleting, setDeleting] = useState<ScheduledTaskVm | null>(null);

  const loadTasks = useCallback(() => {
    const requestId = ++taskListRequestIdRef.current;
    const mutationGeneration = taskMutationGenerationRef.current;
    setLoading(true);
    setLoadError(false);
    return listScheduledTasks(null)
      .then((nextTasks) => {
        if (requestId !== taskListRequestIdRef.current || mutationGeneration !== taskMutationGenerationRef.current) return;
        setTasks(nextTasks);
      })
      .catch(() => {
        if (requestId === taskListRequestIdRef.current) setLoadError(true);
      })
      .finally(() => {
        if (requestId === taskListRequestIdRef.current) setLoading(false);
      });
  }, []);

  useEffect(() => {
    void loadTasks();
  }, [loadTasks]);

  // Local-merge: update only the matching task without a full reload.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledTaskUpdates((event) => {
      taskMutationGenerationRef.current += 1;
      setTasks((prev) =>
        prev.map((item) =>
          item.id === event.scheduledTaskId && event.task
            ? { ...event.task!, workspaceName: item.workspaceName }
            : item,
        ),
      );
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const workspaces = useMemo(
    () => Array.from(new Set(tasks.map((task) => task.workspaceName))).sort((left, right) => left.localeCompare(right)),
    [tasks],
  );

  const visibleTasks = useMemo(() => tasks.filter((task) => {
    const matchesStatus = filter === 'disabled' ? !task.enabled : filter === 'enabled' ? task.enabled : true;
    const matchesWorkspace = workspaceFilter === 'all' || task.workspaceName === workspaceFilter;
    return matchesStatus && matchesWorkspace;
  }), [filter, tasks, workspaceFilter]);

  const setTaskPending = (taskId: string, action: TaskAction | null) => {
    const next = { ...pendingTaskActionsRef.current };
    if (action) next[taskId] = action;
    else delete next[taskId];
    pendingTaskActionsRef.current = next;
    setPendingTaskActions(next);
  };

  const updateEnabled = async (task: ScheduledTaskVm, enabled: boolean) => {
    if (pendingTaskActionsRef.current[task.id]) return;
    setTaskPending(task.id, 'enable');
    setActionError(false);
    try {
      const updated = await setScheduledTaskEnabled(task.projectId, task.id, enabled);
      taskMutationGenerationRef.current += 1;
      setTasks((current) => current.map((item) => item.id === updated.id ? updated : item));
    } catch {
      setActionError(true);
    } finally {
      setTaskPending(task.id, null);
    }
  };

  const runNow = async (task: ScheduledTaskVm) => {
    if (pendingTaskActionsRef.current[task.id]) return;
    setTaskPending(task.id, 'run');
    setActionError(false);
    try {
      await runScheduledTaskNow(task.projectId, task.id);
      await loadTasks();
    } catch {
      setActionError(true);
    } finally {
      setTaskPending(task.id, null);
    }
  };

  const openEdit = async (task: ScheduledTaskVm) => {
    if (pendingTaskActionsRef.current[task.id]) return;
    setTaskPending(task.id, 'edit');
    setActionError(false);
    setEditLoading(true);
    try {
      const definition = await getScheduledTask(task.projectId, task.id);
      setEditing({ task, definition });
    } catch {
      setActionError(true);
    } finally {
      setEditLoading(false);
      setTaskPending(task.id, null);
    }
  };

  const editConfig = (definition: ScheduledTaskEditVm): ScheduledTaskInitialConfig => ({
    schedule: definition.schedule,
    overlapPolicy: definition.overlapPolicy,
    sessionPolicy: definition.sessionPolicy,
  });

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        variant="integrated"
        icon={<AlarmClock />}
        title={<span className="text-title">{t('scheduled.management.title')}</span>}
        badges={<span className="text-sm font-normal text-muted-foreground">{tasks.length}</span>}
        actions={(
          <>
          {onCreate ? <Button size="sm" className="h-8 gap-1.5" onClick={onCreate}><Plus className="size-3.5" />{t('scheduled.management.create')}</Button> : null}
          <TooltipProvider><Tooltip><TooltipTrigger asChild><Button variant="ghost" size="icon" className="size-8" onClick={() => void loadTasks()} disabled={loading} aria-label={t('scheduled.management.refresh')}><RefreshCw className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('scheduled.management.refresh')}</TooltipContent></Tooltip></TooltipProvider>
          <Select value={workspaceFilter} onValueChange={setWorkspaceFilter}>
            <SelectTrigger size="sm" className={scheduledWorkspaceFilterTriggerClassName} aria-label={t('scheduled.management.workspaceFilter')}>
              <SelectValue>{workspaceFilter === 'all' ? t('scheduled.management.allWorkspaces') : workspaceFilter}</SelectValue>
            </SelectTrigger>
            <SelectContent align="end">
              <SelectItem value="all">{t('scheduled.management.allWorkspaces')}</SelectItem>
              {workspaces.map((workspace) => <SelectItem key={workspace} value={workspace}>{workspace}</SelectItem>)}
            </SelectContent>
          </Select>
          <div className="flex items-center gap-0.5" aria-label={t('scheduled.management.statusFilter')}>
            {([
              ['all', 'all'],
              ['enabled', 'enabled'],
              ['disabled', 'disabled'],
            ] as const).map(([value, labelKey]) => (
              <Button
                key={value}
                type="button"
                size="sm"
                variant={filter === value ? 'secondary' : 'ghost'}
                className="h-8 px-3 text-xs"
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {t(`scheduled.management.${labelKey}`)}
              </Button>
            ))}
          </div>
          </>
        )}
      />

      <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-6 pt-4">
        {loadError ? <p className="mb-3 text-sm text-destructive">{t('scheduled.management.loadFailed')}</p> : null}
        {actionError ? <p className="mb-3 text-sm text-destructive">{t('scheduled.management.actionFailed')}</p> : null}
        {loading && tasks.length === 0 ? <div className="border-y border-border/60 py-12 text-center text-sm text-muted-foreground">{t('scheduled.management.loading')}</div> : null}
        {!loading && !loadError && visibleTasks.length === 0 ? <div className="border-y border-border/60 py-14 text-center text-sm text-muted-foreground">{t('scheduled.management.empty')}</div> : null}
        {visibleTasks.length > 0 ? (
          <section className="w-full min-w-0">
          <div className="hidden grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,1fr)_2.75rem_2rem] items-center gap-4 border-b border-border/60 px-3 pb-3 text-xs text-muted-foreground md:grid">
            <span>{t('scheduled.management.columns.task')}</span><span>{t('scheduled.management.columns.schedule')}</span><span>{t('scheduled.management.columns.next')}</span><span>{t('scheduled.management.columns.recent')}</span><span>{t('scheduled.management.columns.enabled')}</span><span />
          </div>
          <div className="divide-y divide-border/60">
            {visibleTasks.map((task) => (
              <div
                key={task.id}
                className="grid cursor-pointer grid-cols-[minmax(0,1fr)_2.75rem_2rem] items-center gap-3 px-3 py-4 transition-colors hover:bg-muted/30 md:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,1fr)_2.75rem_2rem] md:gap-4"
                onClick={() => onOpenDetail?.(task)}
              >
                <div className="flex min-w-0 items-center gap-3">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-foreground/10 text-foreground"><AlarmClock className="size-4" /></span>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{task.title || t('scheduled.unnamed')}</div>
                    <div className="mt-1 truncate text-xs text-muted-foreground">{modeLabels[task.mode] ?? task.mode}{task.mode === 'direct' ? ` · ${t(`scheduled.session.${task.sessionPolicy}`)}` : ''}<span className="md:hidden"> · {formatScheduledSchedule(t, task.schedule)}</span></div>
                  </div>
                </div>
                <div className="hidden min-w-0 text-xs md:block">
                  <div className="truncate font-medium text-foreground">{formatScheduledSchedule(t, task.schedule)}</div>
                  <div className="mt-1 truncate text-muted-foreground">{scheduledScheduleTimezone(task.schedule)}</div>
                </div>
                <div className="hidden min-w-0 text-xs md:block">
                  <div className="truncate font-medium text-foreground">{task.enabled ? (formatTimestamp(task.nextAt) || t('scheduled.management.completed')) : t('scheduled.management.disabled')}</div>
                  <div className="mt-1 text-muted-foreground">{task.enabled ? t('scheduled.management.waiting') : t('scheduled.management.taskDisabled')}</div>
                </div>
                <div className="hidden min-w-0 text-xs md:block">
                  <div className="truncate font-medium text-foreground">{task.lastTriggerAt ? formatTimestamp(task.lastTriggerAt) : t('scheduled.neverRun')}</div>
                  <div className="mt-1 truncate text-muted-foreground">{task.lastTriggerStatus === 'skipped' ? t('scheduled.management.queueSkipped') : scheduledTaskStatusLabel(t, task.status)}</div>
                </div>
                <div onClick={(e) => e.stopPropagation()}>
                  <Switch checked={task.enabled} disabled={Boolean(pendingTaskActions[task.id])} onCheckedChange={(enabled) => void updateEnabled(task, enabled)} aria-label={t(task.enabled ? 'scheduled.management.disableAria' : 'scheduled.management.enableAria')} />
                </div>
                <div onClick={(e) => e.stopPropagation()}>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" className="size-8" aria-label={t('scheduled.management.more')}><MoreHorizontal className="size-4" /></Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={() => onOpenDetail?.(task)}><MoreHorizontal className="size-4" />{t('scheduled.management.detail')}</DropdownMenuItem>
                      <DropdownMenuItem disabled={Boolean(pendingTaskActions[task.id])} onClick={() => void runNow(task)}><Play className="size-4" />{t('scheduled.management.runNow')}</DropdownMenuItem>
                      <DropdownMenuItem onClick={() => void openEdit(task)} disabled={editLoading || Boolean(pendingTaskActions[task.id])}><Pencil className="size-4" />{t('scheduled.management.edit')}</DropdownMenuItem>
                      <DropdownMenuItem disabled={Boolean(pendingTaskActions[task.id])} onClick={() => void updateEnabled(task, !task.enabled)}>
                        {task.enabled ? <Pause className="size-4" /> : <Play className="size-4" />}
                        {t(task.enabled ? 'scheduled.management.disable' : 'scheduled.management.enable')}
                      </DropdownMenuItem>
                      <DropdownMenuItem disabled={Boolean(pendingTaskActions[task.id])} className="text-destructive focus:text-destructive" onClick={() => setDeleting(task)}><Trash2 className="size-4" />{t('scheduled.management.delete')}</DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
            ))}
          </div>
          </section>
        ) : null}
      </div>
      <Sheet open={Boolean(editing)} onOpenChange={(open) => { if (!open) setEditing(null); }}>
        <SheetContent className="gap-0 overflow-hidden p-0" resizeStorageKey="scheduled-task-management/edit" defaultSize={720} minSize={520} maxSize={960} closeLabel={t('common.close')}>
          <SheetTitle className="sr-only">{t('scheduled.dialog.title')}</SheetTitle>
          {editing ? (
            <ScheduledTaskDialog
              open
              presentation="workspace"
              onOpenChange={(open) => { if (!open) setEditing(null); }}
              allowContinuous={editing.definition.runMode === 'direct'}
              initialConfig={editConfig(editing.definition)}
              initialContent={editing.definition.content}
              showContent
              onSave={async (config, content) => {
                const definition = editing.definition;
                await updateScheduledTask({
                  scheduledTaskId: definition.scheduledTaskId,
                  projectId: definition.projectId,
                  expectedUpdatedAt: definition.expectedUpdatedAt,
                  content: content ?? definition.content,
                  runMode: definition.runMode,
                  workflowTemplateId: definition.workflowTemplateId,
                  includeInterview: definition.includeInterview,
                  directConfig: definition.directConfig,
                  autoConfig: definition.autoConfig,
                  schedule: config.schedule,
                  overlapPolicy: config.overlapPolicy,
                  sessionPolicy: config.sessionPolicy,
                });
                setEditing(null);
                await loadTasks();
              }}
            />
          ) : null}
        </SheetContent>
      </Sheet>
      <AlertDialog open={Boolean(deleting)} onOpenChange={(open) => { if (!open) setDeleting(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('scheduled.management.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('scheduled.management.deleteDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('scheduled.management.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={Boolean(deleting && pendingTaskActions[deleting.id])} className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={(event) => {
              event.preventDefault();
              if (!deleting) return;
              const task = deleting;
              setTaskPending(task.id, 'delete');
              setActionError(false);
              void deleteScheduledTask(task.projectId, task.id)
                .then(() => {
                  setTasks((current) => current.filter((item) => item.id !== task.id));
                  taskMutationGenerationRef.current += 1;
                  setDeleting(null);
                })
                .catch(() => setActionError(true))
                .finally(() => setTaskPending(task.id, null));
            }}>{t('scheduled.management.confirmDelete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}
