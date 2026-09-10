import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { Ban, Loader2, Play } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { formatLocalDateTime } from '@/lib/datetime';
import type { RemoteTaskVm } from '../../types';

/**
 * 远程任务看板（presentational）。
 *
 * 数据/订阅/动作（connect/prepare/cancel/refresh/workspace 选择）由容器页
 * `MulticaTaskManagementPage` 持有；本组件只接收「选定工作空间的扁平任务列表」，
 * 按 canonical status 分桶到 4 列（待办 / 进行中 / 已完成 / 失败），逐任务渲染卡片。
 *
 * 列与动作 1:1（pinned 不展示后，展示中的任务都不可重试 → 无 rerun）：
 * - queued   → 准备执行(prepare)：只读取需求正文 + 绑定 composer，任务仍 queued，发送时才 claim+start
 * - running  → 取消(cancel)
 * - completed/failed（带本地 run 链接）→ 点击回看会话(onSelectRun)
 */

/// 看板列定义：4 canonical status，与 `RemoteTaskVm.status` 1:1
/// （后端 `normalize_remote_status` 已把服务端各种拼写收敛到这 4 个）。
export const BOARD_COLUMNS = ['queued', 'running', 'completed', 'failed'] as const;
export type BoardColumnStatus = (typeof BOARD_COLUMNS)[number];

/// 远程任务状态 → 徽章 / 列头计数色调（看板词汇：待办=灰、进行中=黄、已完成=绿、失败=红）。
/// 结构化管理（杜绝硬编码）：每个 canonical status 一个色调类，经 Badge className（twMerge 合并）应用。
/// 导出供单测固化「4 个 canonical status 各有色调」这一接口层验收。
export const MULTICA_STATUS_TONE: Record<BoardColumnStatus, string> = {
  queued: 'border-transparent bg-muted text-muted-foreground',
  running: 'border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-300',
  completed: 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-300',
  failed: 'border-transparent bg-destructive/15 text-destructive',
};

/// issue 类型徽标：只有 dev/test/bug 渲染（story dev/test 拆分）。
///
/// `general` 与无 issue 任务不渲染——对齐 multica 自有 views 的 `hideGeneral` 惯例，降低噪音；
/// 闭集外的未知值同样不渲染（版本前向兼容：新类型不猜文案）。
export const MULTICA_ISSUE_KINDS = ['dev', 'test', 'bug'] as const;
export type MulticaIssueKind = (typeof MULTICA_ISSUE_KINDS)[number];

/// issue 类型 → 徽标色调（与 `MULTICA_STATUS_TONE` 同款 token 风格：语义色调 + 明暗双主题变体）。
/// dev = 开发（sky）、test = 测试（violet）、bug = 缺陷（destructive，与 failed 同色系）。
export const MULTICA_ISSUE_KIND_TONE: Record<MulticaIssueKind, string> = {
  dev: 'border-transparent bg-sky-500/15 text-sky-600 dark:text-sky-300',
  test: 'border-transparent bg-violet-500/15 text-violet-600 dark:text-violet-300',
  bug: 'border-transparent bg-destructive/15 text-destructive',
};

/// 纯函数：wire 的 `issue_kind` → 可渲染的类型徽标（不可渲染返回 null）。
/// 导出供单测固化「哪些类型渲染徽标」这一接口层验收。
export function visibleIssueKind(issueKind: string | null): MulticaIssueKind | null {
  return MULTICA_ISSUE_KINDS.find((kind) => kind === issueKind) ?? null;
}

/// 执行准入谓词（与后端 `RemoteTask::executable_ready` 同构）：
/// 非 test 恒可执行；test 需 `isReady === true`（缺失按未就绪，保守——覆盖服务端灰度窗口）。
/// 旧 server（两字段皆 null）→ `null !== 'test'` → true，行为与拆分前一致。
/// 导出供单测固化看板置灰 + 执行按钮准入的判定契约（与后端 claim 拦截同源）。
export function isTaskExecutable(task: Pick<RemoteTaskVm, 'issueKind' | 'isReady'>): boolean {
  return task.issueKind !== 'test' || task.isReady === true;
}

