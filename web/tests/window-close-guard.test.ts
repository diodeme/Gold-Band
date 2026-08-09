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
    completeClose: vi.fn().mockResolvedValue(undefined),
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

  it('flushes files before handing completion to the desktop lifecycle host', async () => {
    const order: string[] = [];
    const deps = dependencies({
      flushPendingChanges: vi.fn(async () => { order.push('flush'); return true; }),
      completeClose: vi.fn(async () => { order.push('complete'); }),
    });
    const event = closeRequest();

    await createWindowCloseTransaction(deps)(event);

    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(order).toEqual(['flush', 'complete']);
  });

  it('retries saving before completing the close', async () => {
    const flush = vi.fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const deps = dependencies({
      flushPendingChanges: flush,
      requestSaveFailureDecision: vi.fn().mockResolvedValue('retry'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(flush).toHaveBeenCalledTimes(2);
    expect(deps.completeClose).toHaveBeenCalledTimes(1);
  });

  it('cancels closing without stopping sessions when saving fails', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockResolvedValue(false),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('cancel'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.completeClose).not.toHaveBeenCalled();
  });

  it('completes the close after the user discards changes', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockResolvedValue(false),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('discard'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.completeClose).toHaveBeenCalledTimes(1);
  });

  it('treats an unexpected flush error as a save failure decision', async () => {
    const deps = dependencies({
      flushPendingChanges: vi.fn().mockRejectedValue(new Error('write failed')),
      requestSaveFailureDecision: vi.fn().mockResolvedValue('discard'),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.requestSaveFailureDecision).toHaveBeenCalledTimes(1);
    expect(deps.completeClose).toHaveBeenCalledTimes(1);
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
    expect(deps.completeClose).toHaveBeenCalledTimes(1);
  });

  it('does not retry a failed host completion inside the save transaction', async () => {
    const deps = dependencies({
      completeClose: vi.fn().mockRejectedValue(new Error('ipc unavailable')),
    });

    await createWindowCloseTransaction(deps)(closeRequest());

    expect(deps.completeClose).toHaveBeenCalledTimes(1);
  });

  it('can run the save guard for a native app-exit handshake without closing directly', async () => {
    const deps = dependencies();
    const transaction = createWindowCloseTransaction(deps);

    expect(await transaction.prepareToClose()).toBe(true);
    expect(deps.completeClose).not.toHaveBeenCalled();
  });
});
