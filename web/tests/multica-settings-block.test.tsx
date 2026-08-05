/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 跨渲染稳定（组件 useEffect 依赖 [refresh] → [t]），返回 key 本身便于断言。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({
  displayAppError: () => 'mock-error',
}));

vi.mock('lucide-react', () => ({
  Check: () => null,
  ExternalLink: () => null,
  FolderInput: () => null,
  Loader2: () => null,
  Trash2: () => null,
}));

vi.mock('@/lib/utils', () => ({
  cn: (...args: unknown[]) => args.filter(Boolean).join(' '),
}));

// 轻量桩：Button 透传（便于断言点击目标），其余表单原语渲染为中性元素。
vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));
vi.mock('@/components/ui/input', () => ({
  Input: (props: Record<string, unknown>) => <input {...(props as object)} />,
}));
vi.mock('@/components/ui/switch', () => ({
  Switch: (props: Record<string, unknown>) => <button role="switch" {...(props as object)} />,
}));
vi.mock('@/components/ui/select', () => ({
  Select: ({ children }: { children?: ReactNode }) => <>{children}</>,
  SelectContent: ({ children }: { children?: ReactNode }) => <>{children}</>,
  SelectItem: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  SelectTrigger: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  SelectValue: () => <span />,
}));
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children?: ReactNode }) => <>{children}</>,
  TooltipContent: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));

const mocks = vi.hoisted(() => ({
  getMulticaSettings: vi.fn(),
  openExternalUrl: vi.fn(),
  subscribeMulticaSettingsUpdates: vi.fn(),
}));

vi.mock('@/api', () => ({
  connectMultica: vi.fn(),
  disconnectMultica: vi.fn(),
  getMulticaSettings: mocks.getMulticaSettings,
  openExternalUrl: mocks.openExternalUrl,
  pickLocalDirectory: vi.fn(),
  rebindMulticaWorkspace: vi.fn(),
  removeMulticaWorkspace: vi.fn(),
  saveMulticaSettings: vi.fn(),
  setActiveMulticaWorkspace: vi.fn(),
  subscribeMulticaSettingsUpdates: mocks.subscribeMulticaSettingsUpdates,
}));

const noopUnlisten = () => {};
const { getMulticaSettings, openExternalUrl } = mocks;

import { MulticaSettingsBlock } from '@/components/settings/MulticaSettingsBlock';

beforeEach(() => {
  vi.clearAllMocks();
  subscribeStable();
});

afterEach(() => {
  document.body.innerHTML = '';
});

// 订阅桩需在每次用例前重置（vi.clearAllMocks 清掉了 resolved 值）。
function subscribeStable() {
  mocks.subscribeMulticaSettingsUpdates.mockResolvedValue(noopUnlisten);
}

async function renderBlock() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<MulticaSettingsBlock />);
  });
  // flush getMulticaSettings + 订阅 promise
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  return container;
}

describe('multica settings block — 连接账号可见性 (M5-l)', () => {
  it('连接后展示已连接账号邮箱 + 切换账号按钮', async () => {
    getMulticaSettings.mockResolvedValue({
      enabled: true,
      toggleLocked: false,
      multicaBaseUrl: 'http://maling.weoa.com',
      multicaAppUrl: 'http://maling.weoa.com',
      patSet: true,
      daemonIdSet: true,
      workspaces: [],
      activeWorkspaceId: null,
      defaultProvider: 'claude-acp',
      connected: true,
      connectedAccount: { name: '张三', email: 'zhangsan@maling.local' },
    });

    const container = await renderBlock();

    // 已连接账号身份可见（核对浏览器 cookie 是否静默连到非预期账号）。
    expect(container.textContent).toContain('zhangsan@maling.local');
    expect(container.textContent).toContain('settings.multica.connectedAccount');
    // 切换账号逃生口的 tooltip 文案存在（按钮为图标，靠 tooltip 说明）。
    expect(container.textContent).toContain('settings.multica.switchAccountHint');
  });

  it('未连接时不展示已连接账号行', async () => {
    getMulticaSettings.mockResolvedValue({
      enabled: true,
      toggleLocked: false,
      multicaBaseUrl: 'http://maling.weoa.com',
      multicaAppUrl: 'http://maling.weoa.com',
      patSet: false,
      daemonIdSet: false,
      workspaces: [],
      activeWorkspaceId: null,
      defaultProvider: 'claude-acp',
      connected: false,
      connectedAccount: null,
    });

    const container = await renderBlock();

    expect(container.textContent).not.toContain('settings.multica.connectedAccount');
  });

  it('点击切换账号按钮打开 multica Web（appUrl）', async () => {
    getMulticaSettings.mockResolvedValue({
      enabled: true,
      toggleLocked: false,
      multicaBaseUrl: 'http://maling.weoa.com',
      multicaAppUrl: 'http://maling.weoa.com',
      patSet: true,
      daemonIdSet: true,
      workspaces: [],
      activeWorkspaceId: null,
      defaultProvider: 'claude-acp',
      connected: true,
      connectedAccount: { name: null, email: 'a@b.com' },
    });

    const container = await renderBlock();
    // hint span 的父节点即账号行（Tooltip 为 fragment，hint 与 trigger span 同属账号行），
    // 账号行内唯一的 button 即「切换账号」按钮。
    const hintSpan = Array.from(container.querySelectorAll('span')).find((s) =>
      s.textContent?.includes('switchAccountHint'),
    );
    expect(hintSpan).toBeTruthy();
    const btn = hintSpan!.parentElement!.querySelector('button') as HTMLButtonElement;
    expect(btn).toBeTruthy();
    expect(btn.disabled).toBe(false);

    await act(async () => {
      btn.click();
    });

    expect(openExternalUrl).toHaveBeenCalledTimes(1);
    expect(openExternalUrl).toHaveBeenCalledWith('http://maling.weoa.com');
  });
});
