import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { ArrowLeft, CheckCircle2, ChevronLeft, ChevronRight, Clock3, ExternalLink, ListChecks, MoreHorizontal, Pause, Play, Pencil, RotateCw, Trash2, XCircle } from 'lucide-react';
import { deleteScheduledTask, getScheduledTask, getScheduledTaskDiagnostics, listScheduledTaskOccurrences, listScheduledTasks, runScheduledTaskNow, setScheduledTaskEnabled, subscribeScheduledOccurrenceUpdates, subscribeScheduledTaskUpdates, updateScheduledTask } from '@/api';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { ScheduledTaskDialog, type ScheduledTaskInitialConfig } from '@/components/conversation/ScheduledTaskDialog';
import { formatTimestamp, scheduledTaskStatusLabel } from './ScheduledTaskManagementPage';
import { formatScheduledSchedule, scheduledScheduleTimezone } from '@/lib/scheduled-task-formatting';
import { scheduledOccurrenceTarget } from '@/lib/scheduled-task-navigation';
import { createScheduledTaskDetailRefreshCoordinator, type ScheduledTaskDetailRefreshCoordinator } from '@/lib/scheduled-task-detail-refresh';
import type { ConversationPage, ScheduledOccurrenceVm, ScheduledTaskDiagnosticsVm, ScheduledTaskEditVm, ScheduledTaskVm } from '@/types';

const modeLabels: Record<string, string> = {
  direct: 'Direct',
  workflow: 'Workflow',
  auto: 'AUTO',
};

const historyStatusOptions = ['pending', 'running', 'retrying', 'succeeded', 'failed', 'skipped', 'missed', 'attention_required'] as const;

function occurrenceStatusLabel(t: TFunction, status: string) {
  return t(`scheduled.status.${status}`, { defaultValue: status });
}

function occurrenceStatusClass(status: string) {
  if (status === 'succeeded') return 'text-emerald-600 dark:text-emerald-400';
  if (status === 'failed' || status === 'attention_required') return 'text-destructive';
  if (status === 'running' || status === 'retrying') return 'text-amber-600 dark:text-amber-400';
  return 'text-muted-foreground';
}

function occurrenceStatusIcon(status: string) {
  if (status === 'succeeded') return CheckCircle2;
  if (status === 'failed' || status === 'attention_required') return XCircle;
  if (status === 'running' || status === 'retrying') return RotateCw;
  return Clock3;
}

function errorCodeLabel(t: TFunction, code?: string | null) {
  if (!code) return '--';
  return t(`scheduled.errors.${code}`, { defaultValue: code });
}

interface DetailSnapshotRefreshRequest {
  projectId: string;
  scheduledTaskId: string;
  status: string;
}

