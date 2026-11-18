/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { OnboardingGate } from '@/onboarding/OnboardingGate';
import { createBrowserSetupApi } from '@/onboarding/browser-fixture';
import type { SetupApi, SetupStatus } from '@/onboarding/types';
import i18n from '@/i18n';

type CloseListener = (event: { preventDefault(): void }) => unknown;
type ExitListener = (event: { requestId: string }) => unknown;
const native = vi.hoisted(() => ({
  closeListeners: new Set<CloseListener>(),
  exitListeners: new Set<ExitListener>(),
  completeClose: vi.fn(async () => {}),
  resolveExit: vi.fn(async () => {}),
  flushAll: vi.fn(async () => false),
  setup: vi.fn(),
}));
vi.mock('@/api', () => ({
  completeMainWindowClose: native.completeClose,
  resolveAppExit: native.resolveExit,
  openExternalUrl: vi.fn(),
  subscribeAppExitRequested: async (listener: ExitListener) => {
    native.exitListeners.add(listener);
    return () => native.exitListeners.delete(listener);
  },
}));
vi.mock('@/api/shared', () => ({ isTauriRuntime: () => true }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    show: async () => {},
    onCloseRequested: async (listener: CloseListener) => {
      native.closeListeners.add(listener);
      return () => native.closeListeners.delete(listener);
    },
  }),
}));
vi.mock('@/components/workspace/files/file-content-store', () => ({ fileContentStore: { flushAll: native.flushAll } }));
vi.mock('@/onboarding/api', () => ({ setupApi: native.setup }));
vi.mock('@/App', () => ({ App: () => <div>Business application</div> }));
vi.mock('@/theme', () => ({ applyAppearance: vi.fn() }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let api: SetupApi;
let status: SetupStatus;
beforeEach(async () => {
  await i18n.changeLanguage('en');
  vi.clearAllMocks();
  native.flushAll.mockReset().mockResolvedValue(false);
  api = createBrowserSetupApi();
  status = await api.status();
  status.state = { status: 'existing-install' };
  status.platform = 'windows';
  status.language = 'en';
  vi.spyOn(api, 'start').mockResolvedValue('generation-1');
  vi.spyOn(api, 'cancel');
  native.setup.mockResolvedValue(api);
  history.replaceState(null, '', '#/onboarding/defaults');
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  expect(native.closeListeners.size).toBe(0);
  expect(native.exitListeners.size).toBe(0);
  vi.restoreAllMocks();
});

async function render() {
  await act(async () => root.render(<OnboardingGate />));
  expect(host.textContent).toContain('Connect an Agent');
  expect(native.closeListeners.size).toBe(1);
  expect(native.exitListeners.size).toBe(1);
}
async function requestClose() {
  const event = { preventDefault: vi.fn() };
  const transactions: unknown[] = [];
  await act(async () => {
    for (const listener of native.closeListeners) transactions.push(listener(event));
  });
  expect(event.preventDefault).toHaveBeenCalledTimes(1);
  return { completed: Promise.all(transactions) };
}
async function requestExit(requestId: string) {
  const transactions: unknown[] = [];
  await act(async () => {
    for (const listener of native.exitListeners) transactions.push(listener({ requestId }));
  });
  return { completed: Promise.all(transactions) };
}
async function decide(label: string) {
  const button = [...document.body.querySelectorAll('button')].find((item) => item.textContent === label);
  expect(button).toBeDefined();
  await act(async () => button!.click());
}

it('preserves unsaved files and the active setup run when native close is cancelled', async () => {
  await render();
  const closing = await requestClose();
  expect(native.flushAll).toHaveBeenCalledTimes(1);
  expect(document.body.querySelector('[role="alertdialog"]')).not.toBeNull();
  expect(native.completeClose).not.toHaveBeenCalled();
  await decide(i18n.t('common.cancelClose'));
  await closing.completed;
  expect(native.completeClose).not.toHaveBeenCalled();
  expect(api.cancel).not.toHaveBeenCalled();
  expect(host.textContent).toContain('Connect an Agent');
});

it('retries the existing save transaction before completing native close', async () => {
  native.flushAll.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  await render();
  const closing = await requestClose();
  expect(native.completeClose).not.toHaveBeenCalled();
  await decide(i18n.t('common.retrySave'));
  await closing.completed;
  expect(native.flushAll).toHaveBeenCalledTimes(2);
  expect(native.completeClose).toHaveBeenCalledTimes(1);
});

it('requires an explicit discard before closing after a save failure', async () => {
  await render();
  const closing = await requestClose();
  expect(native.completeClose).not.toHaveBeenCalled();
  await decide(i18n.t('common.discardChangesAndExit'));
  await closing.completed;
  expect(native.completeClose).toHaveBeenCalledTimes(1);
});

it('answers native app exit only after the user cancels or accepts the save failure', async () => {
  await render();
  const cancelled = await requestExit('exit-cancel');
  expect(native.flushAll).toHaveBeenCalledTimes(1);
  expect(native.resolveExit).not.toHaveBeenCalled();
  await decide(i18n.t('common.cancelClose'));
  await cancelled.completed;
  expect(native.resolveExit).toHaveBeenCalledWith({ requestId: 'exit-cancel', decision: 'cancel' });
  expect(api.cancel).not.toHaveBeenCalled();
  const accepted = await requestExit('exit-discard');
  await decide(i18n.t('common.discardChangesAndExit'));
  await accepted.completed;
  expect(native.resolveExit).toHaveBeenLastCalledWith({ requestId: 'exit-discard', decision: 'proceed' });
  expect(native.completeClose).not.toHaveBeenCalled();
});

it('keeps the same close guard and pending decision while switching to the business app', async () => {
  await render();
  const closing = await requestClose();
  const closeListener = [...native.closeListeners][0];
  let finishStatus!: (value: SetupStatus) => void;
  vi.spyOn(api, 'status').mockReturnValueOnce(new Promise((resolve) => { finishStatus = resolve; }));
  await act(async () => {
    history.replaceState(null, '', '#/settings');
    window.dispatchEvent(new HashChangeEvent('hashchange'));
  });
  expect(native.closeListeners.size).toBe(1);
  expect([...native.closeListeners][0]).toBe(closeListener);
  expect(document.body.querySelector('[role="alertdialog"]')).not.toBeNull();
  await act(async () => finishStatus(status));
  // React.lazy must settle before leaving act, including the dynamically imported app.
  await act(async () => { await vi.dynamicImportSettled(); });
  expect(host.textContent).toContain('Business application');
  expect([...native.closeListeners][0]).toBe(closeListener);
  await decide(i18n.t('common.cancelClose'));
  await closing.completed;
  expect(native.completeClose).not.toHaveBeenCalled();
});

it('uses macOS window-close wording while app exit still uses exit wording', async () => {
  status.platform = 'macos';
  await render();
  const closing = await requestClose();
  expect(document.body.textContent).toContain(i18n.t('common.discardChangesAndClose'));
  await decide(i18n.t('common.cancelClose'));
  await closing.completed;
  const exiting = await requestExit('mac-exit');
  expect(document.body.textContent).toContain(i18n.t('common.discardChangesAndExit'));
  await decide(i18n.t('common.cancelClose'));
  await exiting.completed;
});
