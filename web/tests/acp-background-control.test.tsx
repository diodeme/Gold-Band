/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { useDirectBackgroundActive } from '@/lib/acp-background-control';
import type { DirectBackgroundControl } from '@/types';

it('keeps Stop for tools, renews the grace period and disposes timers across sessions', async () => {
  vi.useFakeTimers(); vi.setSystemTime(1000);
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement('div'); document.body.append(host);
  const root = createRoot(host);
  function Control({control, scope}: {control: DirectBackgroundControl | null; scope: string}) {
    return useDirectBackgroundActive(control, scope) ? <button>停止</button> : null;
  }
  const value = {sessionId:'a',connectionGeneration:1,activeTools:1,expiresAtMs:11000};
  const render = async (control: DirectBackgroundControl | null, scope = 'a') => act(async () => root.render(<Control control={control} scope={scope}/>));
  try {
    await render(value);
    await act(async () => vi.advanceTimersByTime(20000));
    expect(host.textContent).toBe('停止');
    await render({...value,activeTools:0,expiresAtMs:Date.now()+10000});
    await act(async () => vi.advanceTimersByTime(9000));
    expect(host.textContent).toBe('停止');
    await render({...value,activeTools:0,expiresAtMs:Date.now()+10000});
    await act(async () => vi.advanceTimersByTime(9999));
    expect(host.textContent).toBe('停止');
    await act(async () => vi.advanceTimersByTime(1));
    expect(host.textContent).toBe('');
    await render({...value,activeTools:0,expiresAtMs:Date.now()+10000});
    await render(null,'b');
    expect(host.textContent).toBe('');
    expect(vi.getTimerCount()).toBe(0);
  } finally { await act(async () => root.unmount()); host.remove(); vi.useRealTimers(); }
});
