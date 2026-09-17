import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImChannelSnapshotVm, ImSettingsVm } from '@/types';

vi.mock('@/api', () => ({
  getImSettings: vi.fn(),
  subscribeImChannelStateUpdates: vi.fn(async () => () => {}),
}));

import { getImSettings } from '@/api';
import {
  STALE_MS,
  __resetImSettingsCache,
  applyImChannelSnapshotCache,
  fetchImSettingsOnce,
  isImSettingsStale,
  mergeImSettingsCache,
  prefetchImSettings,
  readImSettingsCache,
  writeImSettingsCache,
} from '@/components/settings/useImSettings';

const mockedGet = vi.mocked(getImSettings);

const notifications = {
  permission: true,
  elicitation: true,
  manualCheck: true,
  runSuccess: false,
  runFailure: true,
  acpTurnFinished: false,
};

function makeSettings(generation = 2, enabled = true): ImSettingsVm {
  return {
    channels: [{
      kind: 'weCom',
      enabled,
      publicIdentity: 'bot-id',
      credentialConfigured: true,
      binding: null,
      notifications: { ...notifications },
      connection: {
        kind: 'weCom',
        enabled,
        generation,
        state: enabled ? 'connected' : 'disabled',
        capabilities: { proactiveDelivery: true, cardActions: true, messageUpdate: true, privateChat: true },
        identity: { botId: 'bot-id', displayName: 'Bot' },
        binding: null,
        lastConnectedAtMs: 1,
        lastErrorCode: null,
      },
    }],
  };
}

function snapshot(generation: number, state: ImChannelSnapshotVm['state'] = 'connecting'): ImChannelSnapshotVm {
  return {
    kind: 'weCom',
    enabled: true,
    generation,
    state,
    capabilities: { proactiveDelivery: true, cardActions: true, messageUpdate: true, privateChat: true },
    identity: { botId: 'bot-id', displayName: 'Bot' },
    binding: null,
    lastConnectedAtMs: 1,
    lastErrorCode: null,
  };
}

describe('IM settings cache', () => {
  beforeEach(() => {
    __resetImSettingsCache();
    mockedGet.mockReset();
  });

  describe('read / write / staleness', () => {
    it('初始缓存为空且视为过期', () => {
      expect(readImSettingsCache()).toBeNull();
      expect(isImSettingsStale(0)).toBe(true);
    });

    it('写入后可读取，并在新鲜期内不视为过期', () => {
      const settings = makeSettings();
      writeImSettingsCache(settings, 1_000);
      expect(readImSettingsCache()).toEqual(settings);
      expect(isImSettingsStale(1_000)).toBe(false);
      expect(isImSettingsStale(1_000 + STALE_MS - 1)).toBe(false);
    });

    it('超过新鲜期视为过期', () => {
      writeImSettingsCache(makeSettings(), 1_000);
      expect(isImSettingsStale(1_000 + STALE_MS)).toBe(true);
    });

    it('reset 清空缓存', () => {
      writeImSettingsCache(makeSettings(), 1_000);
      __resetImSettingsCache();
      expect(readImSettingsCache()).toBeNull();
      expect(isImSettingsStale(1_000)).toBe(true);
    });
  });

  describe('fetchImSettingsOnce', () => {
    it('成功后写入缓存', async () => {
      const settings = makeSettings();
      mockedGet.mockResolvedValue(settings);

      await expect(fetchImSettingsOnce()).resolves.toEqual(settings);
      expect(readImSettingsCache()).toEqual(settings);
      expect(mockedGet).toHaveBeenCalledTimes(1);
    });

    it('飞行中的并发调用复用同一请求（去重）', async () => {
      const settings = makeSettings();
      let resolve!: (value: ImSettingsVm) => void;
      mockedGet.mockReturnValue(new Promise<ImSettingsVm>((next) => {
        resolve = next;
      }));

      const first = fetchImSettingsOnce();
      const second = fetchImSettingsOnce();

      expect(first).toBe(second);
      expect(mockedGet).toHaveBeenCalledTimes(1);

      resolve(settings);
      await expect(first).resolves.toEqual(settings);
      expect(readImSettingsCache()).toEqual(settings);
    });

    it('does not let a late refresh overwrite a newer saved value', async () => {
      const stale = makeSettings(2, true);
      const saved = makeSettings(3, false);
      let resolve!: (value: ImSettingsVm) => void;
      mockedGet.mockReturnValue(new Promise<ImSettingsVm>((next) => {
        resolve = next;
      }));

      const refresh = fetchImSettingsOnce();
      mergeImSettingsCache(saved, 2_000);
      resolve(stale);

      await expect(refresh).resolves.toEqual(saved);
      expect(readImSettingsCache()).toEqual(saved);
    });

    it('keeps a newer live connection snapshot when a stale fetch returns', async () => {
      const fetched = makeSettings(2);
      let resolve!: (value: ImSettingsVm) => void;
      mockedGet.mockReturnValue(new Promise<ImSettingsVm>((next) => {
        resolve = next;
      }));

      writeImSettingsCache(makeSettings(2), 1_000);
      const refresh = fetchImSettingsOnce();
      applyImChannelSnapshotCache(snapshot(4));
      resolve(fetched);

      const cached = await refresh;
      expect(cached.channels[0].connection?.generation).toBe(4);
      expect(cached.channels[0].connection?.state).toBe('connecting');
      expect(readImSettingsCache()?.channels[0].connection?.generation).toBe(4);
    });

    it('失败后清空飞行标记，允许后续重试', async () => {
      mockedGet.mockRejectedValueOnce(new Error('boom'));
      await expect(fetchImSettingsOnce()).rejects.toThrow('boom');
      expect(readImSettingsCache()).toBeNull();

      const settings = makeSettings();
      mockedGet.mockResolvedValue(settings);
      await expect(fetchImSettingsOnce()).resolves.toEqual(settings);
      expect(mockedGet).toHaveBeenCalledTimes(2);
    });
  });

  describe('prefetchImSettings', () => {
    it('成功填充缓存', async () => {
      const settings = makeSettings();
      mockedGet.mockResolvedValue(settings);

      await prefetchImSettings();

      expect(readImSettingsCache()).toEqual(settings);
    });

    it('失败时静默，不抛出且不写入缓存', async () => {
      mockedGet.mockRejectedValue(new Error('boom'));

      await expect(prefetchImSettings()).resolves.toBeUndefined();
      expect(readImSettingsCache()).toBeNull();
    });
  });
});
