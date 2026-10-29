/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AcpRunningAgentsStrip, type AcpRunningAgentCard } from '@/components/acp/AcpRunningAgentsStrip';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ACP_SESSION_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';

function card(agentExecutionId: string, overrides: Partial<AcpRunningAgentCard> = {}): AcpRunningAgentCard {
  return {
    agentExecutionId,
    label: `${agentExecutionId} review`,
    statusLabel: 'Running',
    metricsSummary: '',
    attention: false,
    canOpen: true,
    ...overrides,
  };
}

describe('AcpRunningAgentsStrip', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function render(agents: AcpRunningAgentCard[], onOpen = vi.fn()) {
    await act(async () => root.render(
      <TooltipProvider>
        <AcpRunningAgentsStrip
          agents={agents}
          ariaLabel="Agents"
          stackSlot={{ above: 'none', below: 'seamless', infoTab: true, gapBelow: false }}
          onOpen={onOpen}
        />
      </TooltipProvider>,
    ));
    return onOpen;
  }

  it('renders nothing when no Agent is running', async () => {
    await render([]);
    expect(host.querySelector('[data-acp-running-agents="true"]')).toBeNull();
  });

  it('opens the clicked Agent and scrolls inside a height-limited area', async () => {
    const onOpen = await render([card('agent-a'), card('agent-b')]);
    const strip = host.querySelector<HTMLElement>('[data-acp-running-agents="true"]');
    expect(strip?.className).toContain(ACP_SESSION_COMPOSER_LAYOUT.runningAgentsMaxHeightClassName);
    expect(strip?.className).toContain('overflow-y-auto');
    expect(strip?.className).toContain('border-b-0');

    await act(async () => host.querySelector<HTMLButtonElement>('[data-acp-running-agent="agent-b"]')!.click());
    expect(onOpen).toHaveBeenCalledWith('agent-b');
  });

  it('disables an Agent that cannot be opened yet', async () => {
    const onOpen = await render([card('agent-a', { canOpen: false })]);
    const button = host.querySelector<HTMLButtonElement>('[data-acp-running-agent="agent-a"]')!;
    expect(button.disabled).toBe(true);
    await act(async () => button.click());
    expect(onOpen).not.toHaveBeenCalled();
  });
});
