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
import { MulticaRemoteTaskList } from '@/components/conversation/MulticaRemoteTaskList';

const NO_TASKS_KEY = 'conversation.sidebar.multica.noTasksInWorkspace';
const NO_WS_KEY = 'conversation.sidebar.multica.noWorkspacesBound';
const RECENTLY_KEY = 'conversation.sidebar.multica.recentlyCompleted';

function baseVm(overrides: Record<string, unknown> = {}) {
  return {
    connected: true,
    workspaces: [],
    tasksByWorkspace: {},
    pinnedTasks: [],
    recentlyCompleted: [],
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
  onNewConversationInWorkspace = vi.fn(),
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
          onNewConversationInWorkspace={onNewConversationInWorkspace}
        />
      </ConversationComposerDraftProvider>,
    );
  });
  // flush getMulticaTasks promise + 订阅 promise
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  return { container, onSelectRun, onNewConversationInWorkspace, prefill };
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
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
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
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
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
    const onNewConversationInWorkspace = vi.fn();
    const prefill = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
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

    const { container, onNewConversationInWorkspace: navSpy, prefill: prefillSpy } =
      await renderList(onSelectRun, onNewConversationInWorkspace, prefill);

    // 领取按钮：queued 任务唯一的动作按钮，子节点为 null → textContent 为空。
    const claimButton = Array.from(container.querySelectorAll('button')).find(
      (b) => (b.textContent ?? '').trim() === '',
    ) as HTMLButtonElement;
    expect(claimButton).toBeTruthy();

    await act(async () => { claimButton.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    // claim 即领取（claim-at-click），不再原子 claim+start。
    expect(claimMulticaTask).toHaveBeenCalledWith('rt-1', 'ws-004');
    // 需求正文预填进 composer 草稿，并带上 multica 绑定（发送时据此分流远程 vs 本地）。
    expect(prefillSpy).toHaveBeenCalledWith('远程任务需求正文', {
      remoteTaskId: 'rt-1',
      workspaceId: 'ws-004',
      localProjectId: 'proj-004',
    });
    // 与本地工作空间「+」同一回调：导航到 conversation-home（仅 projectId），不再按 run 直达。
    expect(navSpy).toHaveBeenCalledWith('proj-004');
    expect(onSelectRun).not.toHaveBeenCalled();
  });

  it('falls back to the task title when the claim response has no requirement body', async () => {
    const onSelectRun = vi.fn();
    const onNewConversationInWorkspace = vi.fn();
    const prefill = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
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
      await renderList(onSelectRun, onNewConversationInWorkspace, prefill);

    const claimButton = Array.from(container.querySelectorAll('button')).find(
      (b) => (b.textContent ?? '').trim() === '',
    ) as HTMLButtonElement;
    await act(async () => { claimButton.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(prefillSpy).toHaveBeenCalledWith('Issue title', expect.objectContaining({ remoteTaskId: 'rt-1' }));
  });

  it('renders a recently completed task and navigates via onSelectRun on click', async () => {
    const onSelectRun = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: { 'ws-004': [] },
      recentlyCompleted: [
        {
          remoteTaskId: 'rt-done',
          localTaskId: 'local-done',
          runId: 'run-done',
          workspaceId: 'ws-004',
          projectId: 'proj-004',
          title: 'Completed task',
          status: 'completed',
          completedAt: '2026-08-06T10:00:00Z',
        },
      ],
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList(onSelectRun);

    // 「最近完成」分区标题 + 任务标题均渲染。
    expect(container.textContent).toContain(RECENTLY_KEY);
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
    const lastActivity = '2026-08-06T02:30:00Z';
    const completedAt = '2026-08-06T10:00:00Z';

    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [
        { id: 'ws-004', name: '004', slug: 'ws-004', localProjectId: 'proj-004', provider: 'claude-acp' },
      ],
      tasksByWorkspace: {
        'ws-004': [
          { id: 'rt-1', workspaceId: 'ws-004', title: 'Queued task', status: 'queued', retryable: true, lastActivityAt: lastActivity },
        ],
      },
      recentlyCompleted: [
        {
          remoteTaskId: 'rt-done',
          localTaskId: 'local-done',
          runId: 'run-done',
          workspaceId: 'ws-004',
          projectId: 'proj-004',
          title: 'Completed task',
          status: 'completed',
          completedAt,
        },
      ],
      lastActiveWorkspaceId: 'ws-004',
    }));

    const { container } = await renderList();

    // pending 行 lastActivityAt 与「最近完成」行 completedAt 均按本地时区渲染。
    expect(container.textContent).toContain(formatLocalDateTime(lastActivity));
    expect(container.textContent).toContain(formatLocalDateTime(completedAt));
    // 不应残留原始 UTC 字面量（旧实现 slice+replace 直接展示 UTC 墙钟）。
    expect(container.textContent).not.toContain('2026-08-06T02:30:00Z');
    expect(container.textContent).not.toContain('2026-08-06T10:00:00Z');
  });
});
