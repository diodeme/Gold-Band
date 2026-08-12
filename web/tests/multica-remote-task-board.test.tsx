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
  BOARD_COLUMNS,
  MULTICA_STATUS_TONE,
} from '@/components/conversation/MulticaRemoteTaskBoard';
import type { RemoteTaskVm } from '@/types';

function task(overrides: Partial<RemoteTaskVm> = {}): RemoteTaskVm {
  return {
    id: 'rt-1',
    issueId: null,
    status: 'queued',
    retryable: false,
    workspaceId: 'ws-1',
    title: 'Task',
    requirement: null,
    lastActivityAt: null,
    localTaskId: null,
    runId: null,
    projectId: null,
    ...overrides,
  } as RemoteTaskVm;
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

async function renderBoard(props: {
  tasks: RemoteTaskVm[];
  busyTaskId?: string | null;
  onClaim?: (t: RemoteTaskVm) => void;
  onCancel?: (t: RemoteTaskVm) => void;
  onSelectRun?: (p: string, t: string, r: string) => void;
}) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  const onClaim = props.onClaim ?? vi.fn();
  const onCancel = props.onCancel ?? vi.fn();
  const onSelectRun = props.onSelectRun ?? vi.fn();
  await act(async () => {
    root.render(
      <MulticaRemoteTaskBoard
        tasks={props.tasks}
        busyTaskId={props.busyTaskId ?? null}
        onClaim={onClaim}
        onCancel={onCancel}
        onSelectRun={onSelectRun}
      />,
    );
  });
  return { container, onClaim, onCancel, onSelectRun };
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

  it('renders a claim button only for queued tasks and forwards onClaim', async () => {
    const onClaim = vi.fn();
    const { container } = await renderBoard({
      tasks: [task({ id: 'q', status: 'queued', title: 'Todo' })],
      onClaim,
    });
    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn).toBeTruthy();
    await act(async () => { claimBtn.click(); });
    expect(onClaim).toHaveBeenCalledTimes(1);
    expect((onClaim.mock.calls[0] as [RemoteTaskVm])[0].id).toBe('q');
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

  it('renders task timestamps in the local timezone, not raw UTC', async () => {
    const ts = '2026-08-06T02:30:00Z';
    const { container } = await renderBoard({
      tasks: [task({ id: 'q', status: 'queued', title: 'Todo', lastActivityAt: ts })],
    });
    expect(container.textContent).toContain(formatLocalDateTime(ts));
    expect(container.textContent).not.toContain('2026-08-06T02:30:00Z');
  });
});
