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

// Dialog 桩：按 open 门控渲染（jsdom 无 portal，直接内联）。
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children, open }: { children?: ReactNode; open?: boolean }) =>
    open ? <>{children}</> : null,
  DialogContent: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children?: ReactNode }) => <h2>{children}</h2>,
  DialogFooter: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));

vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

vi.mock('@/components/ui/input', () => ({
  Input: (props: Record<string, unknown>) => <input {...(props as object)} />,
}));

const mocks = vi.hoisted(() => ({
  saveMulticaConnectionAddress: vi.fn(),
}));

vi.mock('@/api', () => ({
  saveMulticaConnectionAddress: mocks.saveMulticaConnectionAddress,
}));

import { MulticaConnectionSettingsDialog } from '@/components/conversation/MulticaConnectionSettingsDialog';
import type { MulticaSettingsVm } from '@/types';

function baseSettings(overrides: Record<string, unknown> = {}): MulticaSettingsVm {
  return {
    enabled: true,
    toggleLocked: false,
    multicaBaseUrl: 'http://localhost:8080',
    multicaAppUrl: 'http://localhost:3000',
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

async function renderDialog(settingsVm: MulticaSettingsVm | null = baseSettings()) {
  const onOpenChange = vi.fn();
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <MulticaConnectionSettingsDialog
        open
        onOpenChange={onOpenChange}
        settingsVm={settingsVm}
      />,
    );
  });
  await act(async () => { await Promise.resolve(); });
  return { container, onOpenChange };
}

describe('MulticaConnectionSettingsDialog (连接地址设置弹窗)', () => {
  it('prefills the input with the effective base URL on open', async () => {
    const { container } = await renderDialog(baseSettings());

    const input = container.querySelector('input') as HTMLInputElement;
    expect(input.value).toBe('http://localhost:8080');
    expect(container.textContent).toContain('multica.connection.title');
    expect(container.textContent).toContain('multica.connection.addressLabel');
    // 无覆盖（渠道默认）→ 不显示「恢复默认地址」。
    expect(findButton(container, 'multica.connection.restoreDefault')).toBeUndefined();
  });

  it('saves the address with API = login URL (v, v) and closes', async () => {
    mocks.saveMulticaConnectionAddress.mockResolvedValue(
      baseSettings({ multicaBaseUrl: 'https://new.example', multicaAppUrl: 'https://new.example', addressOverrideSet: true }),
    );
    const { container, onOpenChange } = await renderDialog();

    setNativeInputValue(container.querySelector('input') as HTMLInputElement, 'https://new.example');
    await act(async () => { await Promise.resolve(); });

    const saveBtn = findButton(container, 'multica.connection.save') as HTMLButtonElement;
    expect(saveBtn.disabled).toBe(false);
    await act(async () => { saveBtn.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.saveMulticaConnectionAddress).toHaveBeenCalledWith('https://new.example', 'https://new.example');
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('disables save when the address is unchanged (防分端口默认被同址覆盖)', async () => {
    const { container } = await renderDialog(baseSettings());

    const saveBtn = findButton(container, 'multica.connection.save') as HTMLButtonElement;
    expect(saveBtn.disabled).toBe(true);
    expect(mocks.saveMulticaConnectionAddress).not.toHaveBeenCalled();
  });

  it('disables save and shows invalidUrl for a non-http(s) value', async () => {
    const { container } = await renderDialog(baseSettings());

    setNativeInputValue(container.querySelector('input') as HTMLInputElement, 'not-a-url');
    await act(async () => { await Promise.resolve(); });

    expect(container.textContent).toContain('multica.connection.invalidUrl');
    expect(findButton(container, 'multica.connection.save')?.disabled).toBe(true);
  });

  it('restores the channel default via (null, null) and refreshes the field in place', async () => {
    mocks.saveMulticaConnectionAddress.mockResolvedValue(
      baseSettings({ multicaBaseUrl: 'http://default.example', multicaAppUrl: 'http://default.example', addressOverrideSet: false }),
    );
    const { container, onOpenChange } = await renderDialog(
      baseSettings({ multicaBaseUrl: 'https://override.example', multicaAppUrl: 'https://override.example', addressOverrideSet: true }),
    );

    const restoreBtn = findButton(container, 'multica.connection.restoreDefault');
    expect(restoreBtn).toBeTruthy();
    await act(async () => { restoreBtn!.click(); });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });

    expect(mocks.saveMulticaConnectionAddress).toHaveBeenCalledWith(null, null);
    // 用返回 VM 刷新字段（回落渠道默认生效值），弹窗保持打开可继续编辑。
    expect((container.querySelector('input') as HTMLInputElement).value).toBe('http://default.example');
    expect(findButton(container, 'multica.connection.restoreDefault')).toBeUndefined();
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
