import { useCallback, useEffect, useMemo, useState } from 'react';
import { ArrowLeft, CheckCircle2, Clock3, ListChecks, MoreHorizontal, Pause, Play, Pencil, RefreshCw, RotateCw, Trash2, XCircle } from 'lucide-react';
import { deleteScheduledTask, getScheduledTask, getScheduledTaskDiagnostics, listScheduledTaskOccurrences, listScheduledTasks, runScheduledTaskNow, setScheduledTaskEnabled, subscribeScheduledOccurrenceUpdates, subscribeScheduledTaskUpdates, updateScheduledTask } from '@/api';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import { ScheduledTaskDialog, type ScheduledTaskConfig } from '@/components/conversation/ScheduledTaskDialog';
import { formatTimestamp, scheduledTaskStatusLabel } from './ScheduledTaskManagementPage';
import type { ScheduledOccurrenceVm, ScheduledTaskDiagnosticsVm, ScheduledTaskEditVm, ScheduledTaskVm } from '@/types';

const modeLabels: Record<string, string> = {
  direct: 'Direct',
  workflow: 'Workflow',
  auto: 'AUTO',
};

function occurrenceStatusLabel(status: string) {
  return ({ pending: '待执行', running: '执行中', retrying: '重试中', succeeded: '成功', failed: '失败', skipped: '已跳过', missed: '已错过', attention_required: '需要处理' } as Record<string, string>)[status] ?? status;
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

const errorCodeLabels: Record<string, string> = {
  SCHEDULED_PERMISSION_REQUIRED: '需要权限审批',
  SCHEDULED_USER_INPUT_REQUIRED: '需要用户输入',
  SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION: '前序运行需处理',
  SCHEDULED_QUEUE_BUSY: '队列繁忙已跳过',
  SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED: 'Agent 不支持无人值守模式',
  SCHEDULED_EXECUTION_FAILED: '执行失败',
  SCHEDULED_LEASE_LOST: '执行超时租约丢失',
};

function errorCodeLabel(code?: string | null) {
  if (!code) return '--';
  return errorCodeLabels[code] ?? code;
}

export function ScheduledTaskDetailPage({ projectId, scheduledTaskId, onBack }: { projectId: string; scheduledTaskId: string; onBack: () => void }) {
  const [task, setTask] = useState<ScheduledTaskVm | null>(null);
  const [diagnostics, setDiagnostics] = useState<ScheduledTaskDiagnosticsVm | null>(null);
  const [occurrences, setOccurrences] = useState<ScheduledOccurrenceVm[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [editing, setEditing] = useState<{ definition: ScheduledTaskEditVm } | null>(null);
  const [editLoading, setEditLoading] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const resolvedProjectId = task?.projectId ?? projectId;

  const loadDetail = useCallback(async (pid: string, sid: string) => {
    setLoading(true);
    setError(null);
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
        setTask(null);
        setError('未找到此定时任务');
        return;
      }
      setTask(found);
      const [history, nextDiagnostics] = await Promise.all([
        listScheduledTaskOccurrences(effectiveProjectId, sid, 50),
        getScheduledTaskDiagnostics(effectiveProjectId, sid),
      ]);
      setOccurrences(history);
      setDiagnostics(nextDiagnostics);
    } catch {
      setTask(null);
      setError('无法加载执行详情');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDetail(projectId, scheduledTaskId);
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
      if (resolvedProjectId) void loadDetail(resolvedProjectId, scheduledTaskId);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [loadDetail, resolvedProjectId, scheduledTaskId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeScheduledOccurrenceUpdates((event) => {
      if (event.scheduledTaskId !== scheduledTaskId) return;
      if (resolvedProjectId) void loadDetail(resolvedProjectId, scheduledTaskId);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, [loadDetail, resolvedProjectId, scheduledTaskId]);

  const updateEnabled = useCallback(async (enabled: boolean) => {
    if (!task) return;
    try {
      const updated = await setScheduledTaskEnabled(task.projectId, task.id, enabled);
      setTask(updated);
    } catch { /* keep previous state */ }
  }, [task]);

  const runNow = useCallback(async () => {
    if (!task) return;
    setRunning(true);
    setError(null);
    try {
      await runScheduledTaskNow(task.projectId, task.id);
      await loadDetail(task.projectId, task.id);
    } catch {
      setError('无法启动此定时任务');
    } finally {
      setRunning(false);
    }
  }, [loadDetail, task]);

  const openEdit = useCallback(async () => {
    if (!task) return;
    setEditLoading(true);
    try {
      const definition = await getScheduledTask(task.projectId, task.id);
      setEditing({ definition });
    } finally {
      setEditLoading(false);
    }
  }, [task]);

  const editConfig = useCallback((definition: ScheduledTaskEditVm): ScheduledTaskConfig => ({
    schedule: definition.schedule,
    overlapPolicy: definition.overlapPolicy,
    sessionPolicy: definition.sessionPolicy,
  }), []);

  if (loading && !task) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl items-center justify-center text-sm text-muted-foreground">
        正在加载...
      </main>
    );
  }

  if (error && !task) {
    return (
      <main className="mx-auto flex h-full w-full max-w-4xl flex-col gap-4 px-6 py-8">
        <Button variant="ghost" size="sm" className="w-fit gap-1.5" onClick={onBack}><ArrowLeft className="size-3.5" />返回定时任务</Button>
        <p className="text-sm text-destructive">{error}</p>
      </main>
    );
  }

  if (!task) return null;

  return (
    <main className="mx-auto flex h-full w-full max-w-4xl flex-col overflow-auto px-6 py-8">
      <header className="mb-6 flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <Button variant="ghost" size="icon" className="size-8 shrink-0" onClick={onBack} aria-label="返回定时任务" title="返回定时任务"><ArrowLeft className="size-4" /></Button>
          <div className="min-w-0">
            <h1 className="truncate text-lg font-semibold tracking-tight">{task.title}</h1>
            <p className="mt-0.5 truncate text-sm text-muted-foreground">{modeLabels[task.mode] ?? task.mode}{task.sessionPolicy === 'continuous' ? ' · 持续会话' : task.mode === 'direct' ? ' · 新会话' : ''}</p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button size="sm" variant="secondary" className="h-8 gap-1.5" onClick={() => void runNow()} disabled={running}>
            <Play className="size-3.5" />{running ? '启动中...' : '立即执行'}
          </Button>
          <Switch checked={task.enabled} onCheckedChange={(enabled) => void updateEnabled(enabled)} aria-label={task.enabled ? '停用定时任务' : '启用定时任务'} />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="size-8" aria-label="更多操作"><MoreHorizontal className="size-4" /></Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => void openEdit()} disabled={editLoading}><Pencil className="size-4" />编辑任务</DropdownMenuItem>
              <DropdownMenuItem onClick={() => void updateEnabled(!task.enabled)}>
                {task.enabled ? <Pause className="size-4" /> : <Play className="size-4" />}
                {task.enabled ? '停用任务' : '启用任务'}
              </DropdownMenuItem>
              <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => setDeleting(true)}><Trash2 className="size-4" />删除任务</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>

      {error ? <p className="mb-4 text-sm text-destructive">{error}</p> : null}

      <section className="border-y border-border/60 py-4">
        <div className="grid grid-cols-2 gap-x-6 gap-y-4 text-xs sm:grid-cols-4">
          <div><div className="text-muted-foreground">上轮状态</div><div className={`mt-1 font-medium ${occurrenceStatusClass(diagnostics?.lastStatus ?? task.lastTriggerStatus ?? '')}`}>{occurrenceStatusLabel(diagnostics?.lastStatus ?? task.lastTriggerStatus ?? 'pending')}</div></div>
          <div><div className="text-muted-foreground">执行次数</div><div className="mt-1 font-medium">{diagnostics?.runCount ?? 0}</div></div>
          <div><div className="text-muted-foreground">重试次数</div><div className="mt-1 font-medium">{diagnostics?.retryCount ?? 0}</div></div>
          <div><div className="text-muted-foreground">下次执行</div><div className="mt-1 font-medium">{task.enabled ? formatTimestamp(diagnostics?.nextAt ?? task.nextAt) : '已停用'}</div></div>
        </div>
      </section>

      <div className="mt-2 grid grid-cols-1 gap-x-6 gap-y-2 py-4 text-xs sm:grid-cols-2">
        <div><span className="text-muted-foreground">工作区 </span><span className="font-medium">{task.workspaceName}</span></div>
        <div><span className="text-muted-foreground">计划 </span><span className="font-medium">{task.scheduleLabel || task.schedule}</span></div>
        <div><span className="text-muted-foreground">时区 </span><span className="font-medium">{task.timezoneLabel}</span></div>
        <div><span className="text-muted-foreground">状态 </span><span className="font-medium">{scheduledTaskStatusLabel(task.status)}</span></div>
      </div>

      {diagnostics?.lastError ? <p className="mt-2 text-xs text-destructive">{errorCodeLabel(diagnostics.lastError)}</p> : null}

      <section className="mt-6" aria-label="Execution history">
        <div className="mb-2 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <ListChecks className="size-4 text-primary" />
            <h2 className="text-sm font-semibold">执行历史</h2>
          </div>
          <span className="text-xs text-muted-foreground">{occurrences.length}</span>
        </div>
        {occurrences.length === 0 ? (
          <div className="border-y border-border/60 py-8 text-center text-sm text-muted-foreground">暂无执行记录</div>
        ) : (
          <div className="divide-y divide-border/60 border-y border-border/60">
            {occurrences.map((occurrence) => {
              const StatusIcon = occurrenceStatusIcon(occurrence.status);
              return (
                <div key={occurrence.id} className="grid grid-cols-[minmax(150px,1fr)_minmax(110px,0.7fr)_minmax(100px,0.6fr)_minmax(150px,1fr)] items-center gap-4 px-3 py-3 text-xs">
                  <div><div className="font-medium">{formatTimestamp(occurrence.scheduledAt)}</div><div className="mt-1 text-muted-foreground">{occurrence.triggerKind === 'manual' ? '手动' : '计划'}</div></div>
                  <div className={`flex items-center gap-1.5 font-medium ${occurrenceStatusClass(occurrence.status)}`}><StatusIcon className={`size-3.5 ${occurrence.status === 'running' || occurrence.status === 'retrying' ? 'animate-spin' : ''}`} />{occurrenceStatusLabel(occurrence.status)}</div>
                  <div className="text-muted-foreground">第 {occurrence.attempt} 次</div>
                  <div className="truncate text-muted-foreground">{occurrence.errorCode ? errorCodeLabel(occurrence.errorCode) : (occurrence.runId ?? '--')}</div>
                </div>
              );
            })}
          </div>
        )}
      </section>

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
          await loadDetail(definition.projectId, scheduledTaskId);
        }}
      />

      <AlertDialog open={deleting} onOpenChange={(open) => { if (!open) setDeleting(false); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除定时任务？</AlertDialogTitle>
            <AlertDialogDescription>删除后不会再按计划执行，已经触发的会话不受影响。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={() => {
              if (!task) return;
              const t = task;
              setDeleting(false);
              void deleteScheduledTask(t.projectId, t.id).then(() => onBack());
            }}>删除</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </main>
  );
}
