/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  getPromptQueueSettings: vi.fn(),
  savePromptQueueSettings: vi.fn(),
}));

vi.mock('@/api', () => apiMocks);

import {
  resetPromptQueueSettingsForTests,
  savePromptQueueSettings,
  usePromptQueueSettings,
} from '@/lib/prompt-queue-settings-store';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function Probe() {
  const { confirmInterrupt } = usePromptQueueSettings();
  return <span data-confirm={String(confirmInterrupt)} />;
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('prompt queue settings store', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    resetPromptQueueSettingsForTests();
    apiMocks.getPromptQueueSettings.mockReset();
    apiMocks.savePromptQueueSettings.mockReset();
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const confirmValue = () => host.querySelector('[data-confirm]')?.getAttribute('data-confirm');

  it('asks before interrupting until the saved preference loads, and loads only once', async () => {
    const load = deferred<{ confirmInterrupt: boolean }>();
    apiMocks.getPromptQueueSettings.mockReturnValue(load.promise);
    await act(async () => root.render(<><Probe /><Probe /></>));
    expect(confirmValue()).toBe('true');

    await act(async () => load.resolve({ confirmInterrupt: false }));
    expect(confirmValue()).toBe('false');
    expect(apiMocks.getPromptQueueSettings).toHaveBeenCalledTimes(1);
  });

  it('keeps a write issued during the initial load', async () => {
    const load = deferred<{ confirmInterrupt: boolean }>();
    apiMocks.getPromptQueueSettings.mockReturnValue(load.promise);
    apiMocks.savePromptQueueSettings.mockImplementation(async (next) => next);
    await act(async () => root.render(<Probe />));

    await act(async () => { await savePromptQueueSettings({ confirmInterrupt: false }); });
    await act(async () => load.resolve({ confirmInterrupt: true }));
    expect(confirmValue()).toBe('false');
  });

  it('lets the latest write win over an older response', async () => {
    apiMocks.getPromptQueueSettings.mockResolvedValue({ confirmInterrupt: true });
    const first = deferred<{ confirmInterrupt: boolean }>();
    apiMocks.savePromptQueueSettings
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce({ confirmInterrupt: true });
    await act(async () => root.render(<Probe />));

    let firstWrite: Promise<unknown> = Promise.resolve();
    await act(async () => {
      firstWrite = savePromptQueueSettings({ confirmInterrupt: false });
      await savePromptQueueSettings({ confirmInterrupt: true });
    });
    await act(async () => {
      first.resolve({ confirmInterrupt: false });
      await firstWrite;
    });
    expect(confirmValue()).toBe('true');
  });
});
