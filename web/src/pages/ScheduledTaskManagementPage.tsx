import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { AlarmClock, MoreHorizontal, Pause, Play, Pencil, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { deleteScheduledTask, getScheduledTask, listScheduledTasks, runScheduledTaskNow, setScheduledTaskEnabled, subscribeScheduledTaskUpdates, updateScheduledTask } from '@/api';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import { ScheduledTaskDialog, type ScheduledTaskInitialConfig } from '@/components/conversation/ScheduledTaskDialog';
import { formatScheduledSchedule, scheduledScheduleTimezone } from '@/lib/scheduled-task-formatting';
import type { ScheduledTaskEditVm, ScheduledTaskVm } from '@/types';

type StatusFilter = 'all' | 'running' | 'disabled';

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
  const [editing, setEditing] = useState<{ task: ScheduledTaskVm; definition: ScheduledTaskEditVm } | null>(null);
  const [editLoading, setEditLoading] = useState(false);
  const [deleting, setDeleting] = useState<ScheduledTaskVm | null>(null);

  const loadTasks = useCallback(() => {
    setLoading(true);
    return listScheduledTasks(null)
      .then(setTasks)
      .catch(() => setTasks([]))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    void loadTasks();
  }, [loadTasks]);

  // Local-merge: update only the matching task without a full reload.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledTaskUpdates((event) => {
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
    const matchesStatus = filter === 'disabled' ? !task.enabled : filter === 'running' ? task.enabled : true;
    const matchesWorkspace = workspaceFilter === 'all' || task.workspaceName === workspaceFilter;
    return matchesStatus && matchesWorkspace;
  }), [filter, tasks, workspaceFilter]);

  const updateEnabled = (task: ScheduledTaskVm, enabled: boolean) => {
    void setScheduledTaskEnabled(task.projectId, task.id, enabled).then((updated) => {
      setTasks((current) => current.map((item) => item.id === updated.id ? updated : item));
    });
  };

  const runNow = async (task: ScheduledTaskVm) => {
    await runScheduledTaskNow(task.projectId, task.id);
    await loadTasks();
  };

  const openEdit = async (task: ScheduledTaskVm) => {
    setEditLoading(true);
    try {
      const definition = await getScheduledTask(task.projectId, task.id);
      setEditing({ task, definition });
    } finally {
      setEditLoading(false);
    }
  };

  const editConfig = (definition: ScheduledTaskEditVm): ScheduledTaskInitialConfig => ({
    schedule: definition.schedule,
    overlapPolicy: definition.overlapPolicy,
    sessionPolicy: definition.sessionPolicy,
  });

  return (
    <main className="mx-auto flex h-full w-full max-w-6xl flex-col overflow-auto px-6 py-8">
      <header className="mb-7 flex flex-col items-start justify-between gap-4 md:flex-row md:items-end">
        <div className="flex items-center gap-3">
          <AlarmClock className="size-5 text-primary" />
          <div>
            <div className="flex items-center gap-2">
              <h1 className="whitespace-nowrap text-lg font-semibold tracking-tight">{t('scheduled.management.title')}</h1>
              <span className="text-sm text-muted-foreground">{tasks.length}</span>
            </div>
            <p className="mt-0.5 text-sm text-muted-foreground">{t('scheduled.management.subtitle')}</p>
          </div>
        </div>
        <div className="flex w-full flex-wrap items-center gap-2 md:w-auto">
          {onCreate ? <Button size="sm" className="h-8 gap-1.5" onClick={onCreate}><Plus className="size-3.5" />{t('scheduled.management.create')}</Button> : null}
          <Button variant="ghost" size="icon" className="size-8" onClick={() => void loadTasks()} disabled={loading} aria-label={t('scheduled.management.refresh')} title={t('scheduled.management.refresh')}><RefreshCw className="size-3.5" /></Button>
          <select
            value={workspaceFilter}
            onChange={(event) => setWorkspaceFilter(event.target.value)}
            className="h-8 rounded-md border border-border/70 bg-background px-2 text-xs text-foreground outline-none focus:ring-1 focus:ring-ring"
            aria-label={t('scheduled.management.workspaceFilter')}
          >
            <option value="all">{t('scheduled.management.allWorkspaces')}</option>
            {workspaces.map((workspace) => <option key={workspace} value={workspace}>{workspace}</option>)}
          </select>
          <div className="flex items-center rounded-md border border-border/70 p-0.5" aria-label={t('scheduled.management.statusFilter')}>
            {([
              ['all', 'all'],
              ['running', 'active'],
              ['disabled', 'disabled'],
            ] as const).map(([value, labelKey]) => (
              <button
                key={value}
                type="button"
                className={`rounded px-3 py-1.5 text-xs transition-colors ${filter === value ? 'bg-secondary text-foreground' : 'text-muted-foreground hover:text-foreground'}`}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {t(`scheduled.management.${labelKey}`)}
              </button>
            ))}
          </div>
        </div>
      </header>

      {loading ? <div className="border-y border-border/60 py-12 text-center text-sm text-muted-foreground">{t('scheduled.management.loading')}</div> : null}
      {!loading && visibleTasks.length === 0 ? <div className="border-y border-border/60 py-14 text-center text-sm text-muted-foreground">{t('scheduled.management.empty')}</div> : null}
      {!loading && visibleTasks.length > 0 ? (
        <section className="min-w-[980px]">
          <div className="grid grid-cols-[minmax(260px,1.35fr)_minmax(170px,1fr)_minmax(150px,0.9fr)_minmax(170px,1fr)_auto_auto] items-center gap-4 border-b border-border/60 px-3 pb-3 text-xs text-muted-foreground">
            <span>{t('scheduled.management.columns.task')}</span><span>{t('scheduled.management.columns.schedule')}</span><span>{t('scheduled.management.columns.next')}</span><span>{t('scheduled.management.columns.recent')}</span><span>{t('scheduled.management.columns.enabled')}</span><span />
          </div>
          <div className="divide-y divide-border/60">
            {visibleTasks.map((task) => (
              <div
                key={task.id}
                className="grid cursor-pointer grid-cols-[minmax(260px,1.35fr)_minmax(170px,1fr)_minmax(150px,0.9fr)_minmax(170px,1fr)_auto_auto] items-center gap-4 px-3 py-4 transition-colors hover:bg-muted/30"
                onClick={() => onOpenDetail?.(task)}
              >
                <div className="flex min-w-0 items-center gap-3">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary"><AlarmClock className="size-4" /></span>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{task.title || t('scheduled.unnamed')}</div>
                    <div className="mt-1 truncate text-xs text-muted-foreground">{modeLabels[task.mode] ?? task.mode}{task.mode === 'direct' ? ` · ${t(`scheduled.session.${task.sessionPolicy}`)}` : ''}</div>
                  </div>
                </div>
                <div className="min-w-0 text-xs">
                  <div className="truncate font-medium text-foreground">{formatScheduledSchedule(t, task.schedule)}</div>
                  <div className="mt-1 truncate text-muted-foreground">{scheduledScheduleTimezone(task.schedule)}</div>
                </div>
                <div className="min-w-0 text-xs">
                  <div className="truncate font-medium text-foreground">{task.enabled ? (formatTimestamp(task.nextAt) || t('scheduled.management.completed')) : t('scheduled.management.disabled')}</div>
                  <div className="mt-1 text-muted-foreground">{task.enabled ? t('scheduled.management.waiting') : t('scheduled.management.taskDisabled')}</div>
                </div>
                <div className="min-w-0 text-xs">
                  <div className="truncate font-medium text-foreground">{task.lastTriggerAt ? formatTimestamp(task.lastTriggerAt) : t('scheduled.neverRun')}</div>
                  <div className="mt-1 truncate text-muted-foreground">{task.lastTriggerStatus === 'skipped' ? t('scheduled.management.queueSkipped') : scheduledTaskStatusLabel(t, task.status)}</div>
                </div>
                <div onClick={(e) => e.stopPropagation()}>
                  <Switch checked={task.enabled} onCheckedChange={(enabled) => updateEnabled(task, enabled)} aria-label={t(task.enabled ? 'scheduled.management.disableAria' : 'scheduled.management.enableAria')} />
                </div>
                <div onClick={(e) => e.stopPropagation()}>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" className="size-8" aria-label={t('scheduled.management.more')}><MoreHorizontal className="size-4" /></Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={() => onOpenDetail?.(task)}><MoreHorizontal className="size-4" />{t('scheduled.management.detail')}</DropdownMenuItem>
                      <DropdownMenuItem onClick={() => void runNow(task)}><Play className="size-4" />{t('scheduled.management.runNow')}</DropdownMenuItem>
                      <DropdownMenuItem onClick={() => void openEdit(task)} disabled={editLoading}><Pencil className="size-4" />{t('scheduled.management.edit')}</DropdownMenuItem>
                      <DropdownMenuItem onClick={() => updateEnabled(task, !task.enabled)}>
                        {task.enabled ? <Pause className="size-4" /> : <Play className="size-4" />}
                        {t(task.enabled ? 'scheduled.management.disable' : 'scheduled.management.enable')}
                      </DropdownMenuItem>
                      <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => setDeleting(task)}><Trash2 className="size-4" />{t('scheduled.management.delete')}</DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
            ))}
          </div>
        </section>
      ) : null}
      <ScheduledTaskDialog
        open={Boolean(editing)}
        onOpenChange={(open) => { if (!open) setEditing(null); }}
        allowContinuous={editing?.definition.runMode === 'direct'}
        initialConfig={editing ? editConfig(editing.definition) : null}
        initialContent={editing?.definition.content}
        showContent
        onSave={async (config, content) => {
          if (!editing) return;
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
      <AlertDialog open={Boolean(deleting)} onOpenChange={(open) => { if (!open) setDeleting(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('scheduled.management.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('scheduled.management.deleteDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('scheduled.management.cancel')}</AlertDialogCancel>
            <AlertDialogAction className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={() => {
              if (!deleting) return;
              const task = deleting;
              setDeleting(null);
              void deleteScheduledTask(task.projectId, task.id).then(() => setTasks((current) => current.filter((item) => item.id !== task.id)));
            }}>{t('scheduled.management.confirmDelete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
