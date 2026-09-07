/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 返回 key 本身便于断言。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({ displayAppError: () => 'mock-error' }));

vi.mock('lucide-react', () => ({
  Loader2: () => null,
}));

// AlertDialog 桩：按 open 门控渲染；Action/Cancel 暴露为按钮（onClick 直调、不自动关窗——
// 组件已用 e.preventDefault 语义，真实关闭只走 onOpenChange 显式路径）。
vi.mock('@/components/ui/alert-dialog', () => ({
  AlertDialog: ({ children, open }: { children?: ReactNode; open?: boolean }) =>
    open ? <>{children}</> : null,
  AlertDialogContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AlertDialogHeader: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AlertDialogFooter: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  AlertDialogTitle: ({ children }: { children?: ReactNode }) => <h2>{children}</h2>,
  AlertDialogDescription: ({ children }: { children?: ReactNode }) => <p>{children}</p>,
  AlertDialogAction: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
  AlertDialogCancel: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

vi.mock('@/components/ui/input', () => ({
  Input: (props: Record<string, unknown>) => <input {...(props as object)} />,
}));

const mocks = vi.hoisted(() => ({
  connectMultica: vi.fn(),
  cancelMulticaConnect: vi.fn(),
  saveMulticaConnectionAddress: vi.fn(),
}));

vi.mock('@/api', () => ({
  connectMultica: mocks.connectMultica,
  cancelMulticaConnect: mocks.cancelMulticaConnect,
  saveMulticaConnectionAddress: mocks.saveMulticaConnectionAddress,
}));

import { MulticaConnectDialog } from '@/components/conversation/MulticaConnectDialog';
import type { MulticaSettingsVm } from '@/types';

function baseSettings(overrides: Record<string, unknown> = {}): MulticaSettingsVm {
  return {
    enabled: true,
    toggleLocked: false,
    multicaBaseUrl: 'https://m.example',
    multicaAppUrl: 'https://app.example',
    patSet: false,
    daemonIdSet: false,
    workspaces: [],
    activeWorkspaceId: null,
    defaultProvider: 'claude-acp',
    connected: false,
    connectedAccount: null,
    addressOverrideSet: false,
    ...overrides,
  } as MulticaSettingsVm;
}

function findButton(container: HTMLElement, text: string) {
  return Array.from(container.querySelectorAll('button')).find(
    (b) => b.textContent?.trim() === text,
  ) as HTMLButtonElement | undefined;
}

// React 受控 input：须走原型 value setter + dispatch input 事件（绕过 value-tracker 去重）。
function setNativeInputValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  setter.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  document.body.innerHTML = '';
});

async function renderDialog(
  overrides: { settingsVm?: MulticaSettingsVm | null; onConnected?: () => void } = {},
) {
  const onOpenChange = vi.fn();
  const onConnected = overrides.onConnected ?? vi.fn();
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <MulticaConnectDialog
        open
        onOpenChange={onOpenChange}
        settingsVm={overrides.settingsVm ?? baseSettings()}
        onConnected={onConnected}
      />,
    );
  });
  await act(async () => { await Promise.resolve(); });
  return { container, onOpenChange, onConnected };
}

