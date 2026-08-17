import { useCallback, useEffect, useState } from 'react';
import { getScheduledRuntimeSettings } from '@/api';
import type { ScheduledRuntimeSettingsVm } from '@/types';

/**
 * 定时任务运行时设置的客户端缓存层。
 *
 * 为什么需要它：`<ScheduledRuntimeSettings />` 过去在每次挂载时都把 `settings` 置为 `null`
 * 再发起一次 Tauri 拉取，期间渲染“加载中…”。由于 Radix `<TabsContent>` 会卸载非激活
 * 标签页、`SettingsPage` 也会因 `key` 变化被重挂载，用户每次进入“通用 → 定时任务”都会
 * 看到一次加载闪烁。这里用模块级缓存实现 stale-while-revalidate：有缓存就立即渲染，
 * 同时在后台静默刷新动态字段（保持唤醒是否生效、启用任务数、电源错误码）。
 *
 * 领域归属：定时任务设置混合了「用户配置」（开关、保留天数）与「运行时状态」
 * （生效情况、任务数、电源错误），因此它不应像 preferences / updater / metrics 那样
 * 进入启动时的静态快照 `AppBootstrapVm`，而适合用独立的运行时缓存。
 */

/** 缓存新鲜期：在该窗口内重复挂载不会触发后台刷新，避免在标签页间快速切换时重复请求。 */
export const STALE_MS = 5_000;

interface CacheState {
  value: ScheduledRuntimeSettingsVm | null;
  fetchedAt: number;
  inflight: Promise<ScheduledRuntimeSettingsVm> | null;
  generation: number;
}

const state: CacheState = {
  value: null,
  fetchedAt: 0,
  inflight: null,
  generation: 0,
};

const listeners = new Set<(value: ScheduledRuntimeSettingsVm) => void>();

/**
 * 读取当前缓存的设置值（可能为 null）。纯函数，供组件初始化与单元测试使用。
 */
export function readScheduledRuntimeSettingsCache(): ScheduledRuntimeSettingsVm | null {
  return state.value;
}

/**
 * 缓存是否已过期或尚未填充。`now` 仅用于注入测试时钟，默认取当前时间。
 */
export function isScheduledRuntimeSettingsStale(now: number = Date.now()): boolean {
  return state.value === null || now - state.fetchedAt >= STALE_MS;
}

/**
 * 用已知值（通常是 `save` 的返回值）写入缓存并刷新时间戳。
 */
export function writeScheduledRuntimeSettingsCache(
  value: ScheduledRuntimeSettingsVm,
  now: number = Date.now(),
): void {
  state.generation += 1;
  state.value = value;
  state.fetchedAt = now;
  listeners.forEach((listener) => listener(value));
}

/**
 * 拉取一次设置；若已有飞行中的请求则复用同一个 Promise，避免并发挂载重复请求。
 * 成功后写入缓存。失败时向上抛出，由调用方决定是否静默。
 */
export function fetchScheduledRuntimeSettingsOnce(): Promise<ScheduledRuntimeSettingsVm> {
  if (state.inflight) return state.inflight;
  const fetchGeneration = state.generation;
  state.inflight = getScheduledRuntimeSettings()
    .then((value) => {
      if (state.generation !== fetchGeneration && state.value) return state.value;
      state.value = value;
      state.fetchedAt = Date.now();
      listeners.forEach((listener) => listener(value));
      return value;
    })
    .finally(() => {
      state.inflight = null;
    });
  return state.inflight;
}

/**
 * App 启动后静默预取，填充缓存，让首次进入设置页也能秒开。
 * 失败静默（留给组件挂载时重试）。可安全重复调用。
 */
export async function prefetchScheduledRuntimeSettings(): Promise<void> {
  try {
    await fetchScheduledRuntimeSettingsOnce();
  } catch {
    // 静默失败：预取只是体验优化，不应阻塞启动流程。
  }
}

/**
 * 重置模块级缓存状态。仅用于单元测试在用例间隔离状态。
 */
export function __resetScheduledRuntimeSettingsCache(): void {
  state.value = null;
  state.fetchedAt = 0;
  state.inflight = null;
  state.generation = 0;
  listeners.clear();
}

export interface UseScheduledRuntimeSettingsResult {
  /** 当前设置值；为 null 表示尚未拿到任何数据（此时组件可显示加载态）。 */
  settings: ScheduledRuntimeSettingsVm | null;
  /** 拉取失败标志（仅在拿不到任何数据时有意义）。 */
  loadError: boolean;
  /** 用已知值（如保存返回值）直接更新缓存与组件状态，避免多余请求。 */
  replace: (value: ScheduledRuntimeSettingsVm) => void;
}

/**
 * stale-while-revalidate 风格的定时任务运行时设置 hook。
 *
 * - 首次（缓存为空）：返回 null，组件进入加载态；拉取完成后回填。
 * - 后续挂载（缓存命中）：立即以缓存值初始化，无加载闪烁；若缓存已过期则在后台刷新。
 * - 调用方保存成功后应调用 `replace` 同步缓存，使下一次挂载拿到最新值。
 */
export function useScheduledRuntimeSettings(): UseScheduledRuntimeSettingsResult {
  const [settings, setSettings] = useState<ScheduledRuntimeSettingsVm | null>(
    readScheduledRuntimeSettingsCache,
  );
  const [loadError, setLoadError] = useState(false);

  useEffect(() => {
    let active = true;
    const listener = (value: ScheduledRuntimeSettingsVm) => {
      if (!active) return;
      setSettings(value);
      setLoadError(false);
    };
    listeners.add(listener);
    if (isScheduledRuntimeSettingsStale()) {
      fetchScheduledRuntimeSettingsOnce()
        .then((value) => {
          if (!active) return;
          setSettings(value);
          setLoadError(false);
        })
        .catch(() => {
          if (!active) return;
          setLoadError(true);
        });
    }
    return () => {
      active = false;
      listeners.delete(listener);
    };
  }, []);

  const replace = useCallback((value: ScheduledRuntimeSettingsVm) => {
    writeScheduledRuntimeSettingsCache(value);
    setSettings(value);
    setLoadError(false);
  }, []);

  return { settings, loadError, replace };
}
