/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useEventDrivenRefresh } from '@/lib/use-event-driven-refresh';

/**
 * 控制刷新 promise 的 resolve/reject 时机，用于精确模拟 in-flight + pending 合并。
 * `release()` 解除当前刷新；多次 release 依次放行排队的 pending。
 */
function controlledRefresh() {
  let resolveFn: (() => void) | null = null;
  const fn = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        resolveFn = resolve;
      }),
  );
  return {
    fn,
    release() {
      const r = resolveFn;
      resolveFn = null;
      r?.();
    },
    get pending() {
      return resolveFn !== null;
    },
  };
}

/** 构造可记录 listener + 可控行为的 fake subscribe。 */
function fakeSubscribe(opts: { deferDispose?: boolean } = {}) {
  const listeners: Array<() => void> = [];
  const disposes = vi.fn();
  const subscribe = vi.fn((listener: () => void) => {
    listeners.push(listener);
    if (opts.deferDispose) {
      // 返回一个 pending promise，由 test 手动 release（模拟 Tauri listen 异步）。
      return new Promise<() => void>((resolve) => {
        pendingResolvers.push(() => resolve(disposes));
      });
    }
    return Promise.resolve(disposes);
  });
  const pendingResolvers: Array<() => void> = [];
  return {
    subscribe,
    disposes,
    fire: () => listeners.forEach((l) => l()),
    listenerCount: () => listeners.length,
    resolvePending: () => pendingResolvers.splice(0).forEach((r) => r()),
  };
}

afterEach(() => {
  document.body.innerHTML = '';
});

function Harness(props: {
  refresh: () => void | Promise<void>;
  subscribeFns: Array<((l: () => void) => Promise<() => void>) | undefined>;
  refreshOnMount?: boolean;
}) {
  useEventDrivenRefresh(props.refresh, props.subscribeFns, {
    refreshOnMount: props.refreshOnMount,
  });
  return <div />;
}

async function renderHarness(props: Parameters<typeof Harness>[0]) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Harness {...props} />);
  });
  return { container, root };
}

