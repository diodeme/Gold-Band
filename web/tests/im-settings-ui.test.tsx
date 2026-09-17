/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImChannelSnapshotVm, ImSettingsVm } from '@/types';

let currentSettings: ImSettingsVm;
let channelListener: ((snapshot: ImChannelSnapshotVm) => void) | null = null;
let enabledResult: Promise<ImSettingsVm> | null = null;

const api = vi.hoisted(() => ({
  cancelWeComScanAuthorization: vi.fn(async () => {}),
  completeWeComScanAuthorization: vi.fn(),
  deleteImChannel: vi.fn(),
  getImSettings: vi.fn(),
  reconnectImChannel: vi.fn(),
  resetImChannelBinding: vi.fn(),
  saveImNotificationPreferences: vi.fn(),
  setImChannelEnabled: vi.fn(),
  startWeComScanAuthorization: vi.fn(),
  subscribeImChannelStateUpdates: vi.fn(),
}));

vi.mock('@/api', () => api);

import { ImIntegrationSettings } from '@/components/settings/ImIntegrationSettings';
import { __resetImSettingsCache, readImSettingsCache, writeImSettingsCache } from '@/components/settings/useImSettings';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const notifications = {
  permission: true,
  elicitation: true,
  manualCheck: true,
  runSuccess: false,
  runFailure: true,
  acpTurnFinished: false,
};

