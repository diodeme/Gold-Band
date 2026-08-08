/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 跨渲染稳定（组件多个 useEffect 依赖 [t] / [refresh]），返回 key 本身便于断言。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({
  displayAppError: () => 'mock-error',
}));

vi.mock('lucide-react', () => ({
  Ban: () => null,
  ChevronDown: () => null,
  Loader2: () => null,
  Play: () => null,
  Plus: () => null,
  RotateCw: () => null,
  Server: () => null,
  Wifi: () => null,
  WifiOff: () => null,
}));

vi.mock('@/lib/utils', () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(' '),
}));

vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));
vi.mock('@/components/ui/badge', () => ({
  Badge: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));

// 子弹窗桩成空，避免引入 dialog/select 的额外 mock（默认 open=false 也不渲染）。
vi.mock('@/components/conversation/MulticaAddWorkspaceDialog', () => ({
  MulticaAddWorkspaceDialog: () => null,
}));

const mocks = vi.hoisted(() => ({
  getMulticaTasks: vi.fn(),
  connectMultica: vi.fn(),
  cancelMulticaTask: vi.fn(),
  rerunMulticaTask: vi.fn(),
  claimMulticaTask: vi.fn(),
  subscribeMulticaTaskUpdates: vi.fn(),
  subscribeMulticaSettingsUpdates: vi.fn(),
}));

vi.mock('@/api', () => ({
  getMulticaTasks: mocks.getMulticaTasks,
  connectMultica: mocks.connectMultica,
  cancelMulticaTask: mocks.cancelMulticaTask,
  rerunMulticaTask: mocks.rerunMulticaTask,
  claimMulticaTask: mocks.claimMulticaTask,
  subscribeMulticaTaskUpdates: mocks.subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates: mocks.subscribeMulticaSettingsUpdates,
}));

const noopUnlisten = () => {};
const {
  getMulticaTasks,
  claimMulticaTask,
  subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates,
} = mocks;

import { formatLocalDateTime } from '@/lib/datetime';
import { ConversationComposerDraftProvider } from '@/lib/conversation-composer-draft';
import { MulticaRemoteTaskList, MULTICA_STATUS_TONE } from '@/components/conversation/MulticaRemoteTaskList';

const NO_TASKS_KEY = 'conversation.sidebar.multica.noTasksInWorkspace';
const NO_WS_KEY = 'conversation.sidebar.multica.noWorkspacesBound';

function baseVm(overrides: Record<string, unknown> = {}) {
  return {
    connected: true,
    workspaces: [],
    tasksByWorkspace: {},
    pinnedTasks: [],
    lastActiveWorkspaceId: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  subscribeMulticaTaskUpdates.mockResolvedValue(noopUnlisten);
  subscribeMulticaSettingsUpdates.mockResolvedValue(noopUnlisten);
});

afterEach(() => {
  document.body.innerHTML = '';
});

async function renderList(
  onSelectRun = vi.fn(),
  onPrepareMulticaTask = vi.fn(),
  prefill = vi.fn(),
) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  // MulticaRemoteTaskList 落在 composer draft boundary 内，通过 context 预填草稿；
  // 测试注入带 prefill spy 的 context value，便于断言预填参数。
  const draftValue = {
    draft: { content: '', attachments: [], multica: null },
    setContent: vi.fn(),
    setAttachments: vi.fn(),
    prefill,
    reset: vi.fn(),
  };
  await act(async () => {
    root.render(
      <ConversationComposerDraftProvider value={draftValue}>
        <MulticaRemoteTaskList
          onSelectRun={onSelectRun}
          onPrepareMulticaTask={onPrepareMulticaTask}
        />
      </ConversationComposerDraftProvider>,
    );
  });
  // flush getMulticaTasks promise + 订阅 promise
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  return { container, onSelectRun, onPrepareMulticaTask, prefill };
}

function findButtonByText(container: HTMLElement, text: string): HTMLButtonElement {
  const buttons = Array.from(container.querySelectorAll('button'));
  return buttons.find((b) => (b.textContent ?? '').includes(text)) as HTMLButtonElement;
}

