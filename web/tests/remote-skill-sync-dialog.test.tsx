/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 必须跨渲染稳定：被测组件的 useEffect 依赖 [open, t]，若每次渲染返回新的 t，
// 会触发 effect 反复重跑 → 无限渲染循环、用例超时。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({
  displayAppError: () => 'mock-error',
}));

vi.mock('lucide-react', () => ({
  Loader2: () => null,
  RefreshCw: () => null,
}));

vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) => (open ? children : null),
  DialogContent: ({ children }: { children: ReactNode }) => children,
  DialogHeader: ({ children }: { children: ReactNode }) => children,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
  DialogFooter: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

vi.mock('@/components/ui/select', () => ({
  Select: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectTrigger: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectValue: ({ placeholder }: { placeholder?: string }) => <span>{placeholder ?? ''}</span>,
  SelectContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectItem: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

// Checkbox 桩按当前 checked 值转发点击：非勾选（false/indeterminate）→ true，勾选 → false，
// 与 Radix Checkbox 的点击语义一致（indeterminate 点击即全选）。
vi.mock('@/components/ui/checkbox', () => ({
  Checkbox: (props: {
    checked?: boolean | 'indeterminate';
    disabled?: boolean;
    className?: string;
    onCheckedChange?: (checked: boolean) => void;
  }) => (
    <input
      type="checkbox"
      className={props.className}
      disabled={props.disabled}
      data-checked={String(props.checked)}
      onClick={() => props.onCheckedChange?.(props.checked !== true)}
    />
  ),
}));

vi.mock('@/components/ui/badge', () => ({
  Badge: ({ children }: { children: ReactNode }) => <span>{children}</span>,
}));

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

// 就地连接弹窗桩：按 open 门控渲染，透传 sourceLabel（注册表名片）+ 暴露 onConnected
// 触发器（模拟连接成功路径），供未连接分支的连接入口契约断言。
vi.mock('@/components/conversation/MulticaConnectDialog', () => ({
  MulticaConnectDialog: (props: {
    open?: boolean;
    sourceLabel?: string;
    onConnected?: () => void;
  }) =>
    props.open ? (
      <div data-testid="connect-dialog" data-source={props.sourceLabel ?? ''}>
        <button type="button" data-testid="connect-done" onClick={() => props.onConnected?.()}>
          done
        </button>
      </div>
    ) : null,
}));

const mocks = vi.hoisted(() => ({
  getMulticaSettings: vi.fn(),
  listRemoteSkills: vi.fn(),
  pullRemoteSkills: vi.fn(),
}));

vi.mock('@/api', () => ({
  getMulticaSettings: mocks.getMulticaSettings,
  listRemoteSkills: mocks.listRemoteSkills,
  pullRemoteSkills: mocks.pullRemoteSkills,
}));

import { RemoteSkillSyncDialog } from '@/components/RemoteSkillSyncDialog';

const BASE = 'contextManagement.skills.remoteSync';

function renderDialog() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  return { container, root };
}

async function flushEffects() {
  // 打开 → 拉设置 → 设置生效定工作区 → 加载列表，需多轮微任务冲刷。
  for (let i = 0; i < 4; i++) {
    await act(async () => { await Promise.resolve(); });
  }
}

// 通过所在 <label> 的文本定位 Checkbox（skill 名是原始数据；mock t 返回 key 文本），
// 不给生产组件加纯测试用属性。
function itemCheckbox(container: HTMLElement, name: string): HTMLInputElement {
  const label = [...container.querySelectorAll('label')].find(
    (l) => l.textContent?.includes(name),
  );
  const input = label?.querySelector<HTMLInputElement>('input');
  if (!input) throw new Error(`checkbox for ${name} not found`);
  return input;
}

function selectAllCheckbox(container: HTMLElement): HTMLInputElement {
  const label = [...container.querySelectorAll('label')].find(
    (l) => l.textContent?.includes(`${BASE}.selectAll`),
  );
  const input = label?.querySelector<HTMLInputElement>('input');
  if (!input) throw new Error('select-all checkbox not found');
  return input;
}

function syncButton(container: HTMLElement): HTMLButtonElement {
  const button = [...container.querySelectorAll('button')].find(
    (b) =>
      b.textContent === `${BASE}.confirm` ||
      b.textContent === `${BASE}.confirmWithOverwrite`,
  );
  if (!button) throw new Error('sync button not found');
  return button;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getMulticaSettings.mockResolvedValue({
    enabled: true,
    toggleLocked: false,
    multicaBaseUrl: 'https://example.test',
    multicaAppUrl: null,
    patSet: true,
    daemonIdSet: true,
    workspaces: [{ id: 'ws-1', name: 'Alpha', slug: 'alpha' }],
    activeWorkspaceId: 'ws-1',
    defaultProvider: 'claude-acp',
    connected: true,
    connectedAccount: null,
    addressOverrideSet: false,
  });
  mocks.listRemoteSkills.mockResolvedValue([
    { id: 's-new', name: 'New One', description: '', localState: 'new' },
    { id: 's-exists', name: 'Old One', description: '', localState: 'exists' },
  ]);
  mocks.pullRemoteSkills.mockResolvedValue({ results: [] });
});

afterEach(() => {
  document.body.innerHTML = '';
});

describe('remote skill sync dialog', () => {
  it('renders the skill list as the sole scroll region (flex chain contract)', async () => {
    const { container, root } = renderDialog();
    await act(async () => {
      root.render(
        <RemoteSkillSyncDialog open onOpenChange={() => {}} onFinished={() => {}} />,
      );
    });
    await flushEffects();

    // 修复点契约（用户实测反馈后二次整改）：滚动必须直接挂在 flex item 上
    // （overflow-y-auto + min-h-0 + flex-1），不得经 Radix ScrollArea 的 size-full
    // 视口百分比链——在 max-h 封顶的自适应高度弹窗内该百分比不可靠，视口塌到
    // 内容高度导致裁掉且不滚动。
    const scroller = container.querySelector<HTMLElement>('[class*="overflow-y-auto"]');
    expect(scroller).toBeTruthy();
    expect(scroller?.className).toContain('min-h-0');
    expect(scroller?.className).toContain('flex-1');
    expect(scroller?.className).toContain('gold-themed-scrollbar');

    // 列表加载后应渲染两个 skill 行。
    expect(container.querySelectorAll('label input')).toHaveLength(3); // 2 项 + 1 全选
  });

  it('tri-state select-all: default partial → all → none, then single item still selectable', async () => {
    const { container, root } = renderDialog();
    await act(async () => {
      root.render(
        <RemoteSkillSyncDialog open onOpenChange={() => {}} onFinished={() => {}} />,
      );
    });
    await flushEffects();

    // 默认「新增勾选、已存在不勾」的部分态：全选框 indeterminate。
    expect(itemCheckbox(container, 'New One').dataset.checked).toBe('true');
    expect(itemCheckbox(container, 'Old One').dataset.checked).toBe('false');
    expect(selectAllCheckbox(container).dataset.checked).toBe('indeterminate');

    // 第一次点击：全部勾选。
    await act(async () => { selectAllCheckbox(container).click(); });
    expect(itemCheckbox(container, 'New One').dataset.checked).toBe('true');
    expect(itemCheckbox(container, 'Old One').dataset.checked).toBe('true');
    expect(selectAllCheckbox(container).dataset.checked).toBe('true');

    // 第二次点击：全部取消（用户只需连点两次即可清空，再单独勾选想要的）。
    await act(async () => { selectAllCheckbox(container).click(); });
    expect(itemCheckbox(container, 'New One').dataset.checked).toBe('false');
    expect(itemCheckbox(container, 'Old One').dataset.checked).toBe('false');
    expect(selectAllCheckbox(container).dataset.checked).toBe('false');
    expect(syncButton(container).disabled).toBe(true);

    // 全部取消后仍可单独勾选一项并同步：只拉取勾中的那一个。
    await act(async () => { itemCheckbox(container, 'Old One').click(); });
    expect(itemCheckbox(container, 'Old One').dataset.checked).toBe('true');
    expect(selectAllCheckbox(container).dataset.checked).toBe('indeterminate');

    const button = syncButton(container);
    expect(button.disabled).toBe(false);
    await act(async () => { button.click(); });
    await flushEffects();

    expect(mocks.pullRemoteSkills).toHaveBeenCalledTimes(1);
    expect(mocks.pullRemoteSkills).toHaveBeenCalledWith('ws-1', ['s-exists']);
  });

  it('not connected → source selector + inline connect entry; connected via dialog refetches settings', async () => {
    // 首次设置拉取返回未连接态；连接成功后的重拉返回已连接（beforeEach 默认值）。
    mocks.getMulticaSettings.mockResolvedValueOnce({
      enabled: true,
      toggleLocked: false,
      multicaBaseUrl: 'https://example.test',
      multicaAppUrl: null,
      patSet: false,
      daemonIdSet: false,
      workspaces: [],
      activeWorkspaceId: null,
      defaultProvider: 'claude-acp',
      connected: false,
      connectedAccount: null,
      addressOverrideSet: false,
    });
    const { container, root } = renderDialog();
    await act(async () => {
      root.render(
        <RemoteSkillSyncDialog open onOpenChange={() => {}} onFinished={() => {}} />,
      );
    });
    await flushEffects();

    // 来源选择器（注册表驱动）+ 未连接文案 + 就地连接按钮。
    expect(container.textContent).toContain('remote.taskManagement.source.label');
    expect(container.textContent).toContain(`${BASE}.notConnected`);
    const connectBtn = [...container.querySelectorAll('button')].find(
      (b) => b.textContent === `${BASE}.connect`,
    );
    expect(connectBtn).toBeTruthy();

    // 打开连接弹窗：sourceLabel 经注册表名片解析（t 桩返回 key），不散写品牌名。
    await act(async () => { connectBtn!.click(); });
    const connectDialog = container.querySelector<HTMLElement>('[data-testid="connect-dialog"]');
    expect(connectDialog).toBeTruthy();
    expect(connectDialog?.getAttribute('data-source')).toBe('remote.taskManagement.source.multica');

    // 连接成功 → 重拉设置 → 未连接分支消失（工作空间/列表随之级联刷新）。
    await act(async () => {
      container.querySelector<HTMLButtonElement>('[data-testid="connect-done"]')!.click();
    });
    await flushEffects();
    expect(mocks.getMulticaSettings).toHaveBeenCalledTimes(2);
    expect(container.textContent).not.toContain(`${BASE}.notConnected`);
    expect(container.querySelector('[data-testid="connect-dialog"]')).toBeTruthy(); // onOpenChange 由连接弹窗自身收尾
  });
});
