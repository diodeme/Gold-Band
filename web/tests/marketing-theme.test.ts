/** @vitest-environment jsdom */
import { act } from 'react';
import * as client from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { effectiveTheme, readThemePreference } from '../../marketing/site/theme';

vi.mock('react-dom/client', async importOriginal => {
  const original = await importOriginal<typeof client>();
  return { ...original, createRoot: vi.fn(original.createRoot) };
});

it('honors explicit appearance preferences and validates stored versions', () => {
  expect(readThemePreference({ getItem: () => JSON.stringify({ version: 1, theme: 'light' }) })).toBe('light');
  for (const value of ['null', '{}', 'broken', '{"version":2,"theme":"dark"}', '{"version":1,"theme":"invalid"}']) {
    expect(readThemePreference({ getItem: () => value })).toBe('system');
  }
  expect(readThemePreference({ getItem: () => { throw new Error('unavailable'); } })).toBe('system');
  expect(effectiveTheme('light', true)).toBe('light');
  expect(effectiveTheme('dark', false)).toBe('dark');
  expect(effectiveTheme('system', true)).toBe('dark');
  expect(effectiveTheme('system', false)).toBe('light');
});

it('returns theme-menu focus to the trigger without scrolling the story', async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  history.replaceState(null, '', '/en/');
  const container = document.createElement('div');
  container.id = 'root'; document.body.append(container);
  const root = client.createRoot(container);
  vi.mocked(client.createRoot).mockReturnValueOnce(root);
  const system = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal('matchMedia', (query: string) => query === '(prefers-color-scheme: dark)' ? system : { matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal('IntersectionObserver', class { observe() {} disconnect() {} });
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  const focus = vi.spyOn(HTMLElement.prototype, 'focus');
  try {
    await act(async () => { await import('../../marketing/site/main'); });
    const trigger = container.querySelector<HTMLButtonElement>('button[aria-label="Theme"]')!;
    await act(async () => { trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); });
    const choice = [...document.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent === 'Light')!;
    expect(choice).toBeTruthy();
    focus.mockClear();
    await act(async () => { choice.click(); await new Promise(resolve => setTimeout(resolve, 10)); });
    expect(document.documentElement.dataset.siteTheme).toBe('light');
    expect(document.activeElement).toBe(trigger);
    const calls = focus.mock.contexts.map((target, index) => ({ target, options: focus.mock.calls[index][0] }));
    expect(calls.filter(call => call.target === trigger)).toEqual([{ target: trigger, options: { preventScroll: true } }]);
    await act(async () => { system.matches = true; system.dispatchEvent(new Event('change')); });
    expect(document.documentElement.dataset.siteTheme).toBe('light');
    await act(async () => { trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); });
    await act(async () => {
      [...document.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find(item => item.textContent === 'System')!.click();
      await new Promise(resolve => setTimeout(resolve, 10));
    });
    expect(document.documentElement.dataset.siteTheme).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    await act(async () => { system.matches = false; system.dispatchEvent(new Event('change')); });
    expect(document.documentElement.dataset.siteTheme).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  } finally {
    await act(async () => root.unmount());
    focus.mockRestore(); vi.unstubAllGlobals(); container.remove(); localStorage.clear();
  }
});
