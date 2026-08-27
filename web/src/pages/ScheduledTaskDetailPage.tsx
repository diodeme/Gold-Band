import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { ArrowLeft, ChevronLeft, ChevronRight, ExternalLink, ListChecks, MoreHorizontal, Pause, Play, Pencil, Trash2 } from 'lucide-react';
import { deleteScheduledExecutionHistory, deleteScheduledTask, getScheduledTask, getScheduledTaskDiagnostics, listScheduledExecutionHistory, listScheduledTasks, runScheduledTaskNow, setScheduledTaskEnabled, subscribeScheduledOccurrenceUpdates, subscribeScheduledTaskUpdates, updateScheduledTask } from '@/api';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import { Checkbox } from '@/components/ui/checkbox';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { ScheduledTaskDialog, type ScheduledTaskInitialConfig } from '@/components/conversation/ScheduledTaskDialog';
import { formatTimestamp, scheduledTaskStatusLabel } from './ScheduledTaskManagementPage';
import { formatScheduledSchedule, scheduledScheduleTimezone } from '@/lib/scheduled-task-formatting';
import { scheduledHistoryTarget } from '@/lib/scheduled-task-navigation';
import { createScheduledTaskDetailRefreshCoordinator, type ScheduledTaskDetailRefreshCoordinator } from '@/lib/scheduled-task-detail-refresh';
import { displayAppError, displayStatus } from '@/i18n';
import type { ConversationPage, ScheduledExecutionHistoryVm, ScheduledTaskDiagnosticsVm, ScheduledTaskEditVm, ScheduledTaskVm } from '@/types';

const modeLabels: Record<string, string> = {
  direct: 'Direct',
  workflow: 'Workflow',
  auto: 'AUTO',
};

function occurrenceStatusLabel(t: TFunction, status: string) {
  return t(`scheduled.status.${status}`, { defaultValue: status });
}

function occurrenceStatusClass(status: string) {
  if (status === 'succeeded') return 'text-emerald-600 dark:text-emerald-400';
  if (status === 'failed' || status === 'attention_required') return 'text-destructive';
  if (status === 'running' || status === 'retrying') return 'text-amber-600 dark:text-amber-400';
  return 'text-muted-foreground';
}

function errorCodeLabel(t: TFunction, code?: string | null) {
  if (!code) return '--';
  return t(`scheduled.errors.${code}`, { defaultValue: code });
}

function executionHistoryStatusLabel(t: TFunction, item: ScheduledExecutionHistoryVm) {
  return item.run
    ? displayStatus(t, item.run.status)
    : t(`scheduled.detail.historyAvailability.${item.availability}`);
}

interface DetailSnapshotRefreshRequest {
  projectId: string;
  scheduledTaskId: string;
}

interface HistoryDeleteState {
  status: 'stopping' | 'deleting' | 'failed';
  code?: string | null;
  params?: Record<string, unknown>;
}

type HistoryPageLocation =
  | { kind: 'latest' }
  | { kind: 'anchor'; taskId: string; runId: string }
  | { kind: 'cursor'; cursor: string };

function initialHistoryLocations(taskId?: string, runId?: string): HistoryPageLocation[] {
  return taskId && runId
    ? [{ kind: 'latest' }, { kind: 'anchor', taskId, runId }]
    : [{ kind: 'latest' }];
}

function executionHistoryKey(item: Pick<ScheduledExecutionHistoryVm, 'projectId' | 'scheduledTaskId' | 'taskId' | 'runId'>) {
  return `${item.projectId}\0${item.scheduledTaskId}\0${item.taskId}\0${item.runId}`;
}

