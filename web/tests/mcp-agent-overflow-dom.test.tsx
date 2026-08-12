// @vitest-environment jsdom

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import '../src/i18n';
import { McpAgentOverflow } from '../src/components/McpAgentOverflow';
import type { McpAgentCompatibility } from '../src/lib/mcp-agent-compatibility';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class PassiveResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function makeAgent(index: number): McpAgentCompatibility {
  return {
    agentType: `agent-${index}`,
    label: `Agent ${index}`,
    iconKey: 'gold-band',
    diagnosticAvailable: true,
    mcpHttpSupported: null,
  };
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', PassiveResizeObserver);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function getBoundingClientRect() {
    const width = this.getAttribute('data-testid') === 'mcp-agent-overflow' ? 132 : 0;
    return { x: 0, y: 0, width, height: 64, top: 0, right: width, bottom: 64, left: 0, toJSON: () => ({}) };
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('McpAgentOverflow', () => {
  it('uses two rows before +N and keeps hidden unknown Agents diagnosable', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onDiagnoseAgent = vi.fn();

    try {
      await act(async () => {
        root.render(
          <McpAgentOverflow
            agents={Array.from({ length: 11 }, (_, index) => makeAgent(index + 1))}
            transport="http"
            transportLabel="HTTP"
            onDiagnoseAgent={onDiagnoseAgent}
          />,
        );
      });

      const overflowTrigger = [...container.querySelectorAll('button')].find((button) => button.textContent === '+2');
      expect(overflowTrigger).toBeDefined();

      await act(async () => overflowTrigger!.click());
      const hiddenAgent = [...document.body.querySelectorAll<HTMLButtonElement>('button')]
        .find((button) => button.textContent?.includes('Agent 10'));
      expect(hiddenAgent).toBeDefined();

      await act(async () => hiddenAgent!.click());
      expect(onDiagnoseAgent).toHaveBeenCalledWith('agent-10');
      expect(document.body.querySelector('[data-slot="popover-content"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