describe('MulticaConnectDialog (连接确认弹窗：确认 + 可改地址)', () => {
  it('prefills the editable address input (无提示文案)', async () => {
    const { container } = await renderDialog();

    expect(container.textContent).toContain('multica.connect.title');
    expect(container.textContent).toContain('multica.connect.body');
    const input = container.querySelector('input') as HTMLInputElement;
    expect(input.value).toBe('https://m.example');
    // 调整轮：弹窗内直接改地址，不再渲染「如需修改…」指引。
    expect(container.textContent).not.toContain('multica.connect.changeHint');
  });

  it('connects without saving when the address is unchanged (防分端口默认被同址覆盖)', async () => {
    mocks.connectMultica.mockResolvedValue(baseSettings({ connected: true, patSet: true }));
    const { container, onOpenChange, onConnected } = await renderDialog();

    const confirmBtn = findButton(container, 'multica.connect.confirm');
    expect(confirmBtn).toBeTruthy();
    await act(async () => { confirmBtn!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.connectMultica).toHaveBeenCalledTimes(1);
    // 未改地址 → 不写设置（原样确认不会把 8080/3000 覆盖成 8080/8080）。
    expect(mocks.saveMulticaConnectionAddress).not.toHaveBeenCalled();
    expect(onConnected).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('saves the edited address (v, v) before connecting', async () => {
    mocks.saveMulticaConnectionAddress.mockResolvedValue(
      baseSettings({ multicaBaseUrl: 'https://new.example', multicaAppUrl: 'https://new.example', addressOverrideSet: true }),
    );
    mocks.connectMultica.mockResolvedValue(baseSettings({ connected: true, patSet: true }));
    const { container, onOpenChange } = await renderDialog();

    setNativeInputValue(container.querySelector('input') as HTMLInputElement, 'https://new.example');
    await act(async () => { await Promise.resolve(); });

    await act(async () => { findButton(container, 'multica.connect.confirm')!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.saveMulticaConnectionAddress).toHaveBeenCalledWith('https://new.example', 'https://new.example');
    // 先保存后连接。
    expect(mocks.saveMulticaConnectionAddress.mock.invocationCallOrder[0])
      .toBeLessThan(mocks.connectMultica.mock.invocationCallOrder[0]);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('disables connect and shows invalidUrl for a non-http(s) value', async () => {
    const { container } = await renderDialog();

    setNativeInputValue(container.querySelector('input') as HTMLInputElement, 'not-a-url');
    await act(async () => { await Promise.resolve(); });

    expect(container.textContent).toContain('multica.connection.invalidUrl');
    expect(findButton(container, 'multica.connect.confirm')?.disabled).toBe(true);
    expect(mocks.connectMultica).not.toHaveBeenCalled();
  });

  it('shows the mapped error and stays open when connect fails', async () => {
    mocks.connectMultica.mockRejectedValue({ code: 'multica.connect-failed', params: {} });
    const { container, onOpenChange } = await renderDialog();

    await act(async () => { findButton(container, 'multica.connect.confirm')!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(container.textContent).toContain('mock-error');
    expect(onOpenChange).not.toHaveBeenCalled();
    // connecting 态复位：主按钮恢复可点、取消恢复普通取消文案。
    expect(findButton(container, 'multica.connect.confirm')?.disabled).toBe(false);
    expect(findButton(container, 'common.cancel')).toBeTruthy();
  });

  it('closes silently (no error display) when the connect command reports cancellation', async () => {
    mocks.connectMultica.mockRejectedValue({ code: 'multica.connect-cancelled', params: {} });
    const { container, onOpenChange } = await renderDialog();

    await act(async () => { findButton(container, 'multica.connect.confirm')!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(container.textContent).not.toContain('mock-error');
  });

  it('while connecting: confirm disabled, cancel becomes 取消连接 and fires cancelMulticaConnect', async () => {
    // 永不 resolve 的连接 promise：稳定观察 connecting 中间态。
    mocks.connectMultica.mockImplementation(() => new Promise<MulticaSettingsVm>(() => {}));
    mocks.cancelMulticaConnect.mockResolvedValue(undefined);
    const { container, onOpenChange } = await renderDialog();

    await act(async () => { findButton(container, 'multica.connect.confirm')!.click(); });
    await act(async () => { await Promise.resolve(); });

    // 连接中：主按钮禁用转「连接中…」、地址输入禁用，取消转「取消连接」。
    expect(findButton(container, 'multica.connect.connecting')?.disabled).toBe(true);
    expect((container.querySelector('input') as HTMLInputElement).disabled).toBe(true);
    const cancelConnectBtn = findButton(container, 'multica.connect.cancelConnect');
    expect(cancelConnectBtn).toBeTruthy();

    // 点「取消连接」只发取消信号，不直接关窗（关窗走 cancelled 收尾路径）。
    await act(async () => { cancelConnectBtn!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.cancelMulticaConnect).toHaveBeenCalledTimes(1);
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
