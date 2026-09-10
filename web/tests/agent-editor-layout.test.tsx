/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import { mockAgentRegistry } from '../src/mockData';
import { ReadOnlyExperience } from '../src/components/ReadOnlyExperience';
import i18n from '../src/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
it.each([false, true])('keeps editor actions outside scrolling fields (read-only: %s)', async readOnly => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  try {
    await i18n.changeLanguage('en');
    await act(async () => root.render(<ReadOnlyExperience.Provider value={readOnly}><AgentManagementPage
      vm={mockAgentRegistry} loading={false} onRefresh={() => {}} onRegistryChange={() => {}} /></ReadOnlyExperience.Provider>));
    await act(async () => [...container.querySelectorAll('button')].find(button => button.textContent === 'Edit')!.click());
    const dialog = document.querySelector('[role="dialog"]')!;
    const save = [...dialog.querySelectorAll('button')].find(button => button.textContent === 'Save')!;
    expect(save).toBeDefined();
    expect(save.disabled).toBe(true);
    expect(save.closest('[data-slot="sheet-footer"]')).not.toBeNull();
    expect(save.closest('.overflow-y-auto')).toBeNull();
    expect(dialog.querySelector('.overflow-y-auto input')).not.toBeNull();
  } finally {
    await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals();
  }
});
