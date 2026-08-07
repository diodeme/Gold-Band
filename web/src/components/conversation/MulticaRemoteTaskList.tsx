import { Ban, ChevronDown, Loader2, Play, Plus, RotateCw, Wifi, WifiOff } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  cancelMulticaTask,
  claimMulticaTask,
  connectMultica,
  getMulticaTasks,
  rerunMulticaTask,
  subscribeMulticaSettingsUpdates,
  subscribeMulticaTaskUpdates,
} from '../../api';
import { MulticaAddWorkspaceDialog } from './MulticaAddWorkspaceDialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { useConversationComposerDraft } from '@/lib/conversation-composer-draft';
import { formatLocalDateTime } from '@/lib/datetime';
import { displayAppError } from '../../i18n';
import type {
  MulticaWorkspaceRefVm,
  RemoteConversationSidebarVm,
  RemoteTaskVm,
} from '../../types';

interface MulticaRemoteTaskListProps {
  /// 直达指定 run 的会话页（复用本地侧栏 onSelectRun，与 createConversationRun 同路径）。
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  /// 远程任务「点击执行」后进入会话准备页（与本地工作空间「+」同一回调：预填后落 conversation-home）。
  onNewConversationInWorkspace: (projectId: string) => void;
}

const STATUS_VARIANT: Record<string, 'outline' | 'secondary' | 'default' | 'destructive'> = {
  queued: 'outline',
  running: 'secondary',
  completed: 'default',
  failed: 'destructive',
};

