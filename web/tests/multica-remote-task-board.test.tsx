/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

// `t` 跨渲染稳定；返回 key 本身便于断言（列头/徽章/aria-label 都走 i18n key）。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('lucide-react', () => ({
  Ban: () => null,
  Loader2: () => null,
  Play: () => null,
}));

vi.mock('@/lib/utils', () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(' '),
}));

vi.mock('@/components/ui/card', () => ({
  Card: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  CardContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock('@/components/ui/badge', () => ({
  Badge: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));
vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));

import { formatLocalDateTime } from '@/lib/datetime';
import {
  MulticaRemoteTaskBoard,
  bucketTasksByStatus,
  visibleIssueKind,
  isTaskNotReady,
  BOARD_COLUMNS,
  MULTICA_STATUS_TONE,
  MULTICA_ISSUE_KIND_TONE,
} from '@/components/conversation/MulticaRemoteTaskBoard';
import type { RemoteTaskVm } from '@/types';

function task(overrides: Partial<RemoteTaskVm> = {}): RemoteTaskVm {
  return {
    id: 'rt-1',
    issueId: null,
    status: 'queued',
    workspaceId: 'ws-1',
    title: 'Task',
    requirement: null,
    lastActivityAt: null,
    localTaskId: null,
    runId: null,
    projectId: null,
    issueKind: null,
    isReady: null,
    ...overrides,
  };
}

afterEach(() => {
  document.body.innerHTML = '';
});

// 纯函数分桶：看板的接口层不变量（4 列正确 + 未知状态丢弃 + 保序 + 空输入）。
describe('bucketTasksByStatus', () => {
  it('exposes exactly the 4 canonical-status columns and returns empty buckets for empty input', () => {
    expect(BOARD_COLUMNS).toEqual(['queued', 'running', 'completed', 'failed']);
    const buckets = bucketTasksByStatus([]);
    for (const status of BOARD_COLUMNS) {
      expect(buckets[status]).toEqual([]);
    }
  });

  it('distributes tasks into the matching canonical-status bucket', () => {
    const queued = task({ id: 'q', status: 'queued' });
    const running = task({ id: 'r', status: 'running' });
    const completed = task({ id: 'c', status: 'completed' });
    const failed = task({ id: 'f', status: 'failed' });
    const buckets = bucketTasksByStatus([queued, running, completed, failed]);
    expect(buckets.queued).toEqual([queued]);
    expect(buckets.running).toEqual([running]);
    expect(buckets.completed).toEqual([completed]);
    expect(buckets.failed).toEqual([failed]);
  });

  it('drops tasks whose status is not one of the 4 canonical values (normalize 兜底)', () => {
    const unknown = task({ id: 'u', status: 'wat' as RemoteTaskVm['status'] });
    const queued = task({ id: 'q', status: 'queued' });
    const buckets = bucketTasksByStatus([unknown, queued]);
    expect(buckets.queued).toEqual([queued]);
    expect(buckets.running).toEqual([]);
    expect(buckets.completed).toEqual([]);
    expect(buckets.failed).toEqual([]);
  });

  it('preserves insertion order within each bucket', () => {
    const a = task({ id: 'a', status: 'queued' });
    const b = task({ id: 'b', status: 'queued' });
    const c = task({ id: 'c', status: 'failed' });
    const buckets = bucketTasksByStatus([c, a, b]);
    expect(buckets.queued.map((t) => t.id)).toEqual(['a', 'b']);
    expect(buckets.failed.map((t) => t.id)).toEqual(['c']);
  });
});

// 4 canonical status → 看板词汇配色（待办=灰、进行中=黄、已完成=绿、失败=红）。
describe('multica status tone config', () => {
  it('maps every canonical status to its board-vocabulary color', () => {
    expect(MULTICA_STATUS_TONE.queued).toMatch(/muted/);
    expect(MULTICA_STATUS_TONE.running).toMatch(/amber/);
    expect(MULTICA_STATUS_TONE.completed).toMatch(/emerald/);
    expect(MULTICA_STATUS_TONE.failed).toMatch(/destructive/);
    expect(Object.keys(MULTICA_STATUS_TONE).sort()).toEqual(['completed', 'failed', 'queued', 'running']);
  });
});

// issue 类型徽标 + 未就绪提醒谓词（story dev/test 拆分；仅提醒不阻断，§12.42）。
describe('multica issue kind badge and not-ready predicate', () => {
  it('renders badges only for dev/test/bug and hides general/missing kinds', () => {
    expect(visibleIssueKind('dev')).toBe('dev');
    expect(visibleIssueKind('test')).toBe('test');
    expect(visibleIssueKind('bug')).toBe('bug');
    // general 与缺字段不渲染徽标（对齐 multica 自有 views 的 hideGeneral 惯例）。
    expect(visibleIssueKind('general')).toBeNull();
    expect(visibleIssueKind(null)).toBeNull();
    // 未知类型同样不渲染（前向兼容：新类型不猜文案）。
    expect(visibleIssueKind('epic')).toBeNull();
  });

  it('maps every badge kind to its own tone', () => {
    expect(MULTICA_ISSUE_KIND_TONE.dev).toMatch(/sky/);
    expect(MULTICA_ISSUE_KIND_TONE.test).toMatch(/violet/);
    expect(MULTICA_ISSUE_KIND_TONE.bug).toMatch(/destructive/);
  });

  it('flags only test kind as not-ready (提醒谓词：缺失按未就绪展示，不阻断执行)', () => {
    // 非 test（含旧 server 的 null）恒无未就绪提醒，不受 isReady 影响。
    expect(isTaskNotReady({ issueKind: 'dev', isReady: false })).toBe(false);
    expect(isTaskNotReady({ issueKind: 'bug', isReady: null })).toBe(false);
    expect(isTaskNotReady({ issueKind: null, isReady: null })).toBe(false);
    // test：isReady !== true（false / null）都展示未就绪提醒（仅提醒，§12.42）。
    expect(isTaskNotReady({ issueKind: 'test', isReady: true })).toBe(false);
    expect(isTaskNotReady({ issueKind: 'test', isReady: false })).toBe(true);
    expect(isTaskNotReady({ issueKind: 'test', isReady: null })).toBe(true);
  });
});

async function renderBoard(props: {
  tasks: RemoteTaskVm[];
  busyTaskId?: string | null;
  onPrepare?: (t: RemoteTaskVm) => void;
  onCancel?: (t: RemoteTaskVm) => void;
  onSelectRun?: (p: string, t: string, r: string) => void;
}) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  const onPrepare = props.onPrepare ?? vi.fn();
  const onCancel = props.onCancel ?? vi.fn();
  const onSelectRun = props.onSelectRun ?? vi.fn();
  await act(async () => {
    root.render(
      <MulticaRemoteTaskBoard
        tasks={props.tasks}
        busyTaskId={props.busyTaskId ?? null}
        onPrepare={onPrepare}
        onCancel={onCancel}
        onSelectRun={onSelectRun}
      />,
    );
  });
  return { container, onPrepare, onCancel, onSelectRun };
}

describe('MulticaRemoteTaskBoard render', () => {
  it('renders the 4 column headers and a task in each column', async () => {
    const { container } = await renderBoard({
      tasks: [
        task({ id: 'q', status: 'queued', title: 'Todo' }),
        task({ id: 'r', status: 'running', title: 'Doing' }),
        task({ id: 'c', status: 'completed', title: 'Done' }),
        task({ id: 'f', status: 'failed', title: 'Boom' }),
      ],
    });
    // 4 列头（status 标签走 i18n key，mock t 返回 key）。
    for (const status of BOARD_COLUMNS) {
      expect(container.textContent).toContain(`conversation.sidebar.multica.status.${status}`);
    }
    expect(container.textContent).toContain('Todo');
    expect(container.textContent).toContain('Doing');
    expect(container.textContent).toContain('Done');
    expect(container.textContent).toContain('Boom');
  });

  it('shows the empty hint for every column when there are no tasks', async () => {
    const { container } = await renderBoard({ tasks: [] });
    expect(container.textContent).toContain('multica.taskManagement.column.empty');
  });

  it('renders a prepare button only for queued tasks and forwards onPrepare', async () => {
    const onPrepare = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 'q', status: 'queued', title: 'Todo' })],
      onPrepare,
    });
    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn).toBeTruthy();
    await act(async () => { claimBtn.click(); });
    expect(onPrepare).toHaveBeenCalledTimes(1);
    expect((onPrepare.mock.calls[0] as [RemoteTaskVm])[0].id).toBe('q');
  });

  it('renders a cancel button only for running tasks and forwards onCancel', async () => {
    const onCancel = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 'r', status: 'running', title: 'Doing' })],
      onCancel,
    });
    const cancelBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.cancelTask"]') as HTMLButtonElement;
    expect(cancelBtn).toBeTruthy();
    await act(async () => { cancelBtn.click(); });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect((onCancel.mock.calls[0] as [RemoteTaskVm])[0].id).toBe('r');
  });

  it('makes a terminal task with a local run link clickable → onSelectRun', async () => {
    const onSelectRun = vi.fn();
    const { container } = await renderBoard({
      tasks: [
        task({
          id: 'c', status: 'completed', title: 'Done',
          projectId: 'proj-1', localTaskId: 't-1', runId: 'r-1',
        }),
      ],
      onSelectRun,
    });
    // 终态行整块内容包成 button：按文本定位。
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => (b.textContent ?? '').includes('Done'),
    ) as HTMLButtonElement;
    expect(btn).toBeTruthy();
    await act(async () => { btn.click(); });
    expect(onSelectRun).toHaveBeenCalledWith('proj-1', 't-1', 'r-1');
  });

  it('does not wrap a terminal task without a local run link in a click handler', async () => {
    const onSelectRun = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 'c', status: 'failed', title: 'NoLink' })],
      onSelectRun,
    });
    // 无 projectId/localTaskId/runId → 内容直接渲染为文本，非 button。
    const btn = Array.from(container.querySelectorAll('button')).find(
      (b) => (b.textContent ?? '').includes('NoLink'),
    );
    expect(btn).toBeUndefined();
    expect(container.textContent).toContain('NoLink');
  });

  it('renders the issue-kind badge for dev/test/bug but not for general', async () => {
    const { container } = await renderBoard({
      tasks: [
        task({ id: 'd', status: 'queued', title: 'DevTask', issueKind: 'dev' }),
        task({ id: 't', status: 'queued', title: 'TestTask', issueKind: 'test', isReady: true }),
        task({ id: 'b', status: 'queued', title: 'BugTask', issueKind: 'bug' }),
        task({ id: 'g', status: 'queued', title: 'GeneralTask', issueKind: 'general' }),
      ],
    });
    expect(container.textContent).toContain('multica.taskManagement.issueKind.dev');
    expect(container.textContent).toContain('multica.taskManagement.issueKind.test');
    expect(container.textContent).toContain('multica.taskManagement.issueKind.bug');
    // general 不渲染类型徽标（仅有状态徽标）。
    expect(container.textContent).not.toContain('multica.taskManagement.issueKind.general');
  });

  it('marks a queued test task whose dev parent is not done as not-ready but still allows execution', async () => {
    const onPrepare = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 't', status: 'queued', title: 'TestTask', issueKind: 'test', isReady: false })],
      onPrepare,
    });
    // 未就绪标记 + 原因提示（Tooltip 文案）都在卡片上（仅提醒，§12.42 产品决策）。
    expect(container.textContent).toContain('multica.taskManagement.readiness.notReady');
    expect(container.textContent).toContain('multica.taskManagement.readiness.notReadyHint');
    // 执行入口照常可用：未就绪不阻断（后端 claim 拦截已随门控一并移除）。
    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn).toBeTruthy();
    expect(claimBtn.disabled).toBe(false);
    await act(async () => { claimBtn.click(); });
    expect(onPrepare).toHaveBeenCalledTimes(1);
  });

  it('allows executing a ready test task and shows no not-ready marker', async () => {
    const onPrepare = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 't', status: 'queued', title: 'TestTask', issueKind: 'test', isReady: true })],
      onPrepare,
    });
    expect(container.textContent).not.toContain('multica.taskManagement.readiness.notReady');
    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn.disabled).toBe(false);
    await act(async () => { claimBtn.click(); });
    expect(onPrepare).toHaveBeenCalledTimes(1);
  });

  it('shows no not-ready marker for a queued dev task even when the server reports isReady=false', async () => {
    // 非 test 不受就绪字段影响（服务端对非 test 亦可能回传 is_ready=false）。
    const onPrepare = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 'd', status: 'queued', title: 'DevTask', issueKind: 'dev', isReady: false })],
      onPrepare,
    });
    expect(container.textContent).not.toContain('multica.taskManagement.readiness.notReady');
    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn.disabled).toBe(false);
    await act(async () => { claimBtn.click(); });
    expect(onPrepare).toHaveBeenCalledTimes(1);
  });

  it('renders task timestamps in the local timezone, not raw UTC', async () => {
    const ts = '2026-08-06T02:30:00Z';
    const { container } = await renderBoard({
      tasks: [task({ id: 'q', status: 'queued', title: 'Todo', lastActivityAt: ts })],
    });
    expect(container.textContent).toContain(formatLocalDateTime(ts));
    expect(container.textContent).not.toContain('2026-08-06T02:30:00Z');
  });
});
