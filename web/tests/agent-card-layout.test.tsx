/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import { mockAgentRegistry } from '../src/mockData';
import i18n from '../src/i18n';

vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);

let cleanup: (() => void) | undefined;

afterEach(() => {
  cleanup?.();
  cleanup = undefined;
});

it('keeps each Agent as one card and sizes the grid from the list container', async () => {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  cleanup = () => {
    act(() => root.unmount());
    container.remove();
  };

  await act(async () => {
    root.render(
      <AgentManagementPage
        vm={mockAgentRegistry}
        loading={false}
        onRefresh={() => {}}
        onRegistryChange={() => {}}
      />,
    );
  });

  const list = container.querySelector<HTMLElement>('[data-webview-container="agent-list"]');
  const grid = container.querySelector<HTMLElement>('[data-slot="agent-card-grid"]');
  expect(list?.className).toContain('@container/agent-list');
  expect(grid?.className).toContain('@2xl/agent-list:grid-cols-2');
  expect(grid?.className).toContain('@6xl/agent-list:grid-cols-3');
  expect(grid?.className).not.toContain('md:grid-cols-2');
  expect(grid?.className).not.toContain('xl:grid-cols-3');

  const cards = [...container.querySelectorAll<HTMLElement>('[data-slot="card"]')]
    .filter((card) => card.querySelector('h3'));
  expect(cards).toHaveLength(mockAgentRegistry.agents.length);

  for (const card of cards) {
    const actions = [...card.querySelectorAll('button')].map((button) => button.textContent);
    expect(actions).toEqual(expect.arrayContaining([
      i18n.t('agentManagement.diagnose'),
      i18n.t('agentManagement.edit'),
      i18n.t('agentManagement.delete'),
    ]));
  }
});