describe('multica remote task list', () => {
  it('shows a bound workspace group even when it has no tasks (not the no-workspaces empty state)', async () => {
    // 连接 + 已绑定 004 + 该 workspace 暂无任务（未 register / 服务端无 queued）。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {},
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList();

    // 渲染修复：已绑定 workspace 始终成组展示（即便 0 任务）。
    expect(container.textContent).toContain('004');
    expect(container.textContent).toContain(NO_TASKS_KEY);
    // 不应回退到「尚未绑定工作空间」空状态。
    expect(container.textContent).not.toContain(NO_WS_KEY);
  });

  it('shows the connect prompt when not connected', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({ connected: false }));

    const { container } = await renderList();

    expect(container.textContent).toContain('conversation.sidebar.multica.emptyTitle');
    expect(container.textContent).toContain('conversation.sidebar.multica.connectButton');
  });

  it('subscribes to both task and settings update events', async () => {
    getMulticaTasks.mockResolvedValue(baseVm());

    await renderList();

    expect(subscribeMulticaTaskUpdates).toHaveBeenCalledTimes(1);
    expect(subscribeMulticaSettingsUpdates).toHaveBeenCalledTimes(1);
  });

  it('collapses and expands a workspace group via its header', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Some task', status: 'queued', retryable: true },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList();

    // 初始展开：任务可见。
    expect(container.textContent).toContain('Some task');

    const header = findButtonByText(container, '004');
    expect(header).toBeTruthy();

    // 折叠：任务隐藏。
    await act(async () => { header.click(); });
    expect(container.textContent).not.toContain('Some task');

    // 再次点击展开：任务恢复。
    await act(async () => { header.click(); });
    expect(container.textContent).toContain('Some task');
  });

  it('claims a queued task, prefills the composer draft, then navigates to conversation-home (same as local "+")', async () => {
    const onSelectRun = vi.fn();
    const onPrepareMulticaTask = vi.fn();
    const prefill = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Some task', status: 'queued', retryable: true },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));
    // claim 响应回填需求正文（pending 列表只有 thread_name，正文仅 claim 响应里有）。
    claimMulticaTask.mockResolvedValue({
      id: 'rt-1', issueId: null, status: 'queued', retryable: true,
      workspaceId: 'ws-004', title: 'Some task', requirement: '远程任务需求正文', lastActivityAt: null,
    });

    const { container, onPrepareMulticaTask: navSpy, prefill: prefillSpy } =
      await renderList(onSelectRun, onPrepareMulticaTask, prefill);

    // 领取按钮：queued 任务唯一的动作按钮，子节点为 null → textContent 为空。
    // 领取按钮按 aria-label 定位（claim Play 图标桩成 null；图标按钮均带 aria-label，避免与刷新等空文本按钮误匹配）。
    const claimButton = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimButton).toBeTruthy();

    await act(async () => { claimButton.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    // claim 即领取（claim-at-click），不再原子 claim+start。
    expect(claimMulticaTask).toHaveBeenCalledWith('rt-1', 'ws-004');
    // 需求正文预填进 composer 草稿，并带上 multica 绑定（remoteTaskId + workspaceId，本地工作区执行时选）。
    expect(prefillSpy).toHaveBeenCalledWith('远程任务需求正文', {
      remoteTaskId: 'rt-1',
      workspaceId: 'ws-004',
    });
    // 落 conversation-home（无参回调）：本地工作区由 App 预选最近活跃，用户在 composer 下拉改（决策 c/d）。
    expect(navSpy).toHaveBeenCalledWith();
    expect(onSelectRun).not.toHaveBeenCalled();
  });

  it('falls back to the task title when the claim response has no requirement body', async () => {
    const onSelectRun = vi.fn();
    const onPrepareMulticaTask = vi.fn();
    const prefill = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Issue title', status: 'queued', retryable: true },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));
    // issue 型任务：无需求正文来源，requirement 为 null → 预填回退到 title。
    claimMulticaTask.mockResolvedValue({
      id: 'rt-1', issueId: 'issue-1', status: 'queued', retryable: true,
      workspaceId: 'ws-004', title: 'Issue title', requirement: null, lastActivityAt: null,
    });

    const { container, prefill: prefillSpy } =
      await renderList(onSelectRun, onPrepareMulticaTask, prefill);

    // 领取按钮按 aria-label 定位（claim Play 图标桩成 null；图标按钮均带 aria-label，避免与刷新等空文本按钮误匹配）。
    const claimButton = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    await act(async () => { claimButton.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(prefillSpy).toHaveBeenCalledWith('Issue title', expect.objectContaining({ remoteTaskId: 'rt-1' }));
  });

  it('renders a completed task in its workspace group and navigates via onSelectRun on click', async () => {
    // 改动六：终态任务不再进扁平「最近完成」桶，而是并入所属工作空间组（带本地 run 链接）。
    const onSelectRun = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          {
            id: 'rt-done',
            issueId: null,
            workspaceId: 'ws-004',
            title: 'Completed task',
            status: 'completed',
            retryable: false,
            lastActivityAt: '2026-08-06T10:00:00Z',
            localTaskId: 'local-done',
            runId: 'run-done',
            projectId: 'proj-004',
            requirement: null,
          },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList(onSelectRun);

    // 终态任务并入工作空间组渲染（无独立「最近完成」分区）。
    expect(container.textContent).toContain('Completed task');

    const row = findButtonByText(container, 'Completed task');
    expect(row).toBeTruthy();

    await act(async () => { row.click(); });

    // 点击直达本地会话（projectId + localTaskId + runId）。
    expect(onSelectRun).toHaveBeenCalledWith('proj-004', 'local-done', 'run-done');
  });

  it('renders task timestamps in the local timezone, not raw UTC', async () => {
    // 时间显示修复（接入方案 M5-p）：UTC 存储、本地时区展示。复用真实
    // formatLocalDateTime 计算期望值，避免绑定测试机时区（任何 tz 下都成立）。
    // 改动六：终态行 lastActivityAt 即 completed_at（from_completed 映射），与 pending 行同一渲染路径。
    const lastActivity = '2026-08-06T02:30:00Z';
    const completedAt = '2026-08-06T10:00:00Z';

    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Queued task', status: 'queued', retryable: true, lastActivityAt: lastActivity },
          {
            id: 'rt-done',
            issueId: null,
            workspaceId: 'ws-004',
            title: 'Completed task',
            status: 'completed',
            retryable: false,
            lastActivityAt: completedAt,
            localTaskId: 'local-done',
            runId: 'run-done',
            projectId: 'proj-004',
            requirement: null,
          },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList();

    // pending 行 lastActivityAt 与终态行（completed_at → lastActivityAt）均按本地时区渲染。
    expect(container.textContent).toContain(formatLocalDateTime(lastActivity));
    expect(container.textContent).toContain(formatLocalDateTime(completedAt));
    // 不应残留原始 UTC 字面量（旧实现 slice+replace 直接展示 UTC 墙钟）。
    expect(container.textContent).not.toContain('2026-08-06T02:30:00Z');
    expect(container.textContent).not.toContain('2026-08-06T10:00:00Z');
  });

  it('renders a running task in its workspace group with a status badge, click-to-jump and a cancel action', async () => {
    // 改动七：执行中任务（active_runs → running 行）不再从侧栏消失，留在所属工作空间组，
    // 带「进行中」标识、整行点击直达进行中会话，并保留 Cancel 动作。
    const onSelectRun = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          {
            id: 'rt-run',
            issueId: 'iss-run',
            workspaceId: 'ws-004',
            title: 'In flight task',
            status: 'running',
            retryable: false,
            lastActivityAt: '2026-08-07T03:00:00Z',
            localTaskId: 'task-run',
            runId: 'run-run',
            projectId: 'proj-004',
            requirement: null,
          },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList(onSelectRun);

    // 进行中任务留在工作空间组（不再消失）。
    expect(container.textContent).toContain('In flight task');
    // 徽标走 i18n key（mock t 返回 key 本身）→ 进行中标识就位（前端按 key 映射「进行中」）。
    expect(container.textContent).toContain('conversation.sidebar.multica.status.running');

    // 整行点击直达进行中的会话（projectId + localTaskId + runId）。
    const row = findButtonByText(container, 'In flight task');
    expect(row).toBeTruthy();
    await act(async () => { row.click(); });
    expect(onSelectRun).toHaveBeenCalledWith('proj-004', 'task-run', 'run-run');

    // 进行中行保留 Cancel 动作（cancelMulticaTask 入口；tooltip 文案渲染即可见）。
    expect(container.textContent).toContain('conversation.sidebar.multica.cancelTask');
  });

  it('renders a status badge for a queued task (待办)', async () => {
    // point1：queued 任务显状态徽章（待办，看板词汇）。mock t 返回 key，断言 key 出现即证徽章按 status 渲染。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [{ id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' }],
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-1', workspaceId: 'ws-004', title: 'Todo task', status: 'queued', retryable: true }],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));
    const { container } = await renderList();
    expect(container.textContent).toContain('conversation.sidebar.multica.status.queued');
  });

  it('renders a task-count indicator next to a workspace that has tasks', async () => {
    // point3：有任务的工作空间，名称右侧渲染计数「（N个任务）」。mock t 返回 key，断言 key 出现即证计数按 tasks.length 渲染。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [{ id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' }],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Task A', status: 'queued', retryable: true },
          { id: 'rt-2', workspaceId: 'ws-004', title: 'Task B', status: 'completed', retryable: false },
        ],
      },
      lastActiveWorkspaceId: 'ws-004',
    }));
    const { container } = await renderList();
    expect(container.textContent).toContain('conversation.sidebar.multica.taskCount');
  });

  it('omits the task-count indicator for an empty workspace', async () => {
    // point3：0 任务时不渲染计数，避免与空状态文案「该工作空间下暂无远程任务」重复。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [{ id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' }],
      tasksByWorkspace: {},
      lastActiveWorkspaceId: 'ws-004',
    }));
    const { container } = await renderList();
    expect(container.textContent).not.toContain('conversation.sidebar.multica.taskCount');
  });

  it('manual refresh re-fetches the task list without remounting', async () => {
    // point3：刷新按钮免切换页面重进即可拉最新列表。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [{ id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' }],
      tasksByWorkspace: {},
      lastActiveWorkspaceId: 'ws-004',
    }));
    const { container } = await renderList();

    // 挂载拉取一次。
    expect(getMulticaTasks).toHaveBeenCalledTimes(1);

    // 刷新按钮：aria-label = common.refresh（RotateCw 图标桩成 null，按 aria-label 定位）。
    const refreshBtn = container.querySelector('button[aria-label="common.refresh"]') as HTMLButtonElement;
    expect(refreshBtn).toBeTruthy();
    await act(async () => { refreshBtn.click(); });
    await act(async () => { await Promise.resolve(); });

    // 刷新触发再次拉取（count → 2），无需重进页面。
    expect(getMulticaTasks).toHaveBeenCalledTimes(2);
  });
});

describe('multica status tone config', () => {
  it('maps every canonical status to its board-vocabulary color (灰/黄/绿/红)', () => {
    // point2：4 个 canonical status 各有色调，锁定看板词汇配色（待办=灰、进行中=黄、已完成=绿、失败=红）。
    expect(MULTICA_STATUS_TONE.queued).toMatch(/muted/);
    expect(MULTICA_STATUS_TONE.running).toMatch(/amber/);
    expect(MULTICA_STATUS_TONE.completed).toMatch(/emerald/);
    expect(MULTICA_STATUS_TONE.failed).toMatch(/destructive/);
    // 无遗漏：恰好这 4 个 canonical status。
    expect(Object.keys(MULTICA_STATUS_TONE).sort()).toEqual(['completed', 'failed', 'queued', 'running']);
  });
});