export function ScheduledTaskDetailPage({ projectId, scheduledTaskId, taskId, runId, occurrenceId, onBack, onOpenOccurrence }: { projectId: string; scheduledTaskId: string; taskId?: string; runId?: string; occurrenceId?: string; onBack: () => void; onOpenOccurrence?: (page: ConversationPage) => void }) {
  const { t } = useTranslation();
  const [task, setTask] = useState<ScheduledTaskVm | null>(null);
  const [definitionLoading, setDefinitionLoading] = useState(true);
  const [diagnostics, setDiagnostics] = useState<ScheduledTaskDiagnosticsVm | null>(null);
  const [history, setHistory] = useState<ScheduledExecutionHistoryVm[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [editing, setEditing] = useState<{ definition: ScheduledTaskEditVm } | null>(null);
  const [editLoading, setEditLoading] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [pendingAction, setPendingAction] = useState<'enable' | 'edit' | 'delete' | null>(null);
  const [historyLocationStack, setHistoryLocationStack] = useState<HistoryPageLocation[]>(() => initialHistoryLocations(taskId, runId));
  const [historyNextCursor, setHistoryNextCursor] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState(false);
  const [selectedRuns, setSelectedRuns] = useState<Set<string>>(new Set());
  const [historyDeleteStates, setHistoryDeleteStates] = useState<Record<string, HistoryDeleteState>>({});
  const [historyDeletePending, setHistoryDeletePending] = useState(false);
  const snapshotRefreshCoordinatorRef = useRef<ScheduledTaskDetailRefreshCoordinator<DetailSnapshotRefreshRequest> | null>(null);
  const historyMutationGenerationRef = useRef(0);
  const historyDeleteInFlightRef = useRef(false);
  const foregroundRequestInFlightRef = useRef(false);
  const definitionRequestGenerationRef = useRef(0);
  const pendingActionRef = useRef<'enable' | 'edit' | 'delete' | 'run' | null>(null);

  if (!snapshotRefreshCoordinatorRef.current) {
    snapshotRefreshCoordinatorRef.current = createScheduledTaskDetailRefreshCoordinator({
      load: async (request: DetailSnapshotRefreshRequest) => {
        const [page, nextDiagnostics] = await Promise.all([
          listScheduledExecutionHistory(request.projectId, request.scheduledTaskId, null),
          getScheduledTaskDiagnostics(request.projectId, request.scheduledTaskId).catch(() => null),
        ]);
        return { page, nextDiagnostics };
      },
      commit: ({ page, nextDiagnostics }) => {
        setHistory(page.items);
        setHistoryNextCursor(page.nextCursor ?? null);
        setDiagnostics(nextDiagnostics);
        setHistoryError(false);
      },
      fail: () => setHistoryError(true),
    });
  }

  const requestSnapshotRefresh = useCallback((pid: string, sid: string) => {
    snapshotRefreshCoordinatorRef.current?.request({
      projectId: pid,
      scheduledTaskId: sid,
    });
  }, []);

  const loadDetail = useCallback((pid: string, sid: string) => {
    const coordinator = snapshotRefreshCoordinatorRef.current!;
    const generation = coordinator.beginForegroundRequest();
    const definitionGeneration = ++definitionRequestGenerationRef.current;
    const initialLocations = initialHistoryLocations(taskId, runId);
    const initialLocation = initialLocations.at(-1)!;
    foregroundRequestInFlightRef.current = true;
    setDefinitionLoading(true);
    setLoading(true);
    setError(null);
    setTask(null);
    setDiagnostics(null);
    setHistory([]);
    setHistoryError(false);
    setHistoryLocationStack(initialLocations);
    setHistoryNextCursor(null);
    setSelectedRuns(new Set());
    setHistoryDeleteStates({});
    setHistoryDeletePending(false);
    historyDeleteInFlightRef.current = false;
    historyMutationGenerationRef.current += 1;
    void listScheduledTasks(pid)
      .then((items) => {
        if (definitionGeneration !== definitionRequestGenerationRef.current) return;
        const resolvedTask = items.find((item) => item.id === sid);
        setTask(resolvedTask ?? null);
        if (resolvedTask) {
          void getScheduledTaskDiagnostics(pid, sid)
            .then((nextDiagnostics) => {
              if (definitionGeneration === definitionRequestGenerationRef.current) setDiagnostics(nextDiagnostics);
            })
            .catch(() => undefined);
        }
      })
      .catch(() => {
        if (definitionGeneration === definitionRequestGenerationRef.current) setError(t('scheduled.detail.loadFailed'));
      })
      .finally(() => {
        if (definitionGeneration === definitionRequestGenerationRef.current) setDefinitionLoading(false);
      });

    void listScheduledExecutionHistory(
      pid,
      sid,
      initialLocation.kind === 'cursor' ? initialLocation.cursor : null,
      initialLocation.kind === 'anchor' ? { taskId: initialLocation.taskId, runId: initialLocation.runId } : null,
    )
      .then((page) => {
        if (!coordinator.isCurrent(generation)) return;
        setHistory(page.items);
        setHistoryNextCursor(page.nextCursor ?? null);
      })
      .catch(() => {
        if (coordinator.isCurrent(generation)) setHistoryError(true);
      })
      .finally(() => {
        if (coordinator.isCurrent(generation)) {
          foregroundRequestInFlightRef.current = false;
          setLoading(false);
        }
      });
  }, [runId, t, taskId]);

  useEffect(() => {
    loadDetail(projectId, scheduledTaskId);
    return () => {
      snapshotRefreshCoordinatorRef.current?.invalidate();
      definitionRequestGenerationRef.current += 1;
      foregroundRequestInFlightRef.current = false;
    };
  }, [loadDetail, projectId, scheduledTaskId]);

  // Subscribe only to events for this task
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledTaskUpdates((event) => {
      if (event.projectId !== (task?.projectId ?? projectId) || event.scheduledTaskId !== scheduledTaskId) return;
      if (event.task) {
        setTask((prev) => (prev ? { ...event.task!, workspaceName: prev.workspaceName } : event.task!));
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [projectId, scheduledTaskId, task?.projectId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledOccurrenceUpdates((event) => {
      const effectiveProjectId = task?.projectId ?? projectId;
      if (!effectiveProjectId || event.projectId !== effectiveProjectId || event.scheduledTaskId !== scheduledTaskId) return;
      if (taskId && runId) return;
      if (historyLocationStack.length !== 1 || historyLocationStack[0]?.kind !== 'latest') return;
      if (foregroundRequestInFlightRef.current) return;
      requestSnapshotRefresh(effectiveProjectId, scheduledTaskId);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [historyLocationStack, projectId, requestSnapshotRefresh, runId, scheduledTaskId, task, taskId]);

  const updateEnabled = useCallback(async (enabled: boolean) => {
    if (!task) return;
    if (pendingActionRef.current) return;
    pendingActionRef.current = 'enable';
    setPendingAction('enable');
    setError(null);
    try {
      const updated = await setScheduledTaskEnabled(task.projectId, task.id, enabled);
      setTask((current) => current ? { ...updated, workspaceName: current.workspaceName } : updated);
      requestSnapshotRefresh(task.projectId, task.id);
    } catch {
      setError(t('scheduled.detail.actionFailed'));
    } finally {
      pendingActionRef.current = null;
      setPendingAction(null);
    }
  }, [requestSnapshotRefresh, t, task]);

  const runNow = useCallback(async () => {
    if (!task || pendingActionRef.current) return;
    pendingActionRef.current = 'run';
    setRunning(true);
    setError(null);
    try {
      await runScheduledTaskNow(task.projectId, task.id);
      setHistoryLocationStack([{ kind: 'latest' }]);
      requestSnapshotRefresh(task.projectId, task.id);
    } catch {
      setError(t('scheduled.detail.runFailed'));
    } finally {
      pendingActionRef.current = null;
      setRunning(false);
    }
  }, [requestSnapshotRefresh, t, task]);

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

  const loadHistoryPage = useCallback(async (nextStack: HistoryPageLocation[]) => {
    if (loading || historyDeleteInFlightRef.current) return;
    const location = nextStack.at(-1);
    if (!location) return;
    const coordinator = snapshotRefreshCoordinatorRef.current!;
    const generation = coordinator.beginForegroundRequest();
    foregroundRequestInFlightRef.current = true;
    setLoading(true);
    setHistoryError(false);
    setSelectedRuns(new Set());
    setHistoryDeleteStates({});
    historyMutationGenerationRef.current += 1;
    try {
      const page = await listScheduledExecutionHistory(
        projectId,
        scheduledTaskId,
        location.kind === 'cursor' ? location.cursor : null,
        location.kind === 'anchor' ? { taskId: location.taskId, runId: location.runId } : null,
      );
      if (!coordinator.isCurrent(generation)) return;
      setHistory(page.items);
      setHistoryNextCursor(page.nextCursor ?? null);
      setHistoryLocationStack(nextStack);
    } catch {
      if (coordinator.isCurrent(generation)) setHistoryError(true);
    } finally {
      if (coordinator.isCurrent(generation)) {
        foregroundRequestInFlightRef.current = false;
        setLoading(false);
      }
    }
  }, [loading, projectId, scheduledTaskId]);

  const deleteSelectedHistory = useCallback(async () => {
    if (loading || historyDeleteInFlightRef.current) return;
    const candidates = history.filter((item) => selectedRuns.has(executionHistoryKey(item)));
    if (!candidates.length) return;
    const generation = ++historyMutationGenerationRef.current;
    historyDeleteInFlightRef.current = true;
    setHistoryDeletePending(true);
    setHistoryDeleteStates((current) => ({ ...current, ...Object.fromEntries(candidates.map((item) => [executionHistoryKey(item), { status: 'deleting' as const }])) }));
    try {
      const results = await deleteScheduledExecutionHistory(candidates.map((item) => ({ projectId: item.projectId, scheduledTaskId: item.scheduledTaskId, taskId: item.taskId, runId: item.runId })));
      if (generation !== historyMutationGenerationRef.current) return;
      const completed = new Set(results.filter((result) => result.status === 'completed').map(executionHistoryKey));
      setHistory((current) => current.filter((item) => !completed.has(executionHistoryKey(item))));
      setSelectedRuns((current) => new Set([...current].filter((key) => !completed.has(key))));
      setHistoryDeleteStates((current) => {
        const next = { ...current };
        for (const result of results) {
          const key = executionHistoryKey(result);
          if (result.status === 'completed') delete next[key];
          else next[key] = {
            status: result.status === 'failed' ? 'failed' : result.status === 'stopping' ? 'stopping' : 'deleting',
            code: result.code,
            params: result.params,
          };
        }
        return next;
      });
    } catch {
      if (generation === historyMutationGenerationRef.current) {
        setHistoryDeleteStates((current) => ({ ...current, ...Object.fromEntries(candidates.map((item) => [executionHistoryKey(item), { status: 'failed' as const }])) }));
      }
    } finally {
      if (generation === historyMutationGenerationRef.current) {
        historyDeleteInFlightRef.current = false;
        setHistoryDeletePending(false);
      }
    }
  }, [history, loading, selectedRuns]);

  const editConfig = useCallback((definition: ScheduledTaskEditVm): ScheduledTaskInitialConfig => ({
    schedule: definition.schedule,
    overlapPolicy: definition.overlapPolicy,
    sessionPolicy: definition.sessionPolicy,
  }), []);

  if (loading && definitionLoading && !task) {
    return (
      <main className="mx-auto h-full w-full max-w-4xl px-4 py-6 sm:px-6 sm:py-8" aria-busy="true">
        <div className="space-y-4"><div className="h-6 w-48 animate-pulse rounded bg-muted" /><div className="h-24 animate-pulse rounded bg-muted" /></div>
      </main>
    );
  }

  if (error && !task && historyError) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl flex-col gap-4 px-6 py-8">
        <Button variant="ghost" size="sm" className="w-fit gap-1.5" onClick={onBack}><ArrowLeft className="size-3.5" />{t('scheduled.detail.back')}</Button>
        <p className="text-sm text-destructive">{error}</p>
      </main>
    );
  }

  if (!task) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl flex-col overflow-auto px-4 py-6 sm:px-6 sm:py-8">
        <Button variant="ghost" size="sm" className="w-fit gap-1.5" onClick={onBack}><ArrowLeft className="size-3.5" />{t('scheduled.detail.back')}</Button>
        <p className={`mt-5 text-sm ${error ? 'text-destructive' : 'text-muted-foreground'}`}>{error ?? (definitionLoading ? t('scheduled.detail.loading') : t('scheduled.detail.deleted'))}</p>
        <RunHistorySection readOnly t={t} history={history} loading={loading} historyError={historyError} deletePending={historyDeletePending} locationStack={historyLocationStack} nextCursor={historyNextCursor} selectedRuns={selectedRuns} deleteStates={historyDeleteStates} focusedTaskId={taskId} focusedRunId={runId} occurrenceId={occurrenceId} onToggle={(key, checked) => setSelectedRuns((current) => { const next = new Set(current); if (checked) next.add(key); else next.delete(key); return next; })} onDelete={() => void deleteSelectedHistory()} onPage={(stack) => void loadHistoryPage(stack)} onOpen={onOpenOccurrence} />
      </main>
    );
  }

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

      <RunHistorySection t={t} history={history} loading={loading} historyError={historyError} deletePending={historyDeletePending} locationStack={historyLocationStack} nextCursor={historyNextCursor} selectedRuns={selectedRuns} deleteStates={historyDeleteStates} focusedTaskId={taskId} focusedRunId={runId} occurrenceId={occurrenceId} onToggle={(key, checked) => setSelectedRuns((current) => { const next = new Set(current); if (checked) next.add(key); else next.delete(key); return next; })} onDelete={() => void deleteSelectedHistory()} onPage={(stack) => void loadHistoryPage(stack)} onOpen={onOpenOccurrence} />

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

function RunHistorySection({ t, history, loading, historyError, deletePending, locationStack, nextCursor, selectedRuns, deleteStates, focusedTaskId, focusedRunId, occurrenceId, onToggle, onDelete, onPage, onOpen, readOnly = false }: {
  t: TFunction;
  history: ScheduledExecutionHistoryVm[];
  loading: boolean;
  historyError: boolean;
  deletePending: boolean;
  locationStack: HistoryPageLocation[];
  nextCursor: string | null;
  selectedRuns: Set<string>;
  deleteStates: Record<string, HistoryDeleteState>;
  focusedTaskId?: string;
  focusedRunId?: string;
  occurrenceId?: string;
  readOnly?: boolean;
  onToggle: (key: string, checked: boolean) => void;
  onDelete: () => void;
  onPage: (stack: HistoryPageLocation[]) => void;
  onOpen?: (page: ConversationPage) => void;
}) {
  const allSelected = history.length > 0 && history.every((item) => selectedRuns.has(executionHistoryKey(item)));
  const anchoredWindow = locationStack.some((location) => location.kind === 'anchor');
  return <section className="mt-6" aria-label={t('scheduled.detail.history')}>
    <div className="mb-2 flex items-center justify-between gap-2"><div className="flex items-center gap-2"><ListChecks className="size-4 text-foreground" /><h2 className="text-sm font-semibold">{t('scheduled.detail.history')}</h2></div>{readOnly ? null : <Button size="sm" variant="outline" disabled={loading || selectedRuns.size === 0 || deletePending} onClick={onDelete}><Trash2 className="mr-1 size-3.5" />{t('scheduled.detail.deleteSelected')}</Button>}</div>
    {historyError ? <p className="mb-2 text-xs text-destructive">{t('scheduled.detail.historyLoadFailed')}</p> : null}
    {loading && history.length === 0 ? <div className="border-y border-border/60 py-8 text-center text-sm text-muted-foreground" aria-busy="true">{t('scheduled.detail.loading')}</div> : history.length === 0 ? <div className="border-y border-border/60 py-8 text-center text-sm text-muted-foreground">{t('scheduled.detail.noHistory')}</div> : <div className="divide-y divide-border/60 border-y border-border/60">{readOnly ? null : <div className="flex items-center gap-3 px-3 py-2 text-xs text-muted-foreground"><Checkbox aria-label={t('scheduled.detail.selectAllRuns')} checked={allSelected} disabled={loading || deletePending} onCheckedChange={(checked) => history.forEach((item) => onToggle(executionHistoryKey(item), checked === true))} /><span>{history.length}</span></div>}{history.map((item) => { const key = executionHistoryKey(item); const state = deleteStates[key]; const focused = item.taskId === focusedTaskId && item.runId === focusedRunId; return <div key={key} data-focused={focused || undefined} className={`flex min-w-0 items-center gap-3 px-3 py-3 text-xs ${focused ? 'bg-accent/50' : ''}`}>{readOnly ? null : <Checkbox aria-label={t('scheduled.detail.selectRun', { summary: item.latestSummary })} checked={selectedRuns.has(key)} disabled={loading || deletePending || Boolean(state && state.status !== 'failed')} onCheckedChange={(checked) => onToggle(key, checked === true)} />}<button type="button" className="min-w-0 flex-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => onOpen?.(scheduledHistoryTarget(item, focused && occurrenceId ? occurrenceId : item.latestOccurrenceId))}><div className="flex min-w-0 items-center justify-between gap-3"><span className="truncate font-medium">{item.latestSummary}</span><span className="shrink-0 text-muted-foreground">{formatTimestamp(item.lastAcceptedAt)}</span></div><div className="mt-1 flex min-w-0 gap-3 text-muted-foreground"><span>{item.occurrenceCount}</span><span className="truncate">{executionHistoryStatusLabel(t, item)}</span>{item.error ? <span className="truncate text-destructive">{displayAppError(t, item.error)}</span> : null}{state ? <span className={state.status === 'failed' ? 'text-destructive' : ''}>{state.status === 'failed' ? state.code ? displayAppError(t, { code: state.code, params: state.params ?? {} }) : t('scheduled.detail.actionFailed') : t('scheduled.detail.deleting')}</span> : null}</div></button><Tooltip><TooltipTrigger asChild><Button variant="ghost" size="icon" className="size-7 shrink-0" onClick={() => onOpen?.({ kind: 'conversation-run', projectId: item.projectId, taskId: item.taskId, runId: item.runId })} aria-label={t('scheduled.detail.openRun')}><ExternalLink className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('scheduled.detail.openRun')}</TooltipContent></Tooltip></div>; })}</div>}
    <div className="mt-3 flex items-center justify-end gap-2"><Button variant="outline" size="sm" className="h-8 gap-1" disabled={loading || deletePending || locationStack.length === 1} onClick={() => onPage(locationStack.slice(0, -1))}><ChevronLeft className="size-3.5" />{t('scheduled.detail.previousPage')}</Button><span className="min-w-8 text-center text-xs text-muted-foreground">{anchoredWindow ? t('scheduled.detail.locatedHistory') : locationStack.length}</span><Button variant="outline" size="sm" className="h-8 gap-1" disabled={loading || deletePending || !nextCursor} onClick={() => nextCursor && onPage([...locationStack, { kind: 'cursor', cursor: nextCursor }])}>{t('scheduled.detail.nextPage')}<ChevronRight className="size-3.5" /></Button></div>
  </section>;
}