export function MulticaRemoteTaskList({ onSelectRun, onNewConversationInWorkspace }: MulticaRemoteTaskListProps) {
  const { t } = useTranslation();
  const composerDraft = useConversationComposerDraft();
  const [vm, setVm] = useState<RemoteConversationSidebarVm | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyTaskId, setBusyTaskId] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [addWorkspaceOpen, setAddWorkspaceOpen] = useState(false);
  // 折叠态：镜像本地侧栏 expandedWorkspaces 模式（默认展开 = key 不存在视为 false 折叠）。
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Record<string, boolean>>({});
  const [pinnedCollapsed, setPinnedCollapsed] = useState(false);
  const mountRef = useRef(true);

  const toggleWorkspace = useCallback((workspaceId: string) => {
    setCollapsedWorkspaces((prev) => ({ ...prev, [workspaceId]: !prev[workspaceId] }));
  }, []);

  const fetchTasks = useCallback(() => {
    setError(null);
    getMulticaTasks()
      .then((next) => { if (mountRef.current) setVm(next); })
      .catch((err) => { if (mountRef.current) setError(displayAppError(t, err)); })
      .finally(() => { if (mountRef.current) setLoading(false); });
  }, [t]);

  useEffect(() => {
    mountRef.current = true;
    fetchTasks();
    // 任务生命周期（multica-task-updated）+ 连接/工作空间配置变更（multica-settings-updated）
    // 都触发 re-fetch：绑定/解绑/连接/断开在别处发起时，本列表即时同步。
    let unsubTask = () => {};
    let unsubSettings = () => {};
    subscribeMulticaTaskUpdates(() => fetchTasks()).then((fn) => { unsubTask = fn; });
    subscribeMulticaSettingsUpdates(() => fetchTasks()).then((fn) => { unsubSettings = fn; });
    return () => { mountRef.current = false; unsubTask(); unsubSettings(); };
  }, [fetchTasks]);

  const workspacesById = useMemo(() => {
    if (!vm?.workspaces) return new Map<string, MulticaWorkspaceRefVm>();
    return new Map(vm.workspaces.map((w) => [w.id, w]));
  }, [vm?.workspaces]);

  async function handleConnect() {
    setConnecting(true);
    setError(null);
    try {
      await connectMultica();
      fetchTasks();
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setConnecting(false);
    }
  }

  async function handleClaimAndPrepare(task: RemoteTaskVm) {
    const ws = workspacesById.get(task.workspaceId);
    if (!ws) return;
    setBusyTaskId(task.id);
    setError(null);
    try {
      // claim 即领取（claim-at-click）：拿到需求正文（pending 列表只有 thread_name，正文仅 claim 响应里有）；
      // 后端同时登记 prepare lease，常驻心跳在 compose 期间续期，防 45s 被回收。
      const claimed = await claimMulticaTask(task.id, task.workspaceId);
      const requirement = claimed.requirement ?? claimed.title ?? '';
      composerDraft.prefill(requirement, {
        remoteTaskId: task.id,
        workspaceId: task.workspaceId,
        localProjectId: ws.localProjectId,
      });
      // 与本地工作空间「+」同一回调：落到 conversation-home，composer 已预填需求正文，用户选模型/模式后发送。
      onNewConversationInWorkspace(ws.localProjectId);
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusyTaskId(null);
    }
  }

  async function handleCancel(task: RemoteTaskVm) {
    setBusyTaskId(task.id);
    setError(null);
    try {
      await cancelMulticaTask(task.id);
      fetchTasks();
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusyTaskId(null);
    }
  }

  async function handleRerun(task: RemoteTaskVm) {
    const wsId = task.workspaceId || vm?.workspaces[0]?.id;
    if (!wsId) return;
    setBusyTaskId(task.id);
    setError(null);
    try {
      await rerunMulticaTask(task.issueId ?? task.id, wsId);
      fetchTasks();
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setBusyTaskId(null);
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!vm?.connected) {
    return (
      <div className="flex flex-col items-center gap-3 px-3 py-8 text-center">
        <WifiOff className="size-5 text-muted-foreground" />
        <p className="text-sm font-medium text-sidebar-foreground">{t('conversation.sidebar.multica.emptyTitle')}</p>
        <p className="text-xs text-muted-foreground">{t('conversation.sidebar.multica.emptyDescription')}</p>
        <Button size="sm" variant="outline" disabled={connecting} onClick={() => void handleConnect()}>
          {connecting ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : <Wifi className="mr-1.5 size-3.5" />}
          {t('conversation.sidebar.multica.connectButton')}
        </Button>
      </div>
    );
  }

  const allRemoteTasksByWs = vm.tasksByWorkspace ?? {};
  const boundWorkspaceIds = (vm.workspaces ?? []).map((w) => w.id);
  const hasWorkspaces = boundWorkspaceIds.length > 0;

  return (
    <div className="space-y-2">
      {hasWorkspaces ? (
        <>
          {/* 已绑定工作空间分组：始终展示（即便暂无任务/尚未 register），让用户看到已绑定的工作空间。
              可折叠（镜像本地侧栏 expandedWorkspaces + ChevronDown 模式）。 */}
          {vm.workspaces.map((ws) => {
            const tasks = allRemoteTasksByWs[ws.id] ?? [];
            const collapsed = !!collapsedWorkspaces[ws.id];
            return (
              <div key={`remote-ws-${ws.id}`} className="mb-1">
                <button
                  type="button"
                  className="flex w-full items-center gap-1.5 px-1 py-0.5 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-sidebar-foreground hover:text-sidebar-accent-foreground"
                  onClick={() => toggleWorkspace(ws.id)}
                >
                  <ChevronDown className={cn('size-3 shrink-0 transition-transform', collapsed && '-rotate-90')} />
                  <span className="truncate">{ws.name}</span>
                </button>
                {!collapsed ? (
                  <div className="space-y-0.5">
                    {tasks.length > 0 ? (
                      tasks.map((task) => (
                        <RemoteTaskRow
                          key={task.id}
                          task={task}
                          busy={busyTaskId === task.id}
                          onClaimAndPrepare={() => handleClaimAndPrepare(task)}
                          onCancel={() => handleCancel(task)}
                          onRerun={() => handleRerun(task)}
                          onSelectRun={onSelectRun}
                          t={t}
                        />
                      ))
                    ) : (
                      <p className="px-2 py-1 text-[11px] text-muted-foreground">
                        {t('conversation.sidebar.multica.noTasksInWorkspace')}
                      </p>
                    )}
                  </div>
                ) : null}
              </div>
            );
          })}

          {/* Pinned failed tasks (local failure display) — 可折叠 */}
          {vm.pinnedTasks.length > 0 && (
            <div>
              <button
                type="button"
                className="flex w-full items-center gap-1.5 px-1 py-0.5 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-sidebar-foreground hover:text-sidebar-accent-foreground"
                onClick={() => setPinnedCollapsed((v) => !v)}
              >
                <ChevronDown className={cn('size-3 shrink-0 transition-transform', pinnedCollapsed && '-rotate-90')} />
                <span>{t('conversation.sidebar.pinned')}</span>
              </button>
              {!pinnedCollapsed ? (
                <div className="space-y-0.5">
                  {vm.pinnedTasks.map((task) => (
                    <RemoteTaskRow
                      key={task.id}
                      task={task}
                      busy={busyTaskId === task.id}
                      onClaimAndPrepare={() => handleClaimAndPrepare(task)}
                      onCancel={() => handleCancel(task)}
                      onRerun={() => handleRerun(task)}
                      onSelectRun={onSelectRun}
                      t={t}
                    />
                  ))}
                </div>
              ) : null}
            </div>
          )}
        </>
      ) : (
        /* 未绑定任何工作空间 → 引导添加（已绑定时即便无任务也逐组展示，不走此空状态） */
        <div className="px-3 py-4 text-center text-xs text-muted-foreground">
          {t('conversation.sidebar.multica.noWorkspacesBound')}
        </div>
      )}

      {/* 添加工作空间入口（常驻，对齐本地任务列表形态） */}
      <button
        type="button"
        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[13px] text-muted-foreground transition-colors hover:bg-muted/40 hover:text-foreground"
        onClick={() => setAddWorkspaceOpen(true)}
      >
        <Plus className="size-3.5" />
        <span>{t('conversation.sidebar.multica.addWorkspace')}</span>
      </button>

      {error && (
        <p className="px-1 text-xs text-destructive">{error}</p>
      )}

      <MulticaAddWorkspaceDialog
        open={addWorkspaceOpen}
        onOpenChange={setAddWorkspaceOpen}
        boundWorkspaceIds={boundWorkspaceIds}
        onAdded={fetchTasks}
      />
    </div>
  );
}