/// 纯函数：把「选定工作空间的扁平任务列表」按 canonical status 分桶到 4 列。
/// 未知 status（理论不会出现——normalize 已收敛）不入任何列，作为接口层兜底。
/// 导出供单测固化分桶不变量（4 列正确 + 未知状态丢弃 + 空输入）。
export function bucketTasksByStatus(tasks: RemoteTaskVm[]): Record<BoardColumnStatus, RemoteTaskVm[]> {
  const buckets: Record<BoardColumnStatus, RemoteTaskVm[]> = {
    queued: [],
    running: [],
    completed: [],
    failed: [],
  };
  for (const task of tasks) {
    if (
      task.status === 'queued' ||
      task.status === 'running' ||
      task.status === 'completed' ||
      task.status === 'failed'
    ) {
      buckets[task.status].push(task);
    }
  }
  return buckets;
}

interface MulticaRemoteTaskBoardProps {
  /// 选定工作空间的全部远程任务（active queued/running + 终态 completed/failed）。
  tasks: RemoteTaskVm[];
  /// 当前正在执行异步动作的任务 id（prepare/cancel），对应卡片禁用 + spin。
  busyTaskId: string | null;
  /// 准备执行 queued 任务（claim-at-send 的只读取）：容器负责拉需求正文 + 预填 composer + 跳会话准备页，
  /// **不领取任务**（仍 queued），发送时才 claim+start。
  onPrepare: (task: RemoteTaskVm) => void;
  /// 取消 running 任务。
  onCancel: (task: RemoteTaskVm) => void;
  /// 终态行（带本地 run 链接）整块点击 → 直达本地 conversation-run。
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
}

export function MulticaRemoteTaskBoard({
  tasks,
  busyTaskId,
  onPrepare,
  onCancel,
  onSelectRun,
}: MulticaRemoteTaskBoardProps) {
  const { t } = useTranslation();
  const buckets = bucketTasksByStatus(tasks);

  return (
    <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
      {BOARD_COLUMNS.map((status) => {
        const colTasks = buckets[status];
        return (
          <section key={status} className="flex min-h-0 min-w-0 flex-col gap-2">
            <header className="flex items-center justify-between gap-2 px-1">
              <h2 className="truncate text-sm font-semibold tracking-tight text-foreground">
                {t(`conversation.sidebar.multica.status.${status}`)}
              </h2>
              <Badge
                variant="outline"
                className={cn('h-5 shrink-0 px-1.5 text-[11px] leading-none', MULTICA_STATUS_TONE[status])}
              >
                {colTasks.length}
              </Badge>
            </header>
            <div className="space-y-2">
              {colTasks.length > 0 ? (
                colTasks.map((task) => (
                  <MulticaRemoteTaskCard
                    key={task.id}
                    task={task}
                    busy={busyTaskId === task.id}
                    onPrepare={onPrepare}
                    onCancel={onCancel}
                    onSelectRun={onSelectRun}
                    t={t}
                  />
                ))
              ) : (
                <p className="rounded-md border border-dashed border-border/60 px-3 py-6 text-center text-[11px] text-muted-foreground">
                  {t('multica.taskManagement.column.empty')}
                </p>
              )}
            </div>
          </section>
        );
      })}
    </div>
  );
}

type TranslationFn = ReturnType<typeof useTranslation>['t'];

