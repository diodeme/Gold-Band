import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { browserApi } from '@/api/browser';
import i18n from '@/i18n';
import {
  buildSaveImNotificationPreferencesInput,
  buildSetImChannelEnabledInput,
  imChannelDisplayModel,
  mergeImChannelSnapshot,
  mergeImSettings,
  notificationPreferencesEqual,
} from '@/lib/im-settings';
import type {
  ImChannelSettingsVm,
  ImConnectionState,
  ImNotificationPreferencesVm,
  ImSettingsVm,
} from '@/types';

const quietDefaults = (): ImNotificationPreferencesVm => ({
  permission: true,
  elicitation: true,
  manualCheck: true,
  runSuccess: false,
  runFailure: true,
  acpTurnFinished: false,
});

function channelFixture(
  patch: Partial<ImChannelSettingsVm> = {},
  state: ImConnectionState = 'connected',
): ImChannelSettingsVm {
  return {
    kind: 'weCom',
    enabled: true,
    publicIdentity: 'bot-id',
    credentialConfigured: true,
    binding: null,
    notifications: quietDefaults(),
    connection: {
      kind: 'weCom',
      enabled: true,
      generation: 3,
      state,
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
      lastConnectedAtMs: null,
      lastErrorCode: null,
    },
    ...patch,
  };
}

