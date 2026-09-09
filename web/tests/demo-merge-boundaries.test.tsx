/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { agentIconSrc } from '@/lib/agent-icons';
import { BrandLoadingState } from '@/components/BrandLoadingState';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { ConversationRunVm } from '@/types';
import { demoHref } from '../../marketing/site/config';
import { entryPreferences } from '../../marketing/demo/entry';
import { createDemoApi } from '../../marketing/demo/api';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => vi.unstubAllEnvs());

it('connects the site URL to Demo language and optional theme without changing layout preferences', async () => {
  const initial = (await createDemoApi().getAppBootstrap()).preferences;
  const url = new URL(demoHref('/nested/demo/?theme=light#task-015?project=ji', 'zh', 'https://example.com'));
  expect(url.pathname).toBe('/nested/demo/');
  expect(url.hash).toBe('#task-015?project=ji');
  expect(entryPreferences(initial, url.search)).toEqual({ ...initial, language: 'zh-cn', appearance: { ...initial.appearance, colorScheme: 'light' } });
  const noTheme = new URL(demoHref('/nested/demo/', 'en', 'https://example.com'));
  expect(entryPreferences(initial, noTheme.search)).toEqual({ ...initial, language: 'en' });
});

it('resolves bundled icons and loading logo within the production base', async () => {
  vi.stubEnv('BASE_URL', '/nested/demo/');
  expect(agentIconSrc('claude')).toBe('/nested/demo/agent-icons/claude.svg');
  expect(agentIconSrc('gold-band')).toBe('/nested/demo/logo.svg');
  expect(agentIconSrc('https://example.com/custom.svg')).toBe('https://example.com/custom.svg');
  const container = document.createElement('div');
  const root = createRoot(container);
  try {
    await act(async () => root.render(<BrandLoadingState label="Loading" />));
    expect(container.querySelector('img')?.getAttribute('src')).toBe('/nested/demo/logo.svg');
  } finally { await act(async () => root.unmount()); }
});

it('disables rerun in readonly headers and preserves desktop rerun', async () => {
  const container = document.createElement('div');
  const root = createRoot(container);
  const onRerun = vi.fn();
  const run = { runMode: 'workflow', runStatus: 'paused', sessionTree: { rounds: [], selectedSessionKey: null } } as unknown as ConversationRunVm;
  const render = (readOnly: boolean) => <TooltipProvider><ConversationRunHeader run={run} taskTitle="History" readOnly={readOnly} onRerun={onRerun} onEditWorkflow={() => {}} onViewWorkflow={() => {}} onSessionSwitcherOpenChange={() => {}} sessionSwitcherOpen={false} sessionSwitcher={null} /></TooltipProvider>;
  try {
    await act(async () => root.render(render(true)));
    const button = container.querySelector<HTMLButtonElement>('[aria-label="conversation.runtime.rerun"]')!;
    expect(button.disabled).toBe(true);
    await act(async () => button.click());
    expect(onRerun).not.toHaveBeenCalled();
    await act(async () => root.render(render(false)));
    expect(button.disabled).toBe(false);
    await act(async () => button.click());
    expect(onRerun).toHaveBeenCalledOnce();
  } finally { await act(async () => root.unmount()); }
});
