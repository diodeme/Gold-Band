import { Ban, ChevronDown, Loader2, Play, Plus, RotateCw, Server, Wifi, WifiOff } from 'lucide-react';
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
  /// 远程任务「点击执行」claim 后进入会话准备页：预填需求正文 + multica 绑定，落到 conversation-home，
  /// 本地工作区延迟到执行时下拉选（App 预选最近活跃本地工作区，决策 c）。
  onPrepareMulticaTask: () => void;
}

// 远程任务状态 → 徽章色调（看板词汇对齐：待办=灰、进行中=黄、已完成=绿、失败=红）。
// 结构化管理（杜绝硬编码）：每个 canonical status 一个色调类，经 Badge className（twMerge 合并）应用。
// 导出供单测固化「4 个 canonical status 各有色调」这一接口层验收。
export const MULTICA_STATUS_TONE: Record<string, string> = {
  queued: 'border-transparent bg-muted text-muted-foreground',
  running: 'border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-300',
  completed: 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-300',
  failed: 'border-transparent bg-destructive/15 text-destructive',
};

export function MulticaRemoteTaskList({ onSelectRun, onPrepareMulticaTask }: MulticaRemoteTaskListProps) {
  const { t } = useTranslation();
  const composerDraft = useConversationComposerDraft();
  const [vm, setVm] = useState<RemoteConversationSidebarVm | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyTaskId, setBusyTaskId] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [addWorkspaceOpen, setAddWorkspaceOpen] = useState(false);
  // 手动刷新态：刷新按钮 spin 指示（不复用 loading——loading 会用整屏 spinner 替换列表）。
  const [refreshing, setRefreshing] = useState(false);
  // 折叠态：镜像本地侧栏 expandedWorkspaces 模式（默认展开 = key 不存在视为 false 折叠）。
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Record<string, boolean>>({});
  const [pinnedCollapsed, setPinnedCollapsed] = useState(false);
  const mountRef = useRef(true);

  const toggleWorkspace = useCallback((workspaceId: string) => {
    setCollapsedWorkspaces((prev) => ({ ...prev, [workspaceId]: !prev[workspaceId] }));
  }, []);

  const fetchTasks = useCallback(() => {
    setError(null);
    // 返回 promise 供手动刷新链接 refreshing 收尾（mount/event 调用忽略返回值）。
    return getMulticaTasks()
      .then((next) => { if (mountRef.current) setVm(next); })
      .catch((err) => { if (mountRef.current) setError(displayAppError(t, err)); })
      .finally(() => { if (mountRef.current) setLoading(false); });
  }, [t]);

  const handleManualRefresh = useCallback(() => {
    setRefreshing(true);
    fetchTasks().finally(() => { if (mountRef.current) setRefreshing(false); });
  }, [fetchTasks]);

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
    if (!workspacesById.has(task.workspaceId)) return;
    setBusyTaskId(task.id);
    setError(null);
    try {
      // claim 即领取（claim-at-click）：拿到需求正文（pending 列表只有 thread_name，正文仅 claim 响应里有）；
      // 后端同时登记 prepare lease，常驻心跳在 compose 期间续期，防 45s 被回收。
      const claimed = await claimMulticaTask(task.id, task.workspaceId);
      const requirement = claimed.requirement ?? claimed.title ?? '';
      // 绑定只记 remoteTaskId + workspaceId（决策 a/c）：本地工作区延迟到执行时由 composer 下拉选，
      // 不再随绑定钉死。发送时 input.projectId（下拉值）→ startMulticaConversationRun。
      composerDraft.prefill(requirement, {
        remoteTaskId: task.id,
        workspaceId: task.workspaceId,
      });
      // 落 conversation-home：composer 已预填正文 + multica 绑定；本地工作区由 App 预选最近活跃，用户可改（决策 c/d）。
      onPrepareMulticaTask();
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
      {/* 手动刷新：免切换页面重进即可拉最新任务列表。常驻连接态右上角。 */}
      <div className="flex justify-end">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="icon"
              variant="ghost"
              className="size-7"
              disabled={refreshing}
              onClick={handleManualRefresh}
              aria-label={t('common.refresh')}
            >
              <RotateCw className={cn('size-3.5', refreshing && 'animate-spin')} />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top" className="text-xs">{t('common.refresh')}</TooltipContent>
        </Tooltip>
      </div>
      {hasWorkspaces ? (
        <>
          {/* 已绑定工作空间分组：始终展示（即便暂无任务/尚未 register），让用户看到已绑定的工作空间。
              可折叠（镜像本地侧栏 expandedWorkspaces + ChevronDown 模式）。 */}
          {vm.workspaces.map((ws) => {
            const tasks = allRemoteTasksByWs[ws.id] ?? [];
            const collapsed = !!collapsedWorkspaces[ws.id];
            return (
              <div key={`remote-ws-${ws.id}`} className="mb-2">
                <button
                  type="button"
                  className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-sidebar-foreground transition-colors hover:bg-muted/40 hover:text-sidebar-accent-foreground"
                  onClick={() => toggleWorkspace(ws.id)}
                >
                  <ChevronDown className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform', collapsed && '-rotate-90')} />
                  <Server className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate">{ws.name}</span>
                  {tasks.length > 0 && (
                    <span className="ml-1 truncate text-[11px] font-normal normal-case tracking-normal text-muted-foreground">
                      {t('conversation.sidebar.multica.taskCount', { count: tasks.length })}
                    </span>
                  )}
                </button>
                {!collapsed ? (
                  <div className="mt-0.5 space-y-0.5 pl-2">
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
                      <p className="px-2 py-4 text-center text-[11px] text-muted-foreground">
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
                className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left text-[12px] font-semibold uppercase tracking-[0.12em] text-sidebar-foreground transition-colors hover:bg-muted/40 hover:text-sidebar-accent-foreground"
                onClick={() => setPinnedCollapsed((v) => !v)}
              >
                <ChevronDown className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform', pinnedCollapsed && '-rotate-90')} />
                <span>{t('conversation.sidebar.pinned')}</span>
              </button>
              {!pinnedCollapsed ? (
                <div className="mt-0.5 space-y-0.5 pl-2">
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

  const statusTone = MULTICA_STATUS_TONE[task.status] ?? MULTICA_STATUS_TONE.queued;

  // 终态行（completed/failed 且带本地 run 链接）：内容区整块可点 → 直达本地 conversation-run
  // （改动六：终态任务并入对应工作空间组，点击回看原会话）。
  // active 行（queued/running）与失败回显行（pinned，无本地链接）不绑点击，走右侧 claim/cancel/rerun。
  const { projectId, localTaskId, runId } = task;
  const clickable = !!(projectId && localTaskId && runId);

  const content = (
    <>
      <div className="truncate text-[14px] font-medium leading-snug text-foreground">{task.title}</div>
      <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <Badge variant="outline" className={cn('h-4 shrink-0 px-1 text-[10px] leading-none', statusTone)}>
          {t(`conversation.sidebar.multica.status.${task.status}`, task.status)}
        </Badge>
        {task.lastActivityAt && (
          <span className="ml-auto shrink-0 truncate text-[10px] tabular-nums">
            {formatLocalDateTime(task.lastActivityAt)}
          </span>
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
          <Button size="icon" variant="ghost" className="size-7" disabled={busy} onClick={onClaimAndPrepare} aria-label={t('conversation.sidebar.multica.executeTask')}>
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
          </Button>
        )}
        {canCancel && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="icon" variant="ghost" className="size-7 hover:text-destructive" disabled={busy} onClick={onCancel} aria-label={t('conversation.sidebar.multica.cancelTask')}>
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Ban className="size-3.5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">{t('conversation.sidebar.multica.cancelTask')}</TooltipContent>
          </Tooltip>
        )}
        {canRerun && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="icon" variant="ghost" className="size-7" disabled={busy} onClick={onRerun} aria-label={t('conversation.sidebar.multica.retryButton')}>
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