function RemoteTaskRow({
  task,
  busy,
  onClaimAndPrepare,
  onCancel,
  onRerun,
  onSelectRun,
  t,
}: {
  task: RemoteTaskVm;
  busy: boolean;
  onClaimAndPrepare: () => void;
  onCancel: () => void;
  onRerun: () => void;
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const canClaim = task.status === 'queued';
  const canCancel = task.status === 'running';
  const canRerun = task.retryable && task.status === 'failed';

  const statusBadge = STATUS_VARIANT[task.status] || 'outline';

  // 终态行（completed/failed 且带本地 run 链接）：内容区整块可点 → 直达本地 conversation-run
  // （改动六：终态任务并入对应工作空间组，点击回看原会话）。
  // active 行（queued/running）与失败回显行（pinned，无本地链接）不绑点击，走右侧 claim/cancel/rerun。
  const { projectId, localTaskId, runId } = task;
  const clickable = !!(projectId && localTaskId && runId);

  const content = (
    <>
      <div className="truncate text-[14px] leading-snug text-sidebar-foreground">{task.title}</div>
      <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <Badge variant={statusBadge as any} className="h-4 px-1 text-[10px] leading-none">
          {t(`conversation.sidebar.multica.status.${task.status}`, task.status)}
        </Badge>
        {task.lastActivityAt && (
          <span className="truncate">{formatLocalDateTime(task.lastActivityAt)}</span>
        )}
      </div>
    </>
  );

  return (
    <div
      className={cn(
        'flex items-center gap-1.5 rounded-md px-2 py-1 text-left transition-colors hover:bg-muted/40',
        busy && 'pointer-events-none opacity-60',
      )}
    >
      <div className="min-w-0 flex-1">
        {clickable && projectId && localTaskId && runId ? (
          <button
            type="button"
            className="block w-full cursor-pointer text-left"
            onClick={() => onSelectRun(projectId, localTaskId, runId)}
          >
            {content}
          </button>
        ) : (
          content
        )}
      </div>
      <div className="flex shrink-0 items-center gap-0.5">
        {canClaim && (
          <Button size="icon" variant="ghost" className="size-7" disabled={busy} onClick={onClaimAndPrepare}>
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
          </Button>
        )}
        {canCancel && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="icon" variant="ghost" className="size-7 hover:text-destructive" disabled={busy} onClick={onCancel}>
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Ban className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">{t('conversation.sidebar.multica.cancelTask')}</TooltipContent>
          </Tooltip>
        )}
        {canRerun && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="icon" variant="ghost" className="size-7" disabled={busy} onClick={onRerun}>
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <RotateCw className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">{t('conversation.sidebar.multica.retryButton')}</TooltipContent>
          </Tooltip>
        )}
        {(!canClaim && !canCancel && !canRerun) && busy && (
          <Loader2 className="size-3.5 animate-spin text-muted-foreground" />
        )}
      </div>
    </div>
  );
}