function MulticaRemoteTaskCard({
  task,
  busy,
  onPrepare,
  onCancel,
  onSelectRun,
  t,
}: {
  task: RemoteTaskVm;
  busy: boolean;
  onPrepare: (task: RemoteTaskVm) => void;
  onCancel: (task: RemoteTaskVm) => void;
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  t: TranslationFn;
}) {
  const readOnly = useReadOnlyExperience();
  const queued = task.status === 'queued';
  const executable = isTaskExecutable(task);
  /// 未就绪的 test 任务（等父 dev issue 完成）：置灰 + 执行入口禁用 + 原因 Tooltip。
  const notReady = queued && !executable;
  const canClaim = queued && executable;
  const canCancel = task.status === 'running';
  const statusTone = MULTICA_STATUS_TONE[task.status as BoardColumnStatus] ?? MULTICA_STATUS_TONE.queued;
  const issueKind = visibleIssueKind(task.issueKind);
  const notReadyHint = t('multica.taskManagement.readiness.notReadyHint');

  // 终态行（completed/failed 且带本地 run 链接）：内容区整块可点 → 直达本地 conversation-run。
  // active 行（queued/running）无本地链接，不绑点击，走右侧 prepare/cancel。
  const { projectId, localTaskId, runId } = task;
  const clickable = !!(projectId && localTaskId && runId);

  const content = (
    <div className="space-y-1.5">
      <div className="truncate text-[13px] font-medium leading-snug text-foreground">{task.title}</div>
      <div className="flex items-center gap-1.5">
        {issueKind && (
          <Badge
            variant="outline"
            className={cn('h-4 shrink-0 px-1 text-[10px] leading-none', MULTICA_ISSUE_KIND_TONE[issueKind])}
          >
            {t(`multica.taskManagement.issueKind.${issueKind}`)}
          </Badge>
        )}
        <Badge variant="outline" className={cn('h-4 shrink-0 px-1 text-[10px] leading-none', statusTone)}>
          {t(`conversation.sidebar.multica.status.${task.status}`, task.status)}
        </Badge>
        {notReady && (
          /* 未就绪标记：原因（等父开发任务完成）经项目 Tooltip 说明；tabIndex 让键盘也能触发提示。 */
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                tabIndex={0}
                aria-label={notReadyHint}
                className={cn(
                  'inline-flex h-4 shrink-0 items-center rounded-full px-1 text-[10px] leading-none',
                  MULTICA_STATUS_TONE.queued,
                )}
              >
                {t('multica.taskManagement.readiness.notReady')}
              </span>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">{notReadyHint}</TooltipContent>
          </Tooltip>
        )}
        {task.lastActivityAt && (
          <span className="ml-auto shrink-0 truncate text-[10px] tabular-nums text-muted-foreground">
            {formatLocalDateTime(task.lastActivityAt)}
          </span>
        )}
      </div>
    </div>
  );

  return (
    <Card
      className={cn(
        'gap-2 py-2.5 shadow-none',
        busy && 'pointer-events-none opacity-60',
        // 未就绪 test：整体弱化，与卡片上「未就绪」标记 + 禁用执行入口构成同一状态表达。
        notReady && 'opacity-60',
      )}
    >
      <CardContent className="px-3">
        <div className="flex items-center gap-1.5">
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
            {queued && (
              /* 未就绪 test 保留执行入口但禁用（可见的「暂时不能执行」而非入口消失——入口消失会被读成
                 任务不可用；禁用 + 卡片「未就绪」标记表达「等开发任务完成」）。原因说明在卡片标记的 Tooltip。 */
              <Button
                size="icon"
                variant="ghost"
                className="size-7"
                disabled={busy || !canClaim}
                onClick={() => onPrepare(task)}
                aria-label={t('conversation.sidebar.multica.executeTask')}
              >
                {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
              </Button>
            )}
            {canCancel && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-7 hover:text-destructive"
                    disabled={readOnly || busy}
                    onClick={() => onCancel(task)}
                    aria-label={t('conversation.sidebar.multica.cancelTask')}
                  >
                    {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Ban className="size-3.5" />}
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top" className="text-xs">{t('conversation.sidebar.multica.cancelTask')}</TooltipContent>
              </Tooltip>
            )}
            {(!canClaim && !canCancel) && busy && (
              <Loader2 className="size-3.5 animate-spin text-muted-foreground" />
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
