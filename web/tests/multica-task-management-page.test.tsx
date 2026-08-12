/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 跨渲染稳定（容器多个回调/渲染依赖 [t]）；返回 key 本身便于断言。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({ displayAppError: () => 'mock-error' }));

vi.mock('lucide-react', () => ({
  ChevronDown: () => null,
  Folders: () => null,
  Globe: () => null,
  Loader2: () => null,
  Plus: () => null,
  RotateCw: () => null,
  Trash2: () => null,
  User: () => null,
  Wifi: () => null,
  WifiOff: () => null,
}));

vi.mock('@/lib/utils', () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(' '),
}));

// composer draft：暴露 prefill spy，断言 claim 预填参数。
const draftMocks = vi.hoisted(() => ({ prefill: vi.fn() }));
vi.mock('@/lib/conversation-composer-draft', () => ({
  useConversationComposerDraft: () => ({ prefill: draftMocks.prefill }),
}));

vi.mock('@/components/PageScaffold', () => ({
  Page: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  PageHeader: ({ title, icon, actions }: { title?: ReactNode; icon?: ReactNode; actions?: ReactNode }) => (
    <div>
      <span data-testid="header-icon">{icon}</span>
      <div data-testid="title">{title}</div>
      <div data-testid="actions">{actions}</div>
    </div>
  ),
}));

// Select 桩成原生 <select>：onChange → onValueChange，便于 jsdom 触发来源切换。
vi.mock('@/components/ui/select', () => ({
  Select: ({ value, onValueChange, children }: { value?: string; onValueChange?: (v: string) => void; children?: ReactNode }) => (
    <select value={value} onChange={(e) => onValueChange?.(e.target.value)}>{children}</select>
  ),
  SelectTrigger: () => null,
  SelectContent: ({ children }: { children?: ReactNode }) => <>{children}</>,
  SelectItem: ({ value, children }: { value: string; children?: ReactNode }) => <option value={value}>{children}</option>,
  SelectValue: () => null,
}));

vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children?: ReactNode }) => <>{children}</>,
  DropdownMenuTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>,
  DropdownMenuContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  DropdownMenuItem: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
  DropdownMenuSeparator: () => <hr />,
}));

