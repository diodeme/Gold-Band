// @vitest-environment jsdom

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import '../src/i18n';
import { SkillAgentOverflow } from '../src/components/SkillAgentOverflow';
import type { AgentCatalogEntryVm } from '../src/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class PassiveResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function makeAgent(index: number): AgentCatalogEntryVm {
  return {
    agentType: `agent-${index}`,
    label: `Agent ${index}`,
    iconKey: 'gold-band',
    version: '1',
    description: '',
    repository: null,
    website: null,
    primaryAgentDir: `.agent-${index}`,
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
    configured: true,
    supportsSystemPrompt: false,
    supportsExternalSessionSync: false,
    defaultDisplayName: `Agent ${index}`,
    defaultCommand: 'agent',
    defaultArgs: [],
    defaultEnv: [],
  };
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', PassiveResizeObserver);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function getBoundingClientRect() {
    const width = this.getAttribute('data-testid') === 'skill-agent-overflow' ? 132 : 0;
    return { x: 0, y: 0, width, height: 64, top: 0, right: width, bottom: 64, left: 0, toJSON: () => ({}) };
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('SkillAgentOverflow', () => {
  it('opens hidden agents after two rows and keeps the popover interactive', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onToggleAgent = vi.fn();
    const syncAgents = Array.from({ length: 11 }, (_, index) => makeAgent(index + 1));

    try {
      await act(async () => {
        root.render(
          <SkillAgentOverflow
            sourceAgents={[]}
            syncAgents={syncAgents}
            syncedAgentTypes={new Set()}
            isPending={() => false}
            onToggleAgent={onToggleAgent}
          />,
        );
      });

      const overflowTrigger = [...container.querySelectorAll('button')].find((button) => button.textContent === '+2');
      expect(overflowTrigger).toBeDefined();

      await act(async () => overflowTrigger!.click());
      const hiddenAgent = document.body.querySelector<HTMLButtonElement>('button[aria-label="同步到 Agent 10"]');
      expect(hiddenAgent).not.toBeNull();

      await act(async () => hiddenAgent!.click());
      expect(onToggleAgent).toHaveBeenCalledWith('agent-10');
      expect(document.body.querySelector('[data-slot="popover-content"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
