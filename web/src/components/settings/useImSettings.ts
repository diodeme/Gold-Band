import { useCallback, useEffect, useState } from 'react';
import { getImSettings, subscribeImChannelStateUpdates } from '@/api';
import { mergeImChannelSnapshot, mergeImSettings } from '@/lib/im-settings';
import type { ImChannelSnapshotVm, ImSettingsVm } from '@/types';

/**
 * IM 设置的客户端缓存层。
 *
 * 为什么需要它：`<ImIntegrationSettings />` 过去在每次挂载时都把 `settings` 置为 `null`
 * 再发起一次 Tauri 拉取，期间渲染“加载中…”。由于 Radix `<TabsContent>` 会卸载非激活
 * 标签页、`SettingsPage` 也会因 `key` 变化或离开路由被重挂载，用户每次进入设置都会
 * 看到一次加载闪烁。这里用模块级缓存实现 stale-while-revalidate：有缓存就立即渲染，
 * 同时在后台静默刷新，并在挂载期间把 connection snapshot 合入同一份投影。
 *
 * 领域归属：IM 设置混合了「用户配置」（开关、通知、凭据、binding）与「运行时连接态」
 * （generation、state、lastError），因此它不应像 preferences 那样进入启动静态快照
 * `AppBootstrapVm`，而适合用独立的运行时缓存。该缓存只是展示投影，不是 canonical 事实源。
 */

/** 缓存新鲜期：在该窗口内重复挂载不会触发后台刷新，避免在标签页间快速切换时重复请求。 */
export const STALE_MS = 5_000;

interface CacheState {
  value: ImSettingsVm | null;
  fetchedAt: number;
  inflight: Promise<ImSettingsVm> | null;
  generation: number;
}

const state: CacheState = {
  value: null,
  fetchedAt: 0,
  inflight: null,
  generation: 0,
};

const listeners = new Set<(value: ImSettingsVm) => void>();

export function readImSettingsCache(): ImSettingsVm | null {
  return state.value;
}

export function isImSettingsStale(now: number = Date.now()): boolean {
  return state.value === null || now - state.fetchedAt >= STALE_MS;
}

export function writeImSettingsCache(
  value: ImSettingsVm,
  now: number = Date.now(),
): void {
  state.generation += 1;
  state.value = value;
  state.fetchedAt = now;
  listeners.forEach((listener) => listener(value));
}

export function mergeImSettingsCache(
  incoming: ImSettingsVm,
  now: number = Date.now(),
): ImSettingsVm {
  const next = state.value ? mergeImSettings(state.value, incoming) : incoming;
  writeImSettingsCache(next, now);
  return next;
}

export function applyImChannelSnapshotCache(snapshot: ImChannelSnapshotVm): ImSettingsVm | null {
  if (!state.value) return null;
  const next = mergeImChannelSnapshot(state.value, snapshot);
  state.value = next;
  listeners.forEach((listener) => listener(next));
  return next;
}

export function fetchImSettingsOnce(): Promise<ImSettingsVm> {
  if (state.inflight) return state.inflight;
  const fetchGeneration = state.generation;
  state.inflight = getImSettings()
    .then((value) => {
      if (state.generation !== fetchGeneration && state.value) return state.value;
      const next = state.value ? mergeImSettings(state.value, value) : value;
      state.value = next;
      state.fetchedAt = Date.now();
      listeners.forEach((listener) => listener(next));
      return next;
    })
    .finally(() => {
      state.inflight = null;
    });
  return state.inflight;
}

export async function prefetchImSettings(): Promise<void> {
  try {
    await fetchImSettingsOnce();
  } catch {
    // 静默失败：预取只是体验优化，不应阻塞启动流程。
  }
}

export function __resetImSettingsCache(): void {
  state.value = null;
  state.fetchedAt = 0;
  state.inflight = null;
  state.generation = 0;
  listeners.clear();
}

export interface UseImSettingsResult {
  settings: ImSettingsVm | null;
  loadError: unknown;
  replace: (incoming: ImSettingsVm) => void;
}

export function useImSettings(): UseImSettingsResult {
  const [settings, setSettings] = useState<ImSettingsVm | null>(readImSettingsCache);
  const [loadError, setLoadError] = useState<unknown>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const listener = (value: ImSettingsVm) => {
      if (!active) return;
      setSettings(value);
      setLoadError(null);
    };
    listeners.add(listener);
    if (isImSettingsStale()) {
      fetchImSettingsOnce().catch((error) => {
        if (!active) return;
        if (readImSettingsCache()) return;
        setLoadError(error);
      });
    }
    void subscribeImChannelStateUpdates((snapshot) => {
      applyImChannelSnapshotCache(snapshot);
    }).then((cleanup) => {
      if (active) unlisten = cleanup;
      else cleanup();
    });
    return () => {
      active = false;
      listeners.delete(listener);
      unlisten?.();
    };
  }, []);

  const replace = useCallback((incoming: ImSettingsVm) => {
    const next = mergeImSettingsCache(incoming);
    setSettings(next);
    setLoadError(null);
  }, []);

  return { settings, loadError, replace };
}
