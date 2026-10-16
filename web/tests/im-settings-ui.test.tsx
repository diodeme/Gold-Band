/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImChannelSnapshotVm, ImSettingsVm } from '@/types';

let currentSettings: ImSettingsVm;
let channelListener: ((snapshot: ImChannelSnapshotVm) => void) | null = null;
let enabledResult: Promise<ImSettingsVm> | null = null;

const api = vi.hoisted(() => ({
  cancelFeishuScanAuthorization: vi.fn(async () => {}),
  cancelWeComScanAuthorization: vi.fn(async () => {}),
  completeFeishuScanAuthorization: vi.fn(),
  completeWeComScanAuthorization: vi.fn(),
  deleteImChannel: vi.fn(),
  getImSettings: vi.fn(),
  reconnectImChannel: vi.fn(),
  resetImChannelBinding: vi.fn(),
  saveImNotificationPreferences: vi.fn(),
  setImChannelEnabled: vi.fn(),
  startFeishuScanAuthorization: vi.fn(),
  startWeComScanAuthorization: vi.fn(),
  subscribeImChannelStateUpdates: vi.fn(),
}));

vi.mock('@/api', () => api);
vi.mock('qrcode', () => ({ default: { toCanvas: vi.fn(async () => {}) } }));

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
        capabilities: {
          proactiveDelivery: true, cardActions: true, messageUpdate: 'callbackOnly', privateChat: true,
          interaction: {
            permissionMaxOptions: 20, maxQuestions: 3,
            singleChoiceMaxOptions: { singleQuestion: 20, multiQuestion: 10 },
            multiChoice: { maxOptions: 20, withinMultiQuestion: false },
            freeText: null, customAnswer: { kind: 'unsupported' }, decline: false,
          },
        },
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

  it('requires Feishu permission consent before starting a scan and cancels on close', async () => {
    currentSettings = settingsFixture(false);
    currentSettings.channels[0].kind = 'feishu';
    const pending = createDeferred<never>();
    api.startFeishuScanAuthorization.mockReturnValue(pending.promise);
    await renderSettings();
    const connect = [...host.querySelectorAll('button')].find((b) => b.textContent === '扫码接入')!;
    await act(async () => connect.click());
    expect(api.startFeishuScanAuthorization).not.toHaveBeenCalled();
    const dialog = document.body.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain('向你发送消息');
    expect(dialog.textContent).toContain('读取你发给机器人的私聊消息');
    expect(dialog.textContent).toContain('更新机器人发送的卡片');
    expect(host.textContent).toContain('一个机器人只连接一台设备');
    const consent = [...dialog.querySelectorAll('button')].find((b) => b.textContent === '同意并生成二维码')!;
    await act(async () => consent.click());
    expect(api.startFeishuScanAuthorization).toHaveBeenCalledWith(expect.any(String), false);
    expect(dialog.textContent).toContain('正在生成二维码');
    const session = api.startFeishuScanAuthorization.mock.calls[0][0];
    const close = [...dialog.querySelectorAll('button')].find((b) => b.textContent === '关闭')!;
    await act(async () => close.click());
    expect(api.cancelFeishuScanAuthorization).toHaveBeenCalledWith(session);
    expect(api.startWeComScanAuthorization).not.toHaveBeenCalled();
  });

  it('reauthorizes the configured Feishu app and completes only after its binding is ready', async () => {
    const channel = currentSettings.channels[0];
    channel.kind = 'feishu';
    channel.connection = { ...channel.connection!, kind: 'feishu', state: 'error', lastErrorCode: 'IM_SCOPE_MISSING' };
    const pending = createDeferred<ImSettingsVm>();
    pendingSettings.add(pending.resolve);
    api.startFeishuScanAuthorization.mockImplementation(async (sessionId) => ({ sessionId, qrUrl: 'https://example.test/qr', expiresAtMs: Date.now() + 60_000 }));
    api.completeFeishuScanAuthorization.mockReturnValue(pending.promise);
    await renderSettings();
    const reauthorize = [...host.querySelectorAll('button')].find((b) => b.textContent === '重新授权')!;
    await act(async () => reauthorize.click());
    expect(api.startFeishuScanAuthorization).not.toHaveBeenCalled();
    const dialog = document.body.querySelector('[role="dialog"]')!;
    const consent = [...dialog.querySelectorAll('button')].find((b) => b.textContent === '同意并生成二维码')!;
    await act(async () => consent.click());
    expect(api.startFeishuScanAuthorization).toHaveBeenCalledWith(expect.any(String), true);
    expect(dialog.querySelector('canvas[aria-label="飞书授权二维码"]')).not.toBeNull();
    const waiting = structuredClone(currentSettings);
    waiting.channels[0].connection = { ...channel.connection!, state: 'connected', lastErrorCode: null };
    await act(async () => pending.resolve(waiting));
    expect(dialog.textContent).toContain('授权完成，等待绑定接收账号');
    await act(async () => channelListener?.({
      ...waiting.channels[0].connection!,
      binding: { destinationId: 'owner', conversationId: 'chat', actorId: 'owner', isPrivate: true },
    }));
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('[data-im-status="ready"]')).not.toBeNull();
    expect(api.startWeComScanAuthorization).not.toHaveBeenCalled();
  });

  it('keeps WeCom scan automatic and does not call Feishu authorization', async () => {
    currentSettings = settingsFixture(false);
    api.startWeComScanAuthorization.mockReturnValue(new Promise(() => {}));
    await renderSettings();
    await act(async () => [...host.querySelectorAll('button')].find((b) => b.textContent === '扫码接入')!.click());
    expect(api.startWeComScanAuthorization).toHaveBeenCalledTimes(1);
    expect(api.startFeishuScanAuthorization).not.toHaveBeenCalled();
    expect(document.body.querySelector('[data-im-scan-consent]')).toBeNull();
  });

  it.each(['feishu', 'weCom'] as const)('waits for the new %s binding and ignores an old ready connection', async (kind) => {
    const channel = currentSettings.channels[0];
    channel.kind = kind;
    channel.binding = { destinationId: 'old', conversationId: 'old-chat', authorizedActorId: 'old', displayName: 'Old' };
    channel.connection = { ...channel.connection!, kind, binding: { destinationId: 'old', conversationId: 'old-chat', actorId: 'old', isPrivate: true } };
    const old = structuredClone(channel.connection);
    channel.connection = { ...old, state: 'authenticationRequired', lastErrorCode: 'IM_AUTHENTICATION_REQUIRED' };
    const pending = createDeferred<ImSettingsVm>();
    pendingSettings.add(pending.resolve);
    const start = kind === 'feishu' ? api.startFeishuScanAuthorization : api.startWeComScanAuthorization;
    const complete = kind === 'feishu' ? api.completeFeishuScanAuthorization : api.completeWeComScanAuthorization;
    start.mockImplementation(async (sessionId) => ({ sessionId, qrUrl: 'https://example.test/qr', expiresAtMs: Date.now() + 60_000 }));
    complete.mockReturnValue(pending.promise);
    await renderSettings();
    await act(async () => [...host.querySelectorAll('button')].find((button) => button.textContent === '重新授权')!.click());
    if (kind === 'feishu') {
      await act(async () => [...document.body.querySelectorAll('[role="dialog"] button')].find((button) => button.textContent === '同意并生成二维码')!.click());
    }
    const waiting = structuredClone(currentSettings);
    waiting.channels[0].binding = null;
    waiting.channels[0].connection = { ...old, generation: old.generation + 1, binding: null };
    await act(async () => pending.resolve(waiting));
    await act(async () => channelListener?.(old));
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain('授权完成，等待绑定接收账号');
    await act(async () => channelListener?.({ ...waiting.channels[0].connection!, binding: { destinationId: 'new', conversationId: 'new-chat', actorId: 'new', isPrivate: true } }));
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    expect(readImSettingsCache()?.channels[0].binding?.authorizedActorId).toBe('new');
  });

  it('ignores authorization completion after the dialog was closed', async () => {
    currentSettings = settingsFixture(false);
    currentSettings.channels[0].kind = 'feishu';
    const pending = createDeferred<ImSettingsVm>();
    pendingSettings.add(pending.resolve);
    api.startFeishuScanAuthorization.mockImplementation(async (sessionId) => ({ sessionId, qrUrl: 'https://example.test/qr', expiresAtMs: Date.now() + 60_000 }));
    api.completeFeishuScanAuthorization.mockReturnValue(pending.promise);
    await renderSettings();
    await act(async () => [...host.querySelectorAll('button')].find((button) => button.textContent === '扫码接入')!.click());
    await act(async () => [...document.body.querySelectorAll('[role="dialog"] button')].find((button) => button.textContent === '同意并生成二维码')!.click());
    await act(async () => [...document.body.querySelectorAll('[role="dialog"] button')].find((button) => button.textContent === '关闭')!.click());
    await act(async () => pending.resolve(settingsFixture(true)));
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    expect(readImSettingsCache()?.channels[0].credentialConfigured).toBe(false);
  });

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
    expect(host.textContent).not.toContain('安装级目标');
    expect(host.textContent).not.toContain('Installation-wide');
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

  it('explains Feishu owner-preserving rebind and confirms the target generation', async () => {
    const channel = currentSettings.channels[0];
    channel.kind = 'feishu';
    channel.binding = {
      destinationId: 'owner', conversationId: 'chat', authorizedActorId: 'owner', displayName: 'Owner',
    };
    channel.connection = {
      ...channel.connection!, kind: 'feishu',
      binding: { destinationId: 'owner', conversationId: 'chat', actorId: 'owner', isPrivate: true },
    };
    await renderSettings();
    const menu = host.querySelector<HTMLButtonElement>('[aria-label="配置管理"]')!;
    await act(async () => {
      menu.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0, buttons: 1 }));
    });
    const item = [...document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')]
      .find((element) => element.textContent === '重新绑定接收账号')!;
    expect(item).toBeTruthy();
    await act(async () => {
      item.focus();
      item.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
    const dialog = document.body.querySelector('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain('请使用扫码授权的账号');
    expect(dialog.textContent).toContain('如需更换授权账号，请重新授权');
    expect(api.resetImChannelBinding).not.toHaveBeenCalled();
    const confirm = [...dialog.querySelectorAll('button')].find((button) => button.textContent === '重新绑定')!;
    await act(async () => confirm.click());
    expect(api.resetImChannelBinding).toHaveBeenCalledWith({ kind: 'feishu', expectedGeneration: 2 });
    expect(api.startWeComScanAuthorization).not.toHaveBeenCalled();
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