describe('IM settings boundary', () => {
  it('exposes every supported channel with explicit quiet defaults', async () => {
    await browserApi.deleteImChannel('weCom');
    const settings = await browserApi.getImSettings();
    expect(settings.channels.map((channel) => channel.kind)).toEqual(['weCom', 'feishu']);
    expect(settings.channels[0]).toMatchObject({ kind: 'weCom', notifications: quietDefaults() });
  });

  it('configures Feishu through its own scan session without exposing credentials', async () => {
    await browserApi.deleteImChannel('feishu');
    const sessionId = crypto.randomUUID();
    const authorization = await browserApi.startFeishuScanAuthorization(sessionId, false);
    expect(authorization).toMatchObject({ sessionId });
    expect(authorization.qrUrl).toBeTruthy();
    const settings = await browserApi.completeFeishuScanAuthorization(sessionId);
    const feishu = settings.channels.find((channel) => channel.kind === 'feishu');
    expect(feishu).toMatchObject({ credentialConfigured: true });
    expect(feishu?.publicIdentity).toBeTruthy();
    expect(JSON.stringify(settings)).not.toContain('secret');
  });

  it('builds narrow commands without identity, binding, or credentials', () => {
    expect(buildSetImChannelEnabledInput('weCom', true)).toEqual({ kind: 'weCom', enabled: true });
    expect(buildSaveImNotificationPreferencesInput('weCom', quietDefaults())).toEqual({
      kind: 'weCom',
      notifications: quietDefaults(),
    });
  });

  it('keeps the browser capability limits aligned with each connector', async () => {
    const feishuSessionId = crypto.randomUUID();
    await browserApi.startFeishuScanAuthorization(feishuSessionId, false);
    await browserApi.completeFeishuScanAuthorization(feishuSessionId);
    const sessionId = crypto.randomUUID();
    await browserApi.startWeComScanAuthorization(sessionId);
    const settings = await browserApi.completeWeComScanAuthorization(sessionId);
    const wecom = settings.channels.find((channel) => channel.kind === 'weCom');
    const feishu = settings.channels.find((channel) => channel.kind === 'feishu');
    expect(wecom?.connection?.capabilities).toMatchObject({
      messageUpdate: 'callbackOnly', interaction: { maxQuestions: 3 },
    });
    expect(feishu?.connection?.capabilities).toMatchObject({
      messageUpdate: 'anytime', interaction: { maxQuestions: 5 },
    });
  });

  it('derives the complete lifecycle and recovery matrix from canonical fields', () => {
    expect(imChannelDisplayModel(channelFixture({ credentialConfigured: false, connection: null })).status)
      .toBe('notConfigured');
    expect(imChannelDisplayModel(channelFixture()).status).toBe('waitingBinding');
    expect(imChannelDisplayModel(channelFixture({}, 'connecting')).status).toBe('connecting');
    expect(imChannelDisplayModel(channelFixture({ enabled: false }, 'disabled')).status).toBe('paused');
    expect(imChannelDisplayModel(channelFixture({
      connection: { ...channelFixture().connection!, lastErrorCode: 'IM_NETWORK_UNAVAILABLE', state: 'reconnecting' },
    })).status).toBe('reconnecting');
    expect(imChannelDisplayModel(channelFixture({
      connection: { ...channelFixture().connection!, lastErrorCode: 'IM_NETWORK_UNAVAILABLE', state: 'error' },
    }))).toMatchObject({ status: 'connectionFailed', recoveryAction: 'reconnect' });
    expect(imChannelDisplayModel(channelFixture({
      connection: { ...channelFixture().connection!, lastErrorCode: 'IM_AUTHENTICATION_REQUIRED', state: 'authenticationRequired' },
    }))).toMatchObject({ status: 'reauthorize', recoveryAction: 'reauthorize' });
    expect(imChannelDisplayModel(channelFixture({
      connection: { ...channelFixture().connection!, lastErrorCode: 'IM_CONNECTION_CONFLICT', state: 'error' },
    }))).toMatchObject({ status: 'conflict', recoveryAction: 'reconnect' });

    const binding = {
      destinationId: 'user-1', conversationId: 'chat-1', authorizedActorId: 'user-1', displayName: 'User',
    };
    expect(imChannelDisplayModel(channelFixture({
      binding,
      connection: {
        ...channelFixture().connection!,
        binding: { destinationId: 'user-1', conversationId: 'chat-1', actorId: 'user-1', isPrivate: true },
      },
    })).status).toBe('ready');
  });

  it('never treats an observed-only or mismatched binding as ready', () => {
    const observed = { destinationId: 'user-1', conversationId: 'chat-1', actorId: 'user-1', isPrivate: true };
    expect(imChannelDisplayModel(channelFixture({
      connection: { ...channelFixture().connection!, binding: observed, lastErrorCode: 'IM_STORAGE_UNAVAILABLE' },
    })).status).toBe('waitingBinding');
    expect(imChannelDisplayModel(channelFixture({
      binding: {
        destinationId: 'user-2', conversationId: 'chat-2', authorizedActorId: 'user-2', displayName: 'Other',
      },
      connection: { ...channelFixture().connection!, binding: observed },
    })).status).toBe('waitingBinding');
  });

  it('routes missing Feishu scopes to reauthorization rather than reconnect', () => {
    const channel = channelFixture({ kind: 'feishu' });
    channel.connection = { ...channel.connection!, kind: 'feishu', state: 'error', lastErrorCode: 'IM_SCOPE_MISSING' };
    expect(imChannelDisplayModel(channel)).toMatchObject({ status: 'reauthorize', recoveryAction: 'reauthorize' });
  });

  it('keeps notification editing available for every configured connection state', () => {
    for (const channel of [
      channelFixture(),
      channelFixture({}, 'connecting'),
      channelFixture({ enabled: false }, 'disabled'),
      channelFixture({ connection: { ...channelFixture().connection!, state: 'reconnecting', lastErrorCode: 'IM_NETWORK_UNAVAILABLE' } }),
      channelFixture({ connection: { ...channelFixture().connection!, state: 'error', lastErrorCode: 'IM_NETWORK_UNAVAILABLE' } }),
      channelFixture({ connection: { ...channelFixture().connection!, state: 'authenticationRequired', lastErrorCode: 'IM_AUTHENTICATION_REQUIRED' } }),
      channelFixture({ connection: { ...channelFixture().connection!, state: 'error', lastErrorCode: 'IM_CONNECTION_CONFLICT' } }),
    ]) {
      expect(imChannelDisplayModel(channel).showNotifications).toBe(true);
    }
  });

  it('merges snapshots and command responses monotonically by generation', () => {
    const base: ImSettingsVm = { channels: [channelFixture()] };
    const stale = { ...base.channels[0].connection!, generation: 2, state: 'error' as const };
    expect(mergeImChannelSnapshot(base, stale).channels[0]).toBe(base.channels[0]);
    const newer = { ...base.channels[0].connection!, generation: 4, state: 'connecting' as const };
    expect(mergeImChannelSnapshot(base, newer).channels[0].connection?.generation).toBe(4);

    const persistedBinding = {
      ...newer,
      state: 'connected' as const,
      binding: { destinationId: 'user-1', conversationId: 'chat-1', actorId: 'user-1', isPrivate: true },
    };
    const bound = mergeImChannelSnapshot(base, persistedBinding);
    expect(bound.channels[0].binding).toMatchObject({ authorizedActorId: 'user-1' });
    expect(imChannelDisplayModel(bound.channels[0]).status).toBe('ready');

    const current = mergeImChannelSnapshot(base, newer);
    const response = {
      channels: [{ ...base.channels[0], notifications: { ...quietDefaults(), runSuccess: true } }],
    };
    const merged = mergeImSettings(current, response);
    expect(merged.channels[0].connection?.generation).toBe(4);
    expect(merged.channels[0].notifications.runSuccess).toBe(true);
  });

  it('keeps narrow browser commands from overwriting independently owned fields', async () => {
    await browserApi.deleteImChannel('weCom');
    const sessionId = crypto.randomUUID();
    await browserApi.startWeComScanAuthorization(sessionId);
    const connected = await browserApi.completeWeComScanAuthorization(sessionId);
    const original = connected.channels[0];
    const notifications = { ...quietDefaults(), runSuccess: true, permission: false };
    const saved = await browserApi.saveImNotificationPreferences({ kind: 'weCom', notifications });
    const paused = await browserApi.setImChannelEnabled({ kind: 'weCom', enabled: false });
    expect(saved.channels[0].notifications).toEqual(notifications);
    expect(paused.channels[0]).toMatchObject({
      enabled: false,
      publicIdentity: original.publicIdentity,
      credentialConfigured: true,
      notifications,
    });
    expect(notificationPreferencesEqual(paused.channels[0].notifications, notifications)).toBe(true);
  });

  it('unsubscribes channel listeners without page-wide refresh', async () => {
    let events = 0;
    const cleanup = await browserApi.subscribeImChannelStateUpdates?.(() => { events += 1; });
    await browserApi.setImChannelEnabled({ kind: 'weCom', enabled: true });
    expect(events).toBe(1);
    cleanup?.();
    await browserApi.setImChannelEnabled({ kind: 'weCom', enabled: false });
    expect(events).toBe(1);
  });

  it('keeps Feishu notification and enable writes isolated from WeCom', async () => {
    const before = await browserApi.getImSettings();
    const wecom = structuredClone(before.channels.find((c) => c.kind === 'weCom'));
    const notifications = { ...quietDefaults(), runSuccess: true, elicitation: false };
    await browserApi.saveImNotificationPreferences({ kind: 'feishu', notifications });
    const paused = await browserApi.setImChannelEnabled({ kind: 'feishu', enabled: false });
    expect(paused.channels.find((c) => c.kind === 'weCom')).toEqual(wecom);
    expect(paused.channels.find((c) => c.kind === 'feishu')).toMatchObject({ enabled: false, notifications });
  });

  it('includes permission consent and rebinding copy in all seven catalogs', () => {
    for (const language of ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es']) {
      const catalog = JSON.parse(readFileSync(new URL(`../src/locales/${language}.json`, import.meta.url), 'utf8'));
      const feishu = catalog.settings.im.channels.feishu;
      for (const key of ['title', 'send', 'receive', 'update', 'confirm']) expect(feishu.permissions[key]).toBeTruthy();
      expect(feishu.singleDeviceHint).toBeTruthy();
      expect(feishu.resetBinding.description).toBeTruthy();
    }
  });

  it('has Chinese and English copy for statuses, actions, and stable errors', () => {
    for (const language of ['zh-CN', 'en']) {
      for (const status of ['notConfigured', 'waitingBinding', 'connecting', 'reconnecting', 'connectionFailed', 'ready', 'paused', 'reauthorize', 'conflict']) {
        expect(i18n.t(`settings.im.status.${status}`, { lng: language })).not.toContain('settings.im');
      }
      for (const kind of ['weCom', 'feishu']) {
        expect(i18n.t(`settings.im.channels.${kind}.title`, { lng: language })).not.toContain('settings.im');
        expect(i18n.t(`settings.im.channels.${kind}.identity`, { lng: language, id: 'x' })).not.toContain('settings.im');
      }
      for (const code of ['IM_CONNECTION_CONFLICT', 'IM_STORAGE_UNAVAILABLE', 'IM_STALE_GENERATION', 'IM_SCAN_DENIED', 'IM_SCOPE_MISSING']) {
        expect(i18n.t(`settings.im.errors.${code}`, { lng: language })).not.toContain('IM_');
      }
    }
  });

  it('keeps secrets and removed command paths out of the web boundary', async () => {
    const sessionId = crypto.randomUUID();
    await browserApi.startWeComScanAuthorization(sessionId);
    const settings = await browserApi.completeWeComScanAuthorization(sessionId);
    expect(JSON.stringify(settings)).not.toContain('secret');
    const sources = [
      '../src/components/settings/ImIntegrationSettings.tsx',
      '../src/api.ts',
      '../src/api/client.ts',
      '../src/api/desktop.ts',
    ].map((path) => readFileSync(new URL(path, import.meta.url), 'utf8')).join('\n');
    expect(sources).not.toContain('saveImChannelSettings');
    expect(sources).not.toContain('disconnectImChannel');
    expect(sources).not.toContain('save_im_channel_settings');
    expect(sources).not.toContain('disconnect_im_channel');
    expect(sources).not.toContain('source = "halo"');
    expect(sources).not.toContain('settings.im.channels.wecom.description');
    const i18n = readFileSync(new URL('../src/i18n.ts', import.meta.url), 'utf8');
    expect(i18n).not.toContain('安装级目标');
    expect(i18n).not.toContain('Installation-wide destination');
  });

  it('prefetches IM settings at app bootstrap', async () => {
    const app = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8');
    expect(app).toContain('prefetchImSettings');
    expect(app).toContain('prefetchScheduledRuntimeSettings');
  });
});
