import { beforeEach, describe, expect, it, vi } from 'vitest';

// 用 mock 替换 @/api，避免在 node 测试环境触发真实 Tauri 调用。
vi.mock('@/api', () => ({
  getScheduledRuntimeSettings: vi.fn(),
}));

import { getScheduledRuntimeSettings } from '@/api';
import type { ScheduledRuntimeSettingsVm } from '@/types';
import {
  STALE_MS,
  __resetScheduledRuntimeSettingsCache,
  fetchScheduledRuntimeSettingsOnce,
  isScheduledRuntimeSettingsStale,
  prefetchScheduledRuntimeSettings,
  readScheduledRuntimeSettingsCache,
  writeScheduledRuntimeSettingsCache,
} from '@/components/scheduled-tasks/useScheduledRuntimeSettings';

const mockedGet = vi.mocked(getScheduledRuntimeSettings);

function makeSettings(overrides: Partial<ScheduledRuntimeSettingsVm> = {}): ScheduledRuntimeSettingsVm {
  return {
    keepAwakeEnabled: false,
    keepAwakeEffective: false,
    completionNotificationsEnabled: true,
    enabledJobCount: 0,
    occurrenceRetentionDays: 30,
    powerErrorCode: null,
    ...overrides,
  };
}

describe('scheduled runtime settings cache', () => {
  beforeEach(() => {
    __resetScheduledRuntimeSettingsCache();
    mockedGet.mockReset();
  });

  describe('read / write / staleness', () => {
    it('初始缓存为空且视为过期', () => {
      expect(readScheduledRuntimeSettingsCache()).toBeNull();
      expect(isScheduledRuntimeSettingsStale(0)).toBe(true);
    });

    it('写入后可读取，并在新鲜期内不视为过期', () => {
      const settings = makeSettings({ occurrenceRetentionDays: 14 });
      writeScheduledRuntimeSettingsCache(settings, 1_000);
      expect(readScheduledRuntimeSettingsCache()).toEqual(settings);
      expect(isScheduledRuntimeSettingsStale(1_000)).toBe(false);
      expect(isScheduledRuntimeSettingsStale(1_000 + STALE_MS - 1)).toBe(false);
    });

    it('超过新鲜期视为过期', () => {
      writeScheduledRuntimeSettingsCache(makeSettings(), 1_000);
      expect(isScheduledRuntimeSettingsStale(1_000 + STALE_MS)).toBe(true);
    });

    it('reset 清空缓存', () => {
      writeScheduledRuntimeSettingsCache(makeSettings(), 1_000);
      __resetScheduledRuntimeSettingsCache();
      expect(readScheduledRuntimeSettingsCache()).toBeNull();
      expect(isScheduledRuntimeSettingsStale(1_000)).toBe(true);
    });
  });

  describe('fetchScheduledRuntimeSettingsOnce', () => {
    it('成功后写入缓存', async () => {
      const settings = makeSettings({ enabledJobCount: 3 });
      mockedGet.mockResolvedValue(settings);

      await expect(fetchScheduledRuntimeSettingsOnce()).resolves.toEqual(settings);
      expect(readScheduledRuntimeSettingsCache()).toEqual(settings);
      expect(mockedGet).toHaveBeenCalledTimes(1);
    });

    it('飞行中的并发调用复用同一请求（去重）', async () => {
      const settings = makeSettings();
      let resolve!: (value: ScheduledRuntimeSettingsVm) => void;
      mockedGet.mockReturnValue(new Promise<ScheduledRuntimeSettingsVm>((r) => {
        resolve = r;
      }));

      const first = fetchScheduledRuntimeSettingsOnce();
      const second = fetchScheduledRuntimeSettingsOnce();

      // 同一 Promise 引用，且底层只发起一次请求。
      expect(first).toBe(second);
      expect(mockedGet).toHaveBeenCalledTimes(1);

      resolve(settings);
      await expect(first).resolves.toEqual(settings);
      expect(readScheduledRuntimeSettingsCache()).toEqual(settings);
    });

    it('does not let a late refresh overwrite a newer saved value', async () => {
      const stale = makeSettings({ occurrenceRetentionDays: 30 });
      const saved = makeSettings({ occurrenceRetentionDays: 90 });
      let resolve!: (value: ScheduledRuntimeSettingsVm) => void;
      mockedGet.mockReturnValue(new Promise<ScheduledRuntimeSettingsVm>((next) => {
        resolve = next;
      }));

      const refresh = fetchScheduledRuntimeSettingsOnce();
      writeScheduledRuntimeSettingsCache(saved, 2_000);
      resolve(stale);

      await expect(refresh).resolves.toEqual(saved);
      expect(readScheduledRuntimeSettingsCache()).toEqual(saved);
    });

    it('失败后清空飞行标记，允许后续重试', async () => {
      mockedGet.mockRejectedValueOnce(new Error('boom'));
      await expect(fetchScheduledRuntimeSettingsOnce()).rejects.toThrow('boom');
      expect(readScheduledRuntimeSettingsCache()).toBeNull();

      const settings = makeSettings();
      mockedGet.mockResolvedValue(settings);
      await expect(fetchScheduledRuntimeSettingsOnce()).resolves.toEqual(settings);
      expect(mockedGet).toHaveBeenCalledTimes(2);
    });
  });

  describe('prefetchScheduledRuntimeSettings', () => {
    it('成功填充缓存', async () => {
      const settings = makeSettings();
      mockedGet.mockResolvedValue(settings);

      await prefetchScheduledRuntimeSettings();

      expect(readScheduledRuntimeSettingsCache()).toEqual(settings);
    });

    it('失败时静默，不抛出且不写入缓存', async () => {
      mockedGet.mockRejectedValue(new Error('boom'));

      await expect(prefetchScheduledRuntimeSettings()).resolves.toBeUndefined();
      expect(readScheduledRuntimeSettingsCache()).toBeNull();
    });
  });
});
