/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ConversationWorkspaceControl } from '@/components/conversation/ConversationComposer';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const workspaces = [
  { projectId: 'gold-band', workspacePath: 'D:/Projects/Gold-Band', name: 'Gold Band' },
  { projectId: 'long', workspacePath: 'D:/Projects/Long', name: 'A very long workspace name that must be truncated' },
];

describe('quick conversation workspace control', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  async function renderControl(projectId: string) {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId={projectId}
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={() => {}}
        />,
      );
    });

    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    const value = host.querySelector<HTMLElement>('[data-conversation-workspace-value="true"]');
    expect(trigger).not.toBeNull();
    expect(value).not.toBeNull();
    return { trigger: trigger!, value: value! };
  }

  it('sizes to its content while preserving the available-width ceiling', async () => {
    const { trigger } = await renderControl('gold-band');

    expect(trigger.className).toContain('w-fit');
    expect(trigger.className).toContain('max-w-full');
    expect(trigger.className).toContain('flex-initial');
    expect(trigger.className).not.toContain('flex-1');
    expect(trigger.textContent).toContain('Gold Band');
  });

  it('shows the complete workspace name when the selected value is truncated', async () => {
    const { trigger, value } = await renderControl('long');
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 340 },
    });

    await act(async () => {
      trigger.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
    });

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('A very long workspace name that must be truncated');
  });

  it('does not show a redundant tooltip when the workspace name fits', async () => {
    const { trigger, value } = await renderControl('gold-band');
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 120 },
    });

    await act(async () => trigger.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')).toBeNull();
  });

  it('keeps a truncated single-workspace label available from the keyboard', async () => {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="long"
          workspaceName="Fallback workspace"
          workspaces={[workspaces[1]]}
          onWorkspaceChange={() => {}}
        />,
      );
    });
    const control = host.querySelector<HTMLElement>('[tabindex="0"]');
    const value = host.querySelector<HTMLElement>('[data-conversation-workspace-value="true"]');
    expect(control).not.toBeNull();
    expect(value).not.toBeNull();
    Object.defineProperties(value!, {
      clientWidth: { configurable: true, value: 120 },
      scrollWidth: { configurable: true, value: 340 },
    });

    await act(async () => control!.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('A very long workspace name that must be truncated');
  });
});