function settingsFixture(configured = true): ImSettingsVm {
  return {
    channels: [{
      kind: 'weCom',
      enabled: configured,
      publicIdentity: configured ? 'bot-id' : '',
      credentialConfigured: configured,
      binding: null,
      notifications: { ...notifications },
      connection: configured ? {
        kind: 'weCom',
        enabled: true,
        generation: 2,
        state: 'connected',
        capabilities: { proactiveDelivery: true, cardActions: true, messageUpdate: true, privateChat: true },
        identity: { botId: 'bot-id', displayName: 'Bot' },
        binding: null,
        lastConnectedAtMs: 1,
        lastErrorCode: null,
      } : null,
    }],
  };
}

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe('IM settings production interaction', () => {
  let host: HTMLDivElement;
  let root: Root;
  const pendingSettings = new Set<(value: ImSettingsVm) => void>();

  function mockPendingImSettings() {
    const pending = createDeferred<ImSettingsVm>();
    pendingSettings.add(pending.resolve);
    api.getImSettings.mockReturnValue(pending.promise);
    return pending;
  }

  beforeEach(() => {
    pendingSettings.clear();
    __resetImSettingsCache();
    currentSettings = settingsFixture();
    channelListener = null;
    enabledResult = null;
    api.getImSettings.mockImplementation(async () => structuredClone(currentSettings));
    api.subscribeImChannelStateUpdates.mockImplementation(async (listener) => {
      channelListener = listener;
      return () => { channelListener = null; };
    });
    api.setImChannelEnabled.mockImplementation(() => enabledResult ?? Promise.resolve(structuredClone(currentSettings)));
    api.saveImNotificationPreferences.mockImplementation(async (input) => {
      currentSettings.channels[0].notifications = structuredClone(input.notifications);
      return structuredClone(currentSettings);
    });
    api.resetImChannelBinding.mockImplementation(async () => structuredClone(currentSettings));
    api.deleteImChannel.mockImplementation(async () => ({
      settings: settingsFixture(false),
      operationId: 'cleanup-1',
      cleanupStatus: 'complete',
    }));
    api.reconnectImChannel.mockImplementation(async () => structuredClone(currentSettings.channels[0].connection!));
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    root?.unmount();
    for (const resolve of pendingSettings) {
      resolve(structuredClone(currentSettings));
    }
    pendingSettings.clear();
    __resetImSettingsCache();
    document.body.replaceChildren();
    vi.clearAllMocks();
  });

  async function renderSettings() {
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ImIntegrationSettings />
        </TooltipProvider>,
      );
    });
  }

  it('shows loading only before the first IM settings value is available', async () => {
    mockPendingImSettings();
    await renderSettings();
    expect(host.textContent).toContain('加载中');
    expect(host.querySelector('[data-im-status]')).toBeNull();
  });

  it('renders cached IM settings on the first paint without a loading flash', async () => {
    writeImSettingsCache(settingsFixture(false));
    mockPendingImSettings();
    await renderSettings();
    expect(host.textContent).not.toContain('加载中');
    expect(host.querySelector('[data-im-status="notConfigured"]')).not.toBeNull();
    expect(host.textContent).toContain('扫码接入');
  });

  it('keeps previously loaded IM settings visible when the settings page remounts', async () => {
    await renderSettings();
    expect(host.querySelector('[data-im-status]')).not.toBeNull();
    expect(host.textContent).not.toContain('加载中');

    await act(async () => root.unmount());

    mockPendingImSettings();
    root = createRoot(host);
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ImIntegrationSettings />
        </TooltipProvider>,
      );
    });

    expect(host.textContent).not.toContain('加载中');
    expect(host.querySelector('[data-im-status="waitingBinding"]')).not.toBeNull();
  });

  it('progressively discloses only the connect action before credentials exist', async () => {
    currentSettings = settingsFixture(false);
    await renderSettings();

    expect(host.querySelector('[data-im-status="notConfigured"]')).not.toBeNull();
    expect(host.querySelector('[role="switch"]')).toBeNull();
    expect(host.querySelector('fieldset')).toBeNull();
    expect(host.textContent).toContain('扫码接入');
    expect(host.textContent).not.toContain('保存通知设置');
  });

  it('keeps a dirty notification draft across connection snapshots and can discard it', async () => {
    await renderSettings();
    const checkboxes = host.querySelectorAll<HTMLButtonElement>('[role="checkbox"]');
    expect(checkboxes).toHaveLength(6);

    await act(async () => checkboxes[0].click());
    expect(checkboxes[0].getAttribute('data-state')).toBe('unchecked');
    expect(host.querySelector('[data-im-notification-actions]')).not.toBeNull();

    await act(async () => {
      channelListener?.({
        ...currentSettings.channels[0].connection!,
        generation: 3,
        state: 'connecting',
      });
    });
    expect(host.querySelectorAll<HTMLButtonElement>('[role="checkbox"]')[0].getAttribute('data-state')).toBe('unchecked');

    const discard = [...host.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.includes('放弃修改'));
    await act(async () => discard?.click());
    expect(host.querySelectorAll<HTMLButtonElement>('[role="checkbox"]')[0].getAttribute('data-state')).toBe('checked');
  });

  it('shows target-level pending state immediately and converges to the command response', async () => {
    let resolveEnabled!: (settings: ImSettingsVm) => void;
    enabledResult = new Promise((resolve) => { resolveEnabled = resolve; });
    await renderSettings();
    const toggle = host.querySelector<HTMLButtonElement>('[role="switch"]')!;

    await act(async () => toggle.click());
    expect(toggle.getAttribute('aria-busy')).toBe('true');
    expect(toggle.disabled).toBe(true);

    const paused = settingsFixture();
    paused.channels[0].enabled = false;
    paused.channels[0].connection = {
      ...paused.channels[0].connection!,
      enabled: false,
      generation: 3,
      state: 'disabled',
    };
    await act(async () => resolveEnabled(paused));
    expect(host.querySelector('[data-im-status="paused"]')).not.toBeNull();
    expect(api.setImChannelEnabled).toHaveBeenCalledTimes(1);
  });

  it('shows reconnect progress only while the canonical lifecycle is reconnecting', async () => {
    currentSettings.channels[0].connection = {
      ...currentSettings.channels[0].connection!,
      state: 'reconnecting',
      lastErrorCode: 'IM_NETWORK_UNAVAILABLE',
    };
    await renderSettings();
    const reconnecting = host.querySelector('[data-im-status="reconnecting"]')!;
    expect(reconnecting.querySelector('.animate-spin')).not.toBeNull();

    await act(async () => {
      channelListener?.({
        ...currentSettings.channels[0].connection!,
        generation: 2,
        state: 'error',
        lastErrorCode: 'IM_NETWORK_UNAVAILABLE',
      });
    });
    const failed = host.querySelector('[data-im-status="connectionFailed"]')!;
    expect(failed.querySelector('.animate-spin')).toBeNull();
    expect(host.textContent).toContain('重新连接');
  });

  it('keeps change-recipient and delete as separately confirmed menu actions', async () => {
    currentSettings.channels[0].binding = {
      destinationId: 'user-1',
      conversationId: 'chat-1',
      authorizedActorId: 'user-1',
      displayName: 'User',
    };
    currentSettings.channels[0].connection!.binding = {
      destinationId: 'user-1',
      conversationId: 'chat-1',
      actorId: 'user-1',
      isPrivate: true,
    };
    await renderSettings();
    const menu = host.querySelector<HTMLButtonElement>('[aria-label="配置管理"]')!;

    act(() => {
      menu.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0, buttons: 1 }));
    });
    const changeRecipient = [...document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')]
      .find((item) => item.textContent?.includes('更换接收账号'));
    expect(changeRecipient).toBeTruthy();
    act(() => {
      changeRecipient?.focus();
      changeRecipient?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
    expect(document.body.textContent).toContain('更换接收账号？');
    expect(document.body.textContent).not.toContain('测试连接');
    const cancel = [...document.body.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '取消');
    act(() => cancel?.click());
  });

  it('shows durable pending cleanup after the configuration is already deleted', async () => {
    api.deleteImChannel.mockResolvedValue({
      settings: settingsFixture(false),
      operationId: 'cleanup-pending-1',
      cleanupStatus: 'pending',
    });
    writeImSettingsCache(currentSettings);
    act(() => {
      root.render(
        <TooltipProvider>
          <ImIntegrationSettings />
        </TooltipProvider>,
      );
    });
    const menu = host.querySelector<HTMLButtonElement>('[aria-label="配置管理"]')!;

    act(() => {
      menu.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0, buttons: 1 }));
    });
    const deleteItem = [...document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')]
      .find((item) => item.textContent === '删除配置');
    expect(deleteItem).toBeTruthy();
    act(() => {
      deleteItem?.focus();
      deleteItem?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
    const dialog = document.body.querySelector<HTMLElement>('[role="alertdialog"]')!;
    const confirm = [...dialog.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent === '删除')!;
    confirm.click();
    await api.deleteImChannel.mock.results.at(-1)?.value;
    expect(api.deleteImChannel).toHaveBeenCalledWith('weCom');
    expect(readImSettingsCache()?.channels[0]?.credentialConfigured).toBe(false);
  });
});