describe('useEventDrivenRefresh', () => {
  it('refreshOnMount triggers an initial refresh', async () => {
    const refresh = vi.fn(async () => {});
    await renderHarness({ refresh, subscribeFns: [], refreshOnMount: true });
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('does not refresh on mount when refreshOnMount is false', async () => {
    const refresh = vi.fn(async () => {});
    await renderHarness({ refresh, subscribeFns: [], refreshOnMount: false });
    expect(refresh).not.toHaveBeenCalled();
  });

  it('coalesces concurrent events into 1 in-flight + 1 pending (no fetch storm)', async () => {
    const { fn: refresh, release } = controlledRefresh();
    const channel = fakeSubscribe();
    await renderHarness({
      refresh,
      subscribeFns: [channel.subscribe],
      refreshOnMount: false,
    });

    // 第 1 个事件：启动 in-flight 刷新（挂起未 release）。
    act(() => channel.fire());
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(refresh).toHaveLastReturnedWith(expect.any(Promise));

    // 风暴：连发 5 个事件，全部落在 in-flight 期间 → 只标记 pending，不新增刷新。
    act(() => {
      channel.fire();
      channel.fire();
      channel.fire();
      channel.fire();
      channel.fire();
    });
    expect(refresh).toHaveBeenCalledTimes(1);

    // release 当前刷新：pending 被消费，触发 1 次拖尾重跑（共 2 次，不是 6 次）。
    await act(async () => {
      release();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(2);

    // 拖尾重跑也 release → 无新增 pending，刷新流回到空闲。
    await act(async () => {
      release();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('coalesces across multiple subscribe channels into a single refresh stream', async () => {
    const { fn: refresh, release } = controlledRefresh();
    const task = fakeSubscribe();
    const settings = fakeSubscribe();
    await renderHarness({
      refresh,
      subscribeFns: [task.subscribe, settings.subscribe],
      refreshOnMount: false,
    });

    // 两通道几乎同时 fire → 合并为 1 次刷新。
    act(() => {
      task.fire();
      settings.fire();
    });
    expect(refresh).toHaveBeenCalledTimes(1);

    await act(async () => {
      release();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(2); // 1 次拖尾

    await act(async () => {
      release();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('registers exactly one listener per channel', async () => {
    const task = fakeSubscribe();
    const settings = fakeSubscribe();
    await renderHarness({
      refresh: async () => {},
      subscribeFns: [task.subscribe, settings.subscribe],
      refreshOnMount: false,
    });
    expect(task.subscribe).toHaveBeenCalledTimes(1);
    expect(settings.subscribe).toHaveBeenCalledTimes(1);
    expect(task.listenerCount()).toBe(1);
    expect(settings.listenerCount()).toBe(1);
  });

  it('disposes all listeners on unmount', async () => {
    const task = fakeSubscribe();
    const settings = fakeSubscribe();
    const { root } = await renderHarness({
      refresh: async () => {},
      subscribeFns: [task.subscribe, settings.subscribe],
      refreshOnMount: false,
    });
    expect(task.disposes).not.toHaveBeenCalled();
    expect(settings.disposes).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });

    expect(task.disposes).toHaveBeenCalledTimes(1);
    expect(settings.disposes).toHaveBeenCalledTimes(1);
  });

  it('does not leak a listener when unmount beats the async subscribe resolve', async () => {
    // subscribe 返回 Promise，cleanup 在 resolve 之前触发：dispose 仍必须被调用一次。
    const channel = fakeSubscribe({ deferDispose: true });
    const { root } = await renderHarness({
      refresh: async () => {},
      subscribeFns: [channel.subscribe],
      refreshOnMount: false,
    });

    // 此时 subscribe 的 Promise 尚未 resolve（deferDispose），dispose 还没注册。
    expect(channel.disposes).not.toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });

    // unmount 后才 resolve：active 已 false → 应立即 dispose（不泄漏）。
    await act(async () => {
      channel.resolvePending();
      await Promise.resolve();
    });
    expect(channel.disposes).toHaveBeenCalledTimes(1);
  });

  it('stops firing refresh after unmount (no setState on dead listener)', async () => {
    const refresh = vi.fn(async () => {});
    const channel = fakeSubscribe();
    const { root } = await renderHarness({
      refresh,
      subscribeFns: [channel.subscribe],
      refreshOnMount: false,
    });

    await act(async () => {
      root.unmount();
    });

    // 卸载后再 fire：listener 内 active 守卫拦截，refresh 不应被调用。
    act(() => channel.fire());
    expect(refresh).not.toHaveBeenCalled();
  });

  it('ignores undefined entries in subscribeFns (browser client omits them)', async () => {
    const refresh = vi.fn(async () => {});
    const channel = fakeSubscribe();
    await renderHarness({
      refresh,
      subscribeFns: [undefined, channel.subscribe, undefined],
      refreshOnMount: false,
    });
    // 只注册了定义的那个通道；undefined 被过滤。
    expect(channel.subscribe).toHaveBeenCalledTimes(1);
  });

  it('treats a refresh rejection as best-effort and keeps consuming pending', async () => {
    let call = 0;
    const refresh = vi.fn(async () => {
      call += 1;
      if (call === 1) throw new Error('boom');
    });
    const channel = fakeSubscribe();
    await renderHarness({
      refresh,
      subscribeFns: [channel.subscribe],
      refreshOnMount: false,
    });

    // 第 1 次：reject（被 hook 吞掉，不应抛出）。
    await act(async () => {
      channel.fire();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(1);

    // 后续事件仍正常刷新（失败不阻断流）。
    await act(async () => {
      channel.fire();
      await Promise.resolve();
    });
    expect(refresh).toHaveBeenCalledTimes(2);
  });
});
