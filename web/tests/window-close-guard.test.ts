import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import {
  createWindowCloseTransaction,
  type WindowCloseRequest,
  type WindowCloseTransactionDependencies,
} from '@/lib/window-close-guard';

function closeRequest() {
  return { preventDefault: vi.fn() } satisfies WindowCloseRequest;
}

function dependencies(overrides: Partial<WindowCloseTransactionDependencies> = {}) {
  return {
    flushPendingChanges: vi.fn().mockResolvedValue(true),
    requestSaveFailureDecision: vi.fn().mockResolvedValue('cancel'),
    prepareAppExit: vi.fn().mockResolvedValue({ warnings: [] }),
    destroyWindow: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } satisfies WindowCloseTransactionDependencies;
}

describe('window close transaction', () => {
  it('grants both permissions required by the coordinated close path', () => {
    const capability = JSON.parse(readFileSync(
      path.resolve(__dirname, '../../src-tauri/capabilities/default.json'),
      'utf8',
    )) as { permissions?: string[] };

    expect(capability.permissions).toContain('core:window:allow-close');
    expect(capability.permissions).toContain('core:window:allow-destroy');
  });

  it('flushes files before preparing the backend and destroying the window', async () => {
    const order: string[] = [];
    const deps = dependencies({
      flushPendingChanges: vi.fn(async () => { order.push('flush'); return true; }),
      prepareAppExit: vi.fn(async () => { order.push('prepare'); }),
      destroyWindow: vi.fn(async () => { order.push('destroy'); }),
    });
    const event = closeRequest();

    await createWindowCloseTransaction(deps)(event);

    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(order).toEqual(['flush', 'prepare', 'destroy']);
  });

  it('retries saving before any backend exit preparation', async () => {
    const flush = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const deps = dependencies({
      flushPendingChanges: flush,
      requestSaveFailureDecision: vi.fn().mockResolvedValue('retry'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(flush).toHaveBeenCalledTimes(2);
    expect(deps.prepareAppExit).toHaveBeenCalledTimes(1);
    expect(deps.destroyWindow).toHaveBeenCalledTimes(1);
  });

  it('cancels closing without stopping sessions when saving fails', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockResolvedValue(false),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('cancel'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.prepareAppExit).not.toHaveBeenCalled();
    expect(deps.destroyWindow).not.toHaveBeenCalled();
  });

  it('prepares exit and destroys the window after the user discards changes', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockResolvedValue(false),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('discard'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.prepareAppExit).toHaveBeenCalledTimes(1);
    expect(deps.destroyWindow).toHaveBeenCalledTimes(1);
  });

  it('treats an unexpected flush error as a save failure decision', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockRejectedValue(new Error('write failed')),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('discard'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.requestSaveFailureDecision).toHaveBeenCalledTimes(1);
    expect(deps.prepareAppExit).toHaveBeenCalledTimes(1);
  });

  it('shares one transaction across concurrent native close requests', async () => {
    let finishFlush: ((saved: boolean) => void) | undefined;
    const flush = vi.fn(() => new Promise<boolean>((resolve) => {
      finishFlush = resolve;
    }));
    const deps = dependencies({ flushPendingChanges: flush });
    const transaction = createWindowCloseTransaction(deps);
    const first = closeRequest();
    const second = closeRequest();

    const firstClose = transaction(first);
    const secondClose = transaction(second);
    await Promise.resolve();
    expect(flush).toHaveBeenCalledTimes(1);
    expect(first.preventDefault).toHaveBeenCalledTimes(1);
    expect(second.preventDefault).toHaveBeenCalledTimes(1);

    finishFlush?.(true);
    await Promise.all([firstClose, secondClose]);
    expect(deps.prepareAppExit).toHaveBeenCalledTimes(1);
    expect(deps.destroyWindow).toHaveBeenCalledTimes(1);
  });

  it('does not destroy the window when backend exit preparation fails', async () => {
    const deps = dependencies({
      prepareAppExit: vi.fn().mockRejectedValue(new Error('ipc unavailable')),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.destroyWindow).not.toHaveBeenCalled();
  });
});
