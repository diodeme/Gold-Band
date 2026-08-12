import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Folders, Globe, Loader2, Plus, RotateCw, Trash2, User, Wifi, WifiOff } from 'lucide-react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Page, PageHeader } from '@/components/PageScaffold';
import { MulticaRemoteTaskBoard } from '@/components/conversation/MulticaRemoteTaskBoard';
import { MulticaAddWorkspaceDialog } from '@/components/conversation/MulticaAddWorkspaceDialog';
import { cn } from '@/lib/utils';
import { useConversationComposerDraft } from '@/lib/conversation-composer-draft';
import {
  cancelMulticaTask,
  claimMulticaTask,
  connectMultica,
  disconnectMultica,
  getMulticaSettings,
  getMulticaTasks,
  openExternalUrl,
  removeMulticaWorkspace,
  setActiveMulticaWorkspace,
  subscribeMulticaSettingsUpdates,
  subscribeMulticaTaskUpdates,
} from '@/api';
import { displayAppError } from '@/i18n';
import type {
  MulticaSettingsVm,
  MulticaWorkspaceRefVm,
  RemoteConversationSidebarVm,
  RemoteTaskVm,
} from '@/types';

/**
 * 远程任务来源（灵活接入，当前仅 multica）。新增来源 = 向 `REMOTE_TASK_SOURCES` 加一项
 * + 在 body 按 `source` 分支渲染对应来源组件（各来源自带数据/刷新）。数据先于接口：
 * `source` 是渲染分流的唯一键，未来扩展无需改动既有 multica 路径。
 */
const REMOTE_TASK_SOURCES = [
  { value: 'multica', labelKey: 'multica.taskManagement.source.multica' },
] as const;
type RemoteTaskSource = (typeof REMOTE_TASK_SOURCES)[number]['value'];

interface MulticaTaskManagementPageProps {
  /// 直达指定远程/本地 run 的会话页（复用本地侧栏 onSelectRun 同路径）。
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  /// 远程任务「点击执行」claim 后进入会话准备页（落 conversation-home，预选最近活跃本地工作区）。
  onPrepareMulticaTask: () => void;
}

/**
 * 远程任务管理页（容器）--会话模式专用整页。
 *
 * 页头与定时任务/运行模式管理页同构：`variant="integrated"` + icon + 标题，无 actions/副标题。
 * 来源/工作空间/账号/刷新等 multica 专属控件统一下沉到底部工具条，且仅在 `source === 'multica'`
 * 且已连接时渲染--工作空间与 PAT 账号都是 multica 专属概念，未来接入其他来源未必有；source 门控
 * 让本页前向兼容（新来源只需在自己的 body 分支自绘控件，不污染 multica 路径）。
 *
 * 工作空间控件是 Popover picker：选定工作空间 + 内嵌「添加/移除工作空间」（移除走 AlertDialog 确认，
 * 对齐定时任务 delete 模式 + ui-interaction §1）。选定值持久化到 `active_workspace_id`
 * （`setActiveMulticaWorkspace`），默认 `lastActiveWorkspaceId`。
 */