export function ScheduledTaskDetailPage({ projectId, scheduledTaskId, onBack, onOpenOccurrence }: { projectId: string; scheduledTaskId: string; onBack: () => void; onOpenOccurrence?: (page: ConversationPage) => void }) {
  const { t } = useTranslation();
  const [task, setTask] = useState<ScheduledTaskVm | null>(null);
  const [diagnostics, setDiagnostics] = useState<ScheduledTaskDiagnosticsVm | null>(null);
  const [occurrences, setOccurrences] = useState<ScheduledOccurrenceVm[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [editing, setEditing] = useState<{ definition: ScheduledTaskEditVm } | null>(null);
  const [editLoading, setEditLoading] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [pendingAction, setPendingAction] = useState<'enable' | 'edit' | 'delete' | null>(null);
  const [statusFilter, setStatusFilter] = useState('all');
  const [historyCursorStack, setHistoryCursorStack] = useState<Array<string | null>>([null]);
  const [historyNextCursor, setHistoryNextCursor] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState(false);
  const snapshotRefreshCoordinatorRef = useRef<ScheduledTaskDetailRefreshCoordinator<DetailSnapshotRefreshRequest> | null>(null);
  const foregroundRequestInFlightRef = useRef(false);
  const pendingActionRef = useRef<'enable' | 'edit' | 'delete' | 'run' | null>(null);

  if (!snapshotRefreshCoordinatorRef.current) {
    snapshotRefreshCoordinatorRef.current = createScheduledTaskDetailRefreshCoordinator({
      load: async (request: DetailSnapshotRefreshRequest) => {
        const [page, nextDiagnostics] = await Promise.all([
          listScheduledTaskOccurrences(
            request.projectId,
            request.scheduledTaskId,
            null,
            request.status === 'all' ? null : request.status,
          ),
          getScheduledTaskDiagnostics(request.projectId, request.scheduledTaskId),
        ]);
        return { page, nextDiagnostics };
      },
      commit: ({ page, nextDiagnostics }) => {
        setOccurrences(page.items);
        setHistoryNextCursor(page.nextCursor ?? null);
        setDiagnostics(nextDiagnostics);
        setHistoryError(false);
      },
      fail: () => setHistoryError(true),
    });
  }

  const resolvedProjectId = task?.projectId ?? projectId;

  const requestSnapshotRefresh = useCallback((pid: string, sid: string, status: string) => {
    snapshotRefreshCoordinatorRef.current?.request({
      projectId: pid,
      scheduledTaskId: sid,
      status,
    });
  }, []);

  const loadDetail = useCallback(async (pid: string, sid: string) => {
    const coordinator = snapshotRefreshCoordinatorRef.current!;
    const generation = coordinator.beginForegroundRequest();
    foregroundRequestInFlightRef.current = true;
    setLoading(true);
    setError(null);
    setTask(null);
    setDiagnostics(null);
    setOccurrences([]);
    setHistoryError(false);
    setStatusFilter('all');
    setHistoryCursorStack([null]);
    setHistoryNextCursor(null);
    try {
      let found: ScheduledTaskVm | undefined;
      let effectiveProjectId = pid;
      if (!effectiveProjectId) {
        const all = await listScheduledTasks(null);
        found = all.find((item) => item.id === sid);
        effectiveProjectId = found?.projectId ?? '';
      } else {
        const all = await listScheduledTasks(null);
        found = all.find((item) => item.id === sid && item.projectId === effectiveProjectId) ?? all.find((item) => item.id === sid);
        effectiveProjectId = found?.projectId ?? effectiveProjectId;
      }
      if (!found) {
        if (!coordinator.isCurrent(generation)) return;
        setError(t('scheduled.detail.notFound'));
        return;
      }
      const [page, nextDiagnostics] = await Promise.all([
        listScheduledTaskOccurrences(effectiveProjectId, sid, null, null),
        getScheduledTaskDiagnostics(effectiveProjectId, sid),
      ]);
      if (!coordinator.isCurrent(generation)) return;
      setTask(found);
      setOccurrences(page.items);
      setHistoryNextCursor(page.nextCursor ?? null);
      setDiagnostics(nextDiagnostics);
    } catch {
      if (coordinator.isCurrent(generation)) setError(t('scheduled.detail.loadFailed'));
    } finally {
      if (coordinator.isCurrent(generation)) {
        foregroundRequestInFlightRef.current = false;
        setLoading(false);
      }
    }
  }, [t]);

  useEffect(() => {
    void loadDetail(projectId, scheduledTaskId);
    return () => {
      snapshotRefreshCoordinatorRef.current?.invalidate();
      foregroundRequestInFlightRef.current = false;
    };
  }, [loadDetail, projectId, scheduledTaskId]);

  // Subscribe only to events for this task
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledTaskUpdates((event) => {
      if (event.scheduledTaskId !== scheduledTaskId) return;
      if (event.task) {
        setTask((prev) => (prev ? { ...event.task!, workspaceName: prev.workspaceName } : event.task!));
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [scheduledTaskId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledOccurrenceUpdates((event) => {
      if (event.scheduledTaskId !== scheduledTaskId) return;
      if (!task || task.id !== scheduledTaskId || historyCursorStack.length !== 1) return;
      if (foregroundRequestInFlightRef.current) return;
      requestSnapshotRefresh(task.projectId, scheduledTaskId, statusFilter);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [historyCursorStack.length, requestSnapshotRefresh, scheduledTaskId, statusFilter, task]);

  const updateEnabled = useCallback(async (enabled: boolean) => {
    if (!task) return;
    if (pendingActionRef.current) return;
    pendingActionRef.current = 'enable';
    setPendingAction('enable');
    setError(null);
    try {
      const updated = await setScheduledTaskEnabled(task.projectId, task.id, enabled);
      setTask((current) => current ? { ...updated, workspaceName: current.workspaceName } : updated);
      requestSnapshotRefresh(task.projectId, task.id, statusFilter);
    } catch {
      setError(t('scheduled.detail.actionFailed'));
    } finally {
      pendingActionRef.current = null;
      setPendingAction(null);
    }
  }, [requestSnapshotRefresh, statusFilter, t, task]);

  const runNow = useCallback(async () => {
    if (!task || pendingActionRef.current) return;
    pendingActionRef.current = 'run';
    setRunning(true);
    setError(null);
    try {
      await runScheduledTaskNow(task.projectId, task.id);
      setHistoryCursorStack([null]);
      requestSnapshotRefresh(task.projectId, task.id, statusFilter);
    } catch {
      setError(t('scheduled.detail.runFailed'));
    } finally {
      pendingActionRef.current = null;
      setRunning(false);
    }
  }, [requestSnapshotRefresh, statusFilter, t, task]);

  const openEdit = useCallback(async () => {
    if (!task) return;
    if (pendingActionRef.current) return;
    pendingActionRef.current = 'edit';
    setPendingAction('edit');
    setError(null);
    setEditLoading(true);
    try {
      const definition = await getScheduledTask(task.projectId, task.id);
      setEditing({ definition });
    } catch {
      setError(t('scheduled.detail.actionFailed'));
    } finally {
      pendingActionRef.current = null;
      setEditLoading(false);
      setPendingAction(null);
    }
  }, [pendingAction, t, task]);

  const loadHistoryPage = useCallback(async (nextStack: Array<string | null>) => {
    if (!task) return;
    const coordinator = snapshotRefreshCoordinatorRef.current!;
    const generation = coordinator.beginForegroundRequest();
    foregroundRequestInFlightRef.current = true;
    setLoading(true);
    setHistoryError(false);
    try {
      const page = await listScheduledTaskOccurrences(
        task.projectId,
        task.id,
        nextStack.at(-1) ?? null,
        statusFilter === 'all' ? null : statusFilter,
      );
      if (!coordinator.isCurrent(generation)) return;
      setOccurrences(page.items);
      setHistoryNextCursor(page.nextCursor ?? null);
      setHistoryCursorStack(nextStack);
    } catch {
      if (coordinator.isCurrent(generation)) setHistoryError(true);
    } finally {
      if (coordinator.isCurrent(generation)) {
        foregroundRequestInFlightRef.current = false;
        setLoading(false);
      }
    }
  }, [statusFilter, task]);

  const changeStatusFilter = useCallback(async (status: string) => {
    setStatusFilter(status);
    setHistoryCursorStack([null]);
    if (!task) return;
    const coordinator = snapshotRefreshCoordinatorRef.current!;
    const generation = coordinator.beginForegroundRequest();
    foregroundRequestInFlightRef.current = true;
    setLoading(true);
    setHistoryError(false);
    try {
      const page = await listScheduledTaskOccurrences(
        task.projectId,
        task.id,
        null,
        status === 'all' ? null : status,
      );
      if (!coordinator.isCurrent(generation)) return;
      setOccurrences(page.items);
      setHistoryNextCursor(page.nextCursor ?? null);
    } catch {
      if (coordinator.isCurrent(generation)) setHistoryError(true);
    } finally {
      if (coordinator.isCurrent(generation)) {
        foregroundRequestInFlightRef.current = false;
        setLoading(false);
      }
    }
  }, [task]);

  const editConfig = useCallback((definition: ScheduledTaskEditVm): ScheduledTaskInitialConfig => ({
    schedule: definition.schedule,
    overlapPolicy: definition.overlapPolicy,
    sessionPolicy: definition.sessionPolicy,
  }), []);

  if (loading && !task) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl items-center justify-center text-sm text-muted-foreground">
        {t('scheduled.detail.loading')}
      </main>
    );
  }

  if (error && !task) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl flex-col gap-4 px-6 py-8">
        <Button variant="ghost" size="sm" className="w-fit gap-1.5" onClick={onBack}><ArrowLeft className="size-3.5" />{t('scheduled.detail.back')}</Button>
        <p className="text-sm text-destructive">{error}</p>
      </main>
    );
  }

  if (!task) return null;

  return (
    <main className="mx-auto flex h-full w-full max-w-4xl flex-col overflow-auto overflow-x-hidden px-4 py-6 sm:px-6 sm:py-8">
      <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <Tooltip>
            <TooltipTrigger asChild><Button variant="ghost" size="icon" className="size-8 shrink-0" onClick={onBack} aria-label={t('scheduled.detail.back')}><ArrowLeft className="size-4" /></Button></TooltipTrigger>
            <TooltipContent>{t('scheduled.detail.back')}</TooltipContent>
          </Tooltip>
          <div className="min-w-0">
            <h1 className="truncate text-lg font-semibold tracking-tight">{task.title || t('scheduled.unnamed')}</h1>
            <p className="mt-0.5 truncate text-sm text-muted-foreground">{modeLabels[task.mode] ?? task.mode}{task.mode === 'direct' ? ` · ${t(`scheduled.session.${task.sessionPolicy}`)}` : ''}</p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="secondary" className="h-8 gap-1.5" onClick={() => void runNow()} disabled={running}>
            <Play className="size-3.5" />{t(running ? 'scheduled.detail.starting' : 'scheduled.detail.runNow')}
          </Button>
          <Switch checked={task.enabled} disabled={Boolean(pendingAction)} onCheckedChange={(enabled) => void updateEnabled(enabled)} aria-label={t(task.enabled ? 'scheduled.management.disableAria' : 'scheduled.management.enableAria')} />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="size-8" aria-label={t('scheduled.management.more')}><MoreHorizontal className="size-4" /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => void openEdit()} disabled={editLoading}><Pencil className="size-4" />{t('scheduled.management.edit')}</DropdownMenuItem>
              <DropdownMenuItem disabled={Boolean(pendingAction)} onClick={() => void updateEnabled(!task.enabled)}>
                {task.enabled ? <Pause className="size-4" /> : <Play className="size-4" />}
                {t(task.enabled ? 'scheduled.management.disable' : 'scheduled.management.enable')}
              </DropdownMenuItem>
              <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => setDeleting(true)}><Trash2 className="size-4" />{t('scheduled.management.delete')}</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>

      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}

      <section className="border-y border-border/60 py-4">
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 text-xs sm:grid-cols-4">
          <div><div className="text-muted-foreground">{t('scheduled.detail.previousStatus')}</div><div className={`mt-1 font-medium ${occurrenceStatusClass(diagnostics?.lastStatus ?? task.lastTriggerStatus ?? '')}`}>{occurrenceStatusLabel(t, diagnostics?.lastStatus ?? task.lastTriggerStatus ?? 'pending')}</div></div>
          <div><div className="text-muted-foreground">{t('scheduled.detail.runs')}</div><div className="mt-1 font-medium">{diagnostics?.runCount ?? 0}</div></div>
          <div><div className="text-muted-foreground">{t('scheduled.detail.retries')}</div><div className="mt-1 font-medium">{diagnostics?.retryCount ?? 0}</div></div>
          <div><div className="text-muted-foreground">{t('scheduled.detail.next')}</div><div className="mt-1 font-medium">{task.enabled ? formatTimestamp(diagnostics?.nextAt ?? task.nextAt) : t('scheduled.management.disabled')}</div></div>
        </div>
      </section>

      <div className="mt-2 grid grid-cols-1 gap-x-6 gap-y-2 py-4 text-xs sm:grid-cols-2">
        <div><span className="text-muted-foreground">{t('scheduled.detail.workspace')} </span><span className="font-medium">{task.workspaceName}</span></div>
        <div><span className="text-muted-foreground">{t('scheduled.detail.schedule')} </span><span className="font-medium">{formatScheduledSchedule(t, task.schedule)}</span></div>
        <div><span className="text-muted-foreground">{t('scheduled.detail.timezone')} </span><span className="font-medium">{scheduledScheduleTimezone(task.schedule)}</span></div>
        <div><span className="text-muted-foreground">{t('scheduled.detail.status')} </span><span className="font-medium">{scheduledTaskStatusLabel(t, task.status)}</span></div>
      </div>

      {diagnostics?.lastError ? <p className="mt-2 text-xs text-destructive">{errorCodeLabel(t, diagnostics.lastError)}</p> : null}

      <section className="mt-6" aria-label="Execution history">
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <ListChecks className="size-4 text-foreground" />
            <h2 className="text-sm font-semibold">{t('scheduled.detail.history')}</h2>
          </div>
          <div className="flex items-center gap-2">
            <Select value={statusFilter} onValueChange={(value) => void changeStatusFilter(value)}>
              <SelectTrigger className="h-8 w-36 text-xs" aria-label={t('scheduled.detail.filter')}><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="all">{t('scheduled.detail.allStatuses')}</SelectItem>{historyStatusOptions.map((status) => <SelectItem key={status} value={status}>{occurrenceStatusLabel(t, status)}</SelectItem>)}</SelectContent>
            </Select>
            <span className="text-xs text-muted-foreground">{occurrences.length}</span>
          </div>
        </div>
        {historyError ? <p className="mb-2 text-xs text-destructive">{t('scheduled.detail.historyLoadFailed')}</p> : null}
        {occurrences.length === 0 ? (
          <div className="border-y border-border/60 py-8 text-center text-sm text-muted-foreground">{t('scheduled.detail.noHistory')}</div>
        ) : (
          <div className="divide-y divide-border/60 border-y border-border/60">
            {occurrences.map((occurrence) => {
              const StatusIcon = occurrenceStatusIcon(occurrence.status);
              const target = scheduledOccurrenceTarget(resolvedProjectId, occurrence);
              return (
                <div key={occurrence.id} className="grid min-w-0 grid-cols-1 gap-2 px-3 py-3 text-xs sm:grid-cols-[minmax(0,1fr)_minmax(110px,0.7fr)_minmax(100px,0.6fr)_minmax(0,1fr)] sm:items-center sm:gap-4">
                  <div><div className="font-medium">{formatTimestamp(occurrence.scheduledAt)}</div><div className="mt-1 text-muted-foreground">{t(`scheduled.trigger.${occurrence.triggerKind}`, { defaultValue: occurrence.triggerKind })}</div></div>
                  <div className={`flex items-center gap-1.5 font-medium ${occurrenceStatusClass(occurrence.status)}`}><StatusIcon className={`size-3.5 ${occurrence.status === 'running' || occurrence.status === 'retrying' ? 'animate-spin' : ''}`} />{occurrenceStatusLabel(t, occurrence.status)}</div>
                  <div className="text-muted-foreground">{t('scheduled.detail.attempt', { count: occurrence.attempt })}</div>
                  <div className="flex min-w-0 items-center gap-2"><span className="truncate text-muted-foreground">{occurrence.errorCode ? errorCodeLabel(t, occurrence.errorCode) : (occurrence.runId ?? '--')}</span>{target && onOpenOccurrence ? <Tooltip><TooltipTrigger asChild><Button variant="ghost" size="icon" className="size-7 shrink-0" onClick={() => onOpenOccurrence(target)} aria-label={t('scheduled.detail.openRun')}><ExternalLink className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('scheduled.detail.openRun')}</TooltipContent></Tooltip> : null}</div>
                </div>
              );
            })}
          </div>
        )}
        <div className="mt-3 flex items-center justify-end gap-2">
          <Button variant="outline" size="sm" className="h-8 gap-1" disabled={loading || historyCursorStack.length === 1} onClick={() => void loadHistoryPage(historyCursorStack.slice(0, -1))}>
            <ChevronLeft className="size-3.5" />{t('scheduled.detail.previousPage')}
          </Button>
          <span className="min-w-8 text-center text-xs text-muted-foreground">{historyCursorStack.length}</span>
          <Button variant="outline" size="sm" className="h-8 gap-1" disabled={loading || !historyNextCursor} onClick={() => historyNextCursor && void loadHistoryPage([...historyCursorStack, historyNextCursor])}>
            {t('scheduled.detail.nextPage')}<ChevronRight className="size-3.5" />
          </Button>
        </div>
      </section>

      <Sheet open={Boolean(editing)} onOpenChange={(open) => { if (!open) setEditing(null); }}>
        <SheetContent className="gap-0 overflow-hidden p-0" resizeStorageKey="scheduled-task-detail/edit" defaultSize={720} minSize={520} maxSize={960} closeLabel={t('common.close')}>
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
                  includeOptionalEntry: definition.includeOptionalEntry,
                  directConfig: definition.directConfig,
                  autoConfig: definition.autoConfig,
                  schedule: config.schedule,
                  overlapPolicy: config.overlapPolicy,
                  sessionPolicy: config.sessionPolicy,
                });
                setEditing(null);
                await loadDetail(definition.projectId, scheduledTaskId);
              }}
            />
          ) : null}
        </SheetContent>
      </Sheet>

      <AlertDialog open={deleting} onOpenChange={(open) => { if (!open) setDeleting(false); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('scheduled.management.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('scheduled.management.deleteDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('scheduled.management.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={pendingAction === 'delete'} className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={(event) => {
              event.preventDefault();
              if (!task) return;
              const currentTask = task;
              if (pendingActionRef.current) return;
              pendingActionRef.current = 'delete';
              setPendingAction('delete');
              setError(null);
              void deleteScheduledTask(currentTask.projectId, currentTask.id)
                .then(() => { setDeleting(false); onBack(); })
                .catch(() => setError(t('scheduled.detail.actionFailed')))
                .finally(() => {
                  pendingActionRef.current = null;
                  setPendingAction(null);
                });
            }}>{t('scheduled.management.confirmDelete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
