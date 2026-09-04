import { useEffect, useRef } from 'react';

/** 事件订阅契约：注册 listener，返回 dispose（取消监听）。可异步（Tauri `listen`）。 */
export type EventSubscribe = (listener: () => void) => Promise<() => void>;

export interface UseEventDrivenRefreshOptions {
  /** 挂载即触发一次 refresh。页面级主数据源置 true；纯事件同步（如 App 侧栏回流）置 false。 */
  refreshOnMount?: boolean;
}

/**
 * 事件驱动刷新：把多个事件订阅合并为单一去重刷新流。
 *
 * 提炼自 App.tsx 既有内联模式，统一解决三类竞态（原 `MulticaTaskManagementPage` 同时踩中三者：
 * 每事件双 fetch 风暴 + 异步 unlisten 泄漏 + 因 `t`/回调身份变化反复订阅）：
 *
 * 1. **事件风暴去重**——一次运行会连发 NodeProgress/NodeCompleted 等数十事件。用 in-flight + pending
 *    双 flag 合并：同一时间最多 1 个刷新在跑、最多 1 个待重跑（拖尾去重，不会无限堆积）。
 * 2. **异步 unlisten 竞态**——`subscribe` 返回 Promise，cleanup 可能在 resolve 前触发。用 `active` flag
 *    守卫；resolve 时若已 inactive 则立即 dispose，杜绝泄漏监听器（直接 cleanup 时只清已 resolve 的）。
 * 3. **refresh 身份抖动**——`refresh` 与 `subscribeFns` 经 ref 读取，effect 只订阅一次（`[]` deps），
 *    不因 `t` / 回调身份变化反复 unlisten/listen（frontend-performance §订阅边界：订阅一次，读最新值）。
 *
 * `refresh` 可返回 Promise（返回时才参与去重计时——等待其 settle 再消费 pending）；
 * 返回 void 时按 fire-and-forget 计（in-flight 立即释放，无法合并后续）。
 */
export function useEventDrivenRefresh(
  refresh: () => void | Promise<void>,
  subscribeFns: ReadonlyArray<EventSubscribe | undefined>,
  options: UseEventDrivenRefreshOptions = {},
): void {
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  const subscribeFnsRef = useRef(subscribeFns);
  subscribeFnsRef.current = subscribeFns;
  const refreshOnMountRef = useRef(options.refreshOnMount ?? false);
  refreshOnMountRef.current = options.refreshOnMount ?? false;

  useEffect(() => {
    // 过滤 undefined（browser client 不提供 / 调用方按运行态条件传入）。空数组 → 无订阅，仅可能 refreshOnMount。
    const subscribes = subscribeFnsRef.current.filter(
      (f): f is EventSubscribe => typeof f === 'function',
    );

    let active = true;
    let refreshInFlight = false;
    let refreshPending = false;

    const runRefresh = async () => {
      if (refreshInFlight) {
        refreshPending = true;
        return;
      }
      refreshInFlight = true;
      try {
        await refreshRef.current();
      } catch {
        // best-effort：事件驱动刷新失败不应阻断 UI；refresh 自身负责 setError/降级。
      } finally {
        refreshInFlight = false;
        if (active && refreshPending) {
          refreshPending = false;
          void runRefresh();
        }
      }
    };

    if (refreshOnMountRef.current) void runRefresh();

    // 串行 await 每个订阅：Tauri `listen` 注册是轻量操作，串行可接受且 dispose 顺序确定。
    const disposes: Array<() => void> = [];
    void (async () => {
      for (const subscribe of subscribes) {
        if (!active) break;
        try {
          const dispose = await subscribe(() => {
            if (active) void runRefresh();
          });
          if (active) {
            disposes.push(dispose);
          } else {
            // cleanup 已先于 resolve 触发：立即释放，避免泄漏。
            try {
              dispose();
            } catch {
              /* best-effort */
            }
          }
        } catch {
          // 单通道订阅失败不影响其他通道（best-effort）。
        }
      }
    })();

    return () => {
      active = false;
      for (const dispose of disposes) {
        try {
          dispose();
        } catch {
          /* best-effort */
        }
      }
    };
  }, []);
}