export function MulticaTaskManagementPage({
  onSelectRun,
  onPrepareMulticaTask,
}: MulticaTaskManagementPageProps) {
  const { t } = useTranslation();
  const composerDraft = useConversationComposerDraft();
  const [vm, setVm] = useState<RemoteConversationSidebarVm | null>(null);
  const [settingsVm, setSettingsVm] = useState<MulticaSettingsVm | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyTaskId, setBusyTaskId] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [source, setSource] = useState<RemoteTaskSource>('multica');
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string>('');
  const [addWorkspaceOpen, setAddWorkspaceOpen] = useState(false);
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const [pendingRemoveWorkspace, setPendingRemoveWorkspace] = useState<MulticaWorkspaceRefVm | null>(null);
  const mountRef = useRef(true);

  const fetchTasks = useCallback(() => {
    setError(null);
    // 返回 promise 供手动刷新链接 refreshing 收尾（mount/event 调用忽略返回值）。
    return getMulticaTasks()
      .then((next) => { if (mountRef.current) setVm(next); })
      .catch((err) => { if (mountRef.current) setError(displayAppError(t, err)); })
      .finally(() => { if (mountRef.current) setLoading(false); });
  }, [t]);

  const fetchSettings = useCallback(() => {
    return getMulticaSettings()
      .then((next) => { if (mountRef.current) setSettingsVm(next); })
      .catch((err) => { if (mountRef.current) setError(displayAppError(t, err)); });
  }, [t]);

  const refreshAll = useCallback(() => {
    fetchTasks();
    fetchSettings();
  }, [fetchTasks, fetchSettings]);

  const handleManualRefresh = useCallback(() => {
    setRefreshing(true);
    Promise.all([fetchTasks(), fetchSettings()]).finally(() => {
      if (mountRef.current) setRefreshing(false);
    });
  }, [fetchTasks, fetchSettings]);

  useEffect(() => {
    mountRef.current = true;
    refreshAll();
    // 任务生命周期（multica-task-updated）+ 连接/工作空间配置变更（multica-settings-updated）
    // 都触发 re-fetch：绑定/解绑/连接/断开在别处发起时，本页即时同步。
    let unsubTask = () => {};
    let unsubSettings = () => {};
    subscribeMulticaTaskUpdates(() => refreshAll()).then((fn) => { unsubTask = fn; });
    subscribeMulticaSettingsUpdates(() => refreshAll()).then((fn) => { unsubSettings = fn; });
    return () => { mountRef.current = false; unsubTask(); unsubSettings(); };
  }, [refreshAll]);

  const connected = vm?.connected ?? false;
  const workspaces = vm?.workspaces ?? [];

  // 有效选定工作空间：用户选择优先；失效（被移除/未绑定）则回退 lastActive -> 首个。恒为合法 id 或 ''。
  // lastActive 也要校验是否仍存在于当前 workspaces，避免移除活跃空间后回退到已失效 id（state §4）。
  const effectiveWorkspaceId = useMemo(() => {
    if (workspaces.length === 0) return '';
    if (selectedWorkspaceId && workspaces.some((w) => w.id === selectedWorkspaceId)) {
      return selectedWorkspaceId;
    }
    const lastActive = vm?.lastActiveWorkspaceId;
    if (lastActive && workspaces.some((w) => w.id === lastActive)) {
      return lastActive;
    }
    return workspaces[0].id;
  }, [workspaces, selectedWorkspaceId, vm?.lastActiveWorkspaceId]);

  const hasWorkspaces = workspaces.length > 0;
  const selectedTasks = vm?.tasksByWorkspace[effectiveWorkspaceId] ?? [];
  const activeWorkspaceName = workspaces.find((w) => w.id === effectiveWorkspaceId)?.name ?? '';

  async function handleConnect() {
    setConnecting(true);
    setError(null);
    try {
      await connectMultica();
      refreshAll();
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setConnecting(false);
    }
  }

  async function handleClaimAndPrepare(task: RemoteTaskVm) {
    if (!task.workspaceId) return;
    setBusyTaskId(task.id);
    setError(null);
    try {
      // claim 即领取（claim-at-click）：拿到需求正文（pending 列表只有 thread_name，正文仅 claim 响应里有）；
      // 后端同时登记 prepare lease，常驻心跳在 compose 期间续期，防 45s 被回收。
      const claimed = await claimMulticaTask(task.id, task.workspaceId);
      const requirement = claimed.requirement ?? claimed.title ?? '';
      // 绑定只记 remoteTaskId + workspaceId（决策 a/c）：本地工作区延迟到执行时由 composer 下拉选，
      // 不再随绑定钉死。发送时 input.projectId（下拉值）-> startMulticaConversationRun。
      composerDraft.prefill(requirement, {
        remoteTaskId: task.id,
        workspaceId: task.workspaceId,
        title: task.title,
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

  async function handleDisconnect() {
    setError(null);
    try {
      await disconnectMultica();
      refreshAll();
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }

  // 切换账号逃生口：码灵把认证委托给浏览器，浏览器 cookie 不受控--若连到了非预期账号，
  // 此处打开 multica Web（在浏览器内登出当前账号 / 登录目标账号），再回此页重连。
  // 根因（webank 见 cookie 即签 JWT）需在 multica-webank 侧加授权确认屏，见设计文档 M5-l。
  async function handleSwitchAccount() {
    const appUrl = settingsVm?.multicaAppUrl;
    if (!appUrl) return;
    await openExternalUrl(appUrl);
  }

  async function handleWorkspaceChange(id: string) {
    setSelectedWorkspaceId(id);
    // 持久化活跃工作空间（best-effort；本地已即时切换，失败只回显错误，不回滚选择）。
    try {
      await setActiveMulticaWorkspace(id);
    } catch (err) {
      setError(displayAppError(t, err));
    }
  }

  // 行级移除：Popover 列表每行一个 Trash2 -> 走 AlertDialog 确认（对齐定时任务 delete 模式）。
  function handleRemoveWorkspaceRequest(id: string) {
    const target = workspaces.find((w) => w.id === id) ?? null;
    setPendingRemoveWorkspace(target);
  }

  async function handleConfirmRemove() {
    const target = pendingRemoveWorkspace;
    if (!target) return;
    setError(null);
    try {
      await removeMulticaWorkspace(target.id);
      // 若移除的正是当前选择，清空选择让 effectiveWorkspaceId 回退到 lastActive/首个。
      setSelectedWorkspaceId((prev) => (prev === target.id ? '' : prev));
      setPendingRemoveWorkspace(null);
      refreshAll();
    } catch (err) {
      setError(displayAppError(t, err));
      setPendingRemoveWorkspace(null);
    }
  }

  const accountLabel = settingsVm?.connectedAccount?.email
    ?? settingsVm?.connectedAccount?.name
    ?? t('multica.taskManagement.account.connected');

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        variant="integrated"
        icon={<Globe />}
        title={<span className="text-title">{t('multica.taskManagement.title')}</span>}
        actions={
          /* 来源（根选择器）放页头：与定时任务管理页 actions 槽同构；为未来多来源保留切换位，当前仅 multica */
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">{t('multica.taskManagement.source.label')}</span>
            <Select value={source} onValueChange={(v) => setSource(v as RemoteTaskSource)}>
              <SelectTrigger size="sm" className="w-[140px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent align="start">
                {REMOTE_TASK_SOURCES.map((s) => (
                  <SelectItem key={s.value} value={s.value}>{t(s.labelKey)}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-5 xl:p-6">
        {source !== 'multica' ? null : loading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        ) : !connected ? (
          <div className="flex flex-col items-center gap-3 px-3 py-8 text-center">
            <WifiOff className="size-5 text-muted-foreground" />
            <p className="text-sm font-medium text-sidebar-foreground">{t('conversation.sidebar.multica.emptyTitle')}</p>
            <p className="text-xs text-muted-foreground">{t('conversation.sidebar.multica.emptyDescription')}</p>
            <Button size="sm" variant="outline" disabled={connecting} onClick={() => void handleConnect()}>
              {connecting ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : <Wifi className="mr-1.5 size-3.5" />}
              {t('conversation.sidebar.multica.connectButton')}
            </Button>
          </div>
        ) : !hasWorkspaces ? (
          /* 未绑定任何工作空间 -> 引导添加（工作空间 picker 内亦可进添加弹窗） */
          <div className="flex flex-col items-center gap-3 px-3 py-8 text-center">
            <p className="text-xs text-muted-foreground">{t('conversation.sidebar.multica.noWorkspacesBound')}</p>
            <Button size="sm" variant="outline" onClick={() => setAddWorkspaceOpen(true)}>
              {t('conversation.sidebar.multica.addWorkspace')}
            </Button>
          </div>
        ) : (
          <MulticaRemoteTaskBoard
            tasks={selectedTasks}
            busyTaskId={busyTaskId}
            onClaim={(task) => void handleClaimAndPrepare(task)}
            onCancel={(task) => void handleCancel(task)}
            onSelectRun={onSelectRun}
          />
        )}

        {error && <p className="px-1 pt-2 text-xs text-destructive">{error}</p>}
      </div>

      {/*
        底部工具条：工作空间管理（Popover picker，内嵌添加/移除）+ 账号 + 刷新。
        来源（根选择器）已上移到页头 actions；此处仅 multica 专属控件，受 source + 连接态门控。
      */}
      {source === 'multica' && connected && (
        <footer className="shrink-0 border-t border-border/60 bg-background/60 px-5 py-3 backdrop-blur xl:px-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex flex-wrap items-center gap-2">
              {/* 工作空间 Popover picker：选定后只看该空间任务；内嵌添加/移除工作空间（选定值 = active workspace，持久化） */}
              {hasWorkspaces && (
                <Popover open={workspacePickerOpen} onOpenChange={setWorkspacePickerOpen}>
                  <PopoverTrigger asChild>
                    <Button variant="outline" size="sm" className="max-w-[220px] gap-1.5">
                      <Folders className="size-3.5 shrink-0" />
                      <span className="truncate">{activeWorkspaceName}</span>
                      <ChevronDown className="size-3 shrink-0 opacity-50" />
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent align="start" className="w-[260px] p-0">
                    <div className="p-1">
                      <Button
                        variant="ghost"
                        size="sm"
                        className="w-full justify-start gap-1.5"
                        onClick={() => {
                          setWorkspacePickerOpen(false);
                          setAddWorkspaceOpen(true);
                        }}
                      >
                        <Plus className="size-3.5" />
                        {t('conversation.sidebar.multica.addWorkspace')}
                      </Button>
                    </div>
                    <Separator />
                    <div className="max-h-64 overflow-auto p-1">
                      {workspaces.map((w) => (
                        <div
                          key={w.id}
                          className={cn(
                            'flex items-center gap-1 rounded-sm p-1',
                            w.id === effectiveWorkspaceId && 'bg-accent text-accent-foreground',
                          )}
                        >
                          <button
                            type="button"
                            data-testid={`ws-pick-${w.id}`}
                            className="flex min-w-0 flex-1 items-center px-1 py-0.5 text-left text-sm"
                            onClick={() => {
                              void handleWorkspaceChange(w.id);
                              setWorkspacePickerOpen(false);
                            }}
                          >
                            <span className="truncate">{w.name}</span>
                          </button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="size-6 shrink-0 hover:text-destructive"
                            data-testid={`ws-remove-${w.id}`}
                            aria-label={t('multica.taskManagement.workspace.remove')}
                            onClick={() => handleRemoveWorkspaceRequest(w.id)}
                          >
                            <Trash2 className="size-3.5" />
                          </Button>
                        </div>
                      ))}
                    </div>
                  </PopoverContent>
                </Popover>
              )}
            </div>

            <div className="flex items-center gap-2">
              {/* 账号菜单：multica 连接/PAT 专属（切换账号 / 断开连接）；设置页不再暴露 multica */}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline" size="sm" className="max-w-[200px] gap-1.5">
                    <User className="size-3.5 shrink-0" />
                    <span className="truncate">{accountLabel}</span>
                    <ChevronDown className="size-3 shrink-0 opacity-50" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-56">
                  <DropdownMenuItem
                    disabled={!settingsVm?.multicaAppUrl}
                    onClick={() => void handleSwitchAccount()}
                  >
                    {t('multica.taskManagement.account.switchAccount')}
                  </DropdownMenuItem>
                  <DropdownMenuItem className="text-destructive" onClick={() => void handleDisconnect()}>
                    {t('multica.taskManagement.account.disconnect')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>

              {/* 手动刷新 */}
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
          </div>
        </footer>
      )}

      <MulticaAddWorkspaceDialog
        open={addWorkspaceOpen}
        onOpenChange={setAddWorkspaceOpen}
        boundWorkspaceIds={workspaces.map((w) => w.id)}
        onAdded={refreshAll}
      />

      {/* 移除工作空间确认（对齐定时任务 delete 模式 + ui-interaction §1 删除确认用 Dialog） */}
      <AlertDialog open={!!pendingRemoveWorkspace} onOpenChange={(open) => { if (!open) setPendingRemoveWorkspace(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('multica.taskManagement.workspace.remove')}</AlertDialogTitle>
            <AlertDialogDescription>{t('multica.taskManagement.workspace.removeConfirm')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => void handleConfirmRemove()}
            >
              {t('common.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}
