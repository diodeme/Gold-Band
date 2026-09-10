/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { afterEach, expect, it, vi } from 'vitest';
import type { RightWorkspaceResource } from '@/components/workspace/right-workspace-context';

const state = vi.hoisted(() => ({ tabs: [] as RightWorkspaceResource[] }));
vi.mock('@/components/workspace/right-workspace-context', async (original) => ({
  ...await original<object>(),
  useRightWorkspace: () => ({
    tabs: state.tabs, activeTabKey: state.tabs[0]?.key,
    activateTab: vi.fn(), closeTab: vi.fn(), renderResource: () => null,
  }),
}));
vi.mock('@/components/workspace/AgentConversationPanel', () => ({ AgentConversationPanel: () => null }));
vi.mock('@/lib/conversation-event-router', () => ({ useConversationBranchLiveSnapshot: () => ({ revision: 0 }) }));
vi.mock('@/components/workspace/files/file-content-store', () => ({ useFileContentEntry: () => ({}) }));
import { RightWorkspaceDock } from '@/components/workspace/RightWorkspaceDock';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { document.body.innerHTML = ''; vi.unstubAllGlobals(); });

it.each(['first.ts', '工作空间'])('renders current functional labels when the stored browser title is %s', async (storedTitle) => {
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  const i18n = createInstance();
  await i18n.init({ lng: 'zh', resources: {
    zh: { translation: { workspace: { files: '工作空间' }, sourceControl: { title: '源码管理' } } },
    en: { translation: { workspace: { files: 'Workspace' }, sourceControl: { title: 'Source Control' } } },
  } });
  const base = { scopeKey: 'scope', attention: false, projectId: 'project' };
  state.tabs = [
    { ...base, kind: 'file-browser', key: 'files', title: storedTitle },
    { ...base, kind: 'source-control', key: 'git', title: '源码管理' },
  ];
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const labels = () => Array.from(host.querySelectorAll('[data-right-workspace-tab] > button:first-child')).map(e => e.textContent);
  try {
    await act(async () => root.render(<I18nextProvider i18n={i18n}><RightWorkspaceDock /></I18nextProvider>));
    expect(labels()).toEqual(['工作空间', '源码管理']);
    const originalTab = host.querySelector('[data-right-workspace-resource-key="files"]');
    await act(async () => { await i18n.changeLanguage('en'); });
    expect(labels()).toEqual(['Workspace', 'Source Control']);
    expect(host.querySelector('[data-right-workspace-resource-key="files"]')).toBe(originalTab);
    expect(state.tabs[0].title).toBe(storedTitle);
  } finally { await act(async () => root.unmount()); }
});