// Popover 桩：始终渲染 trigger + content 内联，使测试可直接交互 workspace picker 行。
vi.mock('@/components/ui/popover', () => ({
  Popover: ({ children }: { children?: ReactNode }) => <>{children}</>,
  PopoverTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>,
  PopoverContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));

// AlertDialog 桩：按 open 门控渲染；AlertDialogAction/Cancel 暴露为按钮，使测试可单击确认/取消。
vi.mock('@/components/ui/alert-dialog', () => ({
  AlertDialog: ({ children, open }: { children?: ReactNode; open?: boolean }) =>
    open ? <>{children}</> : null,
  AlertDialogContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AlertDialogHeader: ({ children }: { children?: ReactNode }) => <>{children}</>,
  AlertDialogFooter: ({ children }: { children?: ReactNode }) => <>{children}</>,
  AlertDialogTitle: ({ children }: { children?: ReactNode }) => <h2>{children}</h2>,
  AlertDialogDescription: ({ children }: { children?: ReactNode }) => <p>{children}</p>,
  AlertDialogAction: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
  AlertDialogCancel: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

vi.mock('@/components/ui/separator', () => ({
  Separator: () => <hr />,
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

// Board 桩：渲染收到的任务标题 + claim/cancel/open 触发按钮，把容器逻辑与看板内部解耦。
vi.mock('@/components/conversation/MulticaRemoteTaskBoard', () => ({
  MulticaRemoteTaskBoard: ({ tasks, onClaim, onCancel, onSelectRun }: {
    tasks: { id: string; title: string; status: string; projectId?: string | null; localTaskId?: string | null; runId?: string | null }[];
    onClaim: (t: unknown) => void;
    onCancel: (t: unknown) => void;
    onSelectRun: (p: string, t: string, r: string) => void;
  }) => (
    <div data-testid="board">
      {tasks.map((task) => (
        <div key={task.id} data-testid={`task-${task.id}`}>
          <span>{task.title}</span>
          {task.status === 'queued' && (
            <button aria-label="conversation.sidebar.multica.executeTask" onClick={() => onClaim(task)} />
          )}
          {task.status === 'running' && (
            <button aria-label="conversation.sidebar.multica.cancelTask" onClick={() => onCancel(task)} />
          )}
          {task.projectId && task.localTaskId && task.runId && (
            <button data-testid={`open-${task.id}`} onClick={() => onSelectRun(task.projectId!, task.localTaskId!, task.runId!)} />
          )}
        </div>
      ))}
    </div>
  ),
}));

vi.mock('@/components/conversation/MulticaAddWorkspaceDialog', () => ({
  MulticaAddWorkspaceDialog: () => null,
}));

const mocks = vi.hoisted(() => ({
  getMulticaTasks: vi.fn(),
  getMulticaSettings: vi.fn(),
  connectMultica: vi.fn(),
  disconnectMultica: vi.fn(),
  claimMulticaTask: vi.fn(),
  cancelMulticaTask: vi.fn(),
  removeMulticaWorkspace: vi.fn(),
  setActiveMulticaWorkspace: vi.fn(),
  openExternalUrl: vi.fn(),
  subscribeMulticaTaskUpdates: vi.fn(),
  subscribeMulticaSettingsUpdates: vi.fn(),
}));

vi.mock('@/api', () => ({
  getMulticaTasks: mocks.getMulticaTasks,
  getMulticaSettings: mocks.getMulticaSettings,
  connectMultica: mocks.connectMultica,
  disconnectMultica: mocks.disconnectMultica,
  claimMulticaTask: mocks.claimMulticaTask,
  cancelMulticaTask: mocks.cancelMulticaTask,
  removeMulticaWorkspace: mocks.removeMulticaWorkspace,
  setActiveMulticaWorkspace: mocks.setActiveMulticaWorkspace,
  openExternalUrl: mocks.openExternalUrl,
  subscribeMulticaTaskUpdates: mocks.subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates: mocks.subscribeMulticaSettingsUpdates,
}));

import { MulticaTaskManagementPage } from '@/pages/MulticaTaskManagementPage';
import type { RemoteConversationSidebarVm, RemoteTaskVm } from '@/types';

const noopUnlisten = () => {};
const {
  getMulticaTasks,
  getMulticaSettings,
  claimMulticaTask,
  cancelMulticaTask,
  setActiveMulticaWorkspace,
  subscribeMulticaTaskUpdates,
  subscribeMulticaSettingsUpdates,
} = mocks;

function baseVm(overrides: Partial<RemoteConversationSidebarVm> = {}): RemoteConversationSidebarVm {
  return {
    connected: true,
    workspaces: [],
    tasksByWorkspace: {},
    pinnedTasks: [],
    lastActiveWorkspaceId: null,
    ...overrides,
  } as RemoteConversationSidebarVm;
}

function baseSettings(overrides: Record<string, unknown> = {}) {
  return {
    enabled: true,
    toggleLocked: false,
    multicaBaseUrl: 'https://m.example',
    multicaAppUrl: 'https://app.example',
    patSet: true,
    daemonIdSet: true,
    workspaces: [],
    activeWorkspaceId: null,
    defaultProvider: 'claude-acp',
    connected: true,
    connectedAccount: { name: 'Demo', email: 'demo@example.com' },
    ...overrides,
  };
}

const ws004 = { id: 'ws-004', name: '004', slug: 'ws-004', provider: 'claude-acp' };
const ws005 = { id: 'ws-005', name: '005', slug: 'ws-005', provider: 'claude-acp' };

beforeEach(() => {
  vi.clearAllMocks();
  getMulticaSettings.mockResolvedValue(baseSettings());
  setActiveMulticaWorkspace.mockResolvedValue(baseSettings());
  subscribeMulticaTaskUpdates.mockResolvedValue(noopUnlisten);
  subscribeMulticaSettingsUpdates.mockResolvedValue(noopUnlisten);
});

afterEach(() => {
  document.body.innerHTML = '';
});

async function renderPage(
  onSelectRun = vi.fn(),
  onPrepareMulticaTask = vi.fn(),
) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <MulticaTaskManagementPage
        onSelectRun={onSelectRun}
        onPrepareMulticaTask={onPrepareMulticaTask}
      />,
    );
  });
  // flush mount fetch（tasks + settings）+ 订阅 promise。
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  return { container, onSelectRun, onPrepareMulticaTask };
}

describe('MulticaTaskManagementPage (container)', () => {
  it('fetches tasks + settings on mount and subscribes to both update channels', async () => {
    getMulticaTasks.mockResolvedValue(baseVm());
    await renderPage();

    expect(getMulticaTasks).toHaveBeenCalledTimes(1);
    expect(getMulticaSettings).toHaveBeenCalledTimes(1);
    expect(subscribeMulticaTaskUpdates).toHaveBeenCalledTimes(1);
    expect(subscribeMulticaSettingsUpdates).toHaveBeenCalledTimes(1);
  });

  it('shows the connect prompt when not connected', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({ connected: false }));
    const { container } = await renderPage();

    expect(container.textContent).toContain('conversation.sidebar.multica.emptyTitle');
    expect(container.textContent).toContain('conversation.sidebar.multica.connectButton');
  });

  it('shows the no-workspaces empty state when connected but no workspaces are bound', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({ workspaces: [] }));
    const { container } = await renderPage();

    expect(container.textContent).toContain('conversation.sidebar.multica.noWorkspacesBound');
    expect(container.textContent).toContain('conversation.sidebar.multica.addWorkspace');
  });

  it('renders only the effective workspace\'s tasks (default = lastActiveWorkspaceId)', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004, ws005],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-a', workspaceId: 'ws-004', title: 'Task A', status: 'queued', retryable: false } as RemoteTaskVm],
        'ws-005': [{ id: 'rt-b', workspaceId: 'ws-005', title: 'Task B', status: 'queued', retryable: false } as RemoteTaskVm],
      },
    }));
    const { container } = await renderPage();

    expect(container.textContent).toContain('Task A');
    // 默认 lastActive = ws-004 → ws-005 的任务不展示。
    expect(container.textContent).not.toContain('Task B');
  });

  it('switches the filtered workspace via the popover picker (and persists active)', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004, ws005],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-a', workspaceId: 'ws-004', title: 'Task A', status: 'queued', retryable: false } as RemoteTaskVm],
        'ws-005': [{ id: 'rt-b', workspaceId: 'ws-005', title: 'Task B', status: 'queued', retryable: false } as RemoteTaskVm],
      },
    }));
    const { container } = await renderPage();

    // 工作空间 Popover picker 每行一个 data-testid="ws-pick-{id}" 按钮。
    const pickBtn = container.querySelector('[data-testid="ws-pick-ws-005"]') as HTMLButtonElement;
    expect(pickBtn).toBeTruthy();

    await act(async () => { pickBtn.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(setActiveMulticaWorkspace).toHaveBeenCalledWith('ws-005');
    expect(container.textContent).toContain('Task B');
    expect(container.textContent).not.toContain('Task A');
  });

  it('removes a workspace via the picker row trash + AlertDialog confirm', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004, ws005],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {},
    }));
    mocks.removeMulticaWorkspace.mockResolvedValue(baseSettings());
    const { container } = await renderPage();

    // 行内移除按钮（data-testid="ws-remove-ws-005"）→ 打开 AlertDialog。
    const removeBtn = container.querySelector('[data-testid="ws-remove-ws-005"]') as HTMLButtonElement;
    expect(removeBtn).toBeTruthy();
    await act(async () => { removeBtn.click(); });

    // AlertDialog 确认按钮（文案 = common.confirm）。
    const confirmBtn = Array.from(container.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'common.confirm',
    ) as HTMLButtonElement;
    expect(confirmBtn).toBeTruthy();
    await act(async () => { confirmBtn.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.removeMulticaWorkspace).toHaveBeenCalledWith('ws-005');
  });

  it('renders the bottom toolbar only when connected (source lives in header, footer is gated)', async () => {
    // 来源下拉已上移页头（常驻）；底部工具条（刷新/账号等）受 source + 连接态门控。
    // 未连接 → 无刷新按钮（footer 不渲染）。
    getMulticaTasks.mockResolvedValue(baseVm({ connected: false }));
    const { container: disconnectedContainer } = await renderPage();
    // 页头来源标签常驻（即便未连接）。
    expect(disconnectedContainer.textContent).toContain('multica.taskManagement.source.label');
    expect(disconnectedContainer.querySelector('button[aria-label="common.refresh"]')).toBeNull();

    // 已连接 → footer 渲染，刷新按钮出现。
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {},
    }));
    const { container: connectedContainer } = await renderPage();
    expect(connectedContainer.querySelector('button[aria-label="common.refresh"]')).not.toBeNull();
  });

  it('claims a queued task, prefills the composer draft, then navigates to conversation-home', async () => {
    const onSelectRun = vi.fn();
    const onPrepareMulticaTask = vi.fn();
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-1', workspaceId: 'ws-004', title: 'Some task', status: 'queued', retryable: false } as RemoteTaskVm],
      },
    }));
    // claim 响应回填需求正文（pending 列表只有 thread_name，正文仅 claim 响应里有）。
    claimMulticaTask.mockResolvedValue({
      id: 'rt-1', issueId: null, status: 'queued', retryable: false,
      workspaceId: 'ws-004', title: 'Some task', requirement: '远程任务需求正文', lastActivityAt: null,
    });
    const { container } = await renderPage(onSelectRun, onPrepareMulticaTask);

    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    expect(claimBtn).toBeTruthy();
    await act(async () => { claimBtn.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    // claim 即领取（claim-at-click），预填正文 + multica 绑定，落 conversation-home。
    expect(claimMulticaTask).toHaveBeenCalledWith('rt-1', 'ws-004');
    expect(draftMocks.prefill).toHaveBeenCalledWith('远程任务需求正文', { remoteTaskId: 'rt-1', workspaceId: 'ws-004', title: 'Some task' });
    expect(onPrepareMulticaTask).toHaveBeenCalledWith();
    expect(onSelectRun).not.toHaveBeenCalled();
  });

  it('falls back to the task title when the claim response has no requirement body', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-1', workspaceId: 'ws-004', title: 'Issue title', status: 'queued', retryable: false } as RemoteTaskVm],
      },
    }));
    claimMulticaTask.mockResolvedValue({
      id: 'rt-1', issueId: 'issue-1', status: 'queued', retryable: true,
      workspaceId: 'ws-004', title: 'Issue title', requirement: null, lastActivityAt: null,
    });
    const { container } = await renderPage();

    const claimBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.executeTask"]') as HTMLButtonElement;
    await act(async () => { claimBtn.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(draftMocks.prefill).toHaveBeenCalledWith('Issue title', expect.objectContaining({ remoteTaskId: 'rt-1' }));
  });

  it('cancels a running task via cancelMulticaTask', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {
        'ws-004': [{ id: 'rt-run', workspaceId: 'ws-004', title: 'In flight', status: 'running', retryable: false } as RemoteTaskVm],
      },
    }));
    cancelMulticaTask.mockResolvedValue(undefined);
    const { container } = await renderPage();

    const cancelBtn = container.querySelector('button[aria-label="conversation.sidebar.multica.cancelTask"]') as HTMLButtonElement;
    expect(cancelBtn).toBeTruthy();
    await act(async () => { cancelBtn.click(); });
    await act(async () => { await Promise.resolve(); });

    expect(cancelMulticaTask).toHaveBeenCalledWith('rt-run');
  });

  it('manual refresh re-fetches the task list without remounting', async () => {
    getMulticaTasks.mockResolvedValue(baseVm({
      workspaces: [ws004],
      lastActiveWorkspaceId: 'ws-004',
      tasksByWorkspace: {},
    }));
    const { container } = await renderPage();

    // 挂载拉取一次。
    expect(getMulticaTasks).toHaveBeenCalledTimes(1);

    // 刷新按钮按 aria-label 定位（RotateCw 图标桩成 null，按钮在底部工具条）。
    const refreshBtn = container.querySelector('button[aria-label="common.refresh"]') as HTMLButtonElement;
    expect(refreshBtn).toBeTruthy();
    await act(async () => { refreshBtn.click(); });
    await act(async () => { await Promise.resolve(); });

    // 刷新触发再次拉取（count → 2），无需重进页面。
    expect(getMulticaTasks).toHaveBeenCalledTimes(2);
  });
});
