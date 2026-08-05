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
  startMulticaRemoteTask: vi.fn(),
  subscribeMulticaTaskUpdates: vi.fn(),
  subscribeMulticaSettingsUpdates: vi.fn(),
}));

vi.mock('@/api', () => ({
  getMulticaTasks: mocks.getMulticaTasks,
  connectMultica: mocks.connectMultica,
  cancelMulticaTask: mocks.cancelMulticaTask,
  rerunMulticaTask: mocks.rerunMulticaTask,
  startMulticaRemoteTask: mocks.startMulticaRemoteTask,
  subscribeMulticaTaskUpdates: mocks.subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates: mocks.subscribeMulticaSettingsUpdates,
}));

const noopUnlisten = () => {};
const {
  getMulticaTasks,
  startMulticaRemoteTask,
  subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates,
} = mocks;

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

async function renderList(onSelectRun = vi.fn()) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<MulticaRemoteTaskList onSelectRun={onSelectRun} />);
  });
  // flush getMulticaTasks promise + 订阅 promise
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  return { container, onSelectRun };
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

  it('claims a queued task and navigates via onSelectRun with localTaskId + runId', async () => {
    const onSelectRun = vi.fn();
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
    startMulticaRemoteTask.mockResolvedValue({ localTaskId: 'local-1', runId: 'run-1' });

    const { container } = await renderList(onSelectRun);

    // 领取按钮：queud 任务唯一的动作按钮，子节点为 null → textContent 为空。
    const claimButton = Array.from(container.querySelectorAll('button')).find(
      (b) => (b.textContent ?? '').trim() === '',
    ) as HTMLButtonElement;
    expect(claimButton).toBeTruthy();

    await act(async () => { claimButton.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(startMulticaRemoteTask).toHaveBeenCalledWith('rt-1', 'ws-004');
    // 按 run 直达本地会话：projectId + localTaskId + runId。
    expect(onSelectRun).toHaveBeenCalledWith('proj-004', 'local-1', 'run-1');
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
});
