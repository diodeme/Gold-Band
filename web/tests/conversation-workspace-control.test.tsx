/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ConversationWorkspaceControl, ConversationWorkspaceInfoBar } from '@/components/conversation/ConversationComposer';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const workspaces = [
  { projectId: 'gold-band', workspacePath: 'D:/Projects/Gold-Band', name: 'Gold Band' },
  { projectId: 'long', workspacePath: 'D:/Projects/Long', name: 'A very long workspace name that must be truncated' },
];

function dispatchPointerEvent(target: Element, type: string) {
  const event = new MouseEvent(type, { bubbles: true, button: 0 });
  Object.defineProperties(event, {
    pointerId: { value: 1 },
    pointerType: { value: 'mouse' },
  });
  target.dispatchEvent(event);
}

describe('quick conversation workspace control', () => {
  let host: HTMLDivElement;
  let root: Root;
  let addedHTMLElementMethods: string[];

  beforeEach(() => {
    addedHTMLElementMethods = [];
    for (const method of ['hasPointerCapture', 'setPointerCapture', 'releasePointerCapture', 'scrollIntoView']) {
      if (method in HTMLElement.prototype) continue;
      Object.defineProperty(HTMLElement.prototype, method, {
        configurable: true,
        value: method === 'hasPointerCapture' ? () => false : () => {},
      });
      addedHTMLElementMethods.push(method);
    }
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
    for (const method of addedHTMLElementMethods) {
      delete (HTMLElement.prototype as unknown as Record<string, unknown>)[method];
    }
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

  it('does not leave pointer focus on the workspace trigger after a selection closes', async () => {
    const onWorkspaceChange = vi.fn();
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="gold-band"
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={onWorkspaceChange}
        />,
      );
    });
    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    expect(trigger).not.toBeNull();

    await act(async () => {
      trigger!.focus();
      dispatchPointerEvent(trigger!, 'pointerdown');
    });
    const item = Array.from(document.body.querySelectorAll<HTMLElement>('[data-slot="select-item"]'))
      .find((candidate) => candidate.textContent?.includes('A very long workspace name'));
    expect(item).not.toBeNull();

    await act(async () => {
      dispatchPointerEvent(item!, 'pointermove');
      dispatchPointerEvent(item!, 'pointerup');
    });

    expect(onWorkspaceChange).toHaveBeenCalledWith('long');
    expect(document.activeElement).not.toBe(trigger);
  });

  it('keeps Radix focus restoration when the workspace selector closes from the keyboard', async () => {
    await act(async () => {
      root.render(
        <ConversationWorkspaceControl
          projectId="gold-band"
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          onWorkspaceChange={() => {}}
        />,
      );
    });
    const trigger = host.querySelector<HTMLButtonElement>('[data-slot="select-trigger"]');
    expect(trigger).not.toBeNull();

    await act(async () => {
      trigger!.focus();
      trigger!.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'ArrowDown' }));
    });
    await act(async () => {
      document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Escape' }));
    });

    expect(document.activeElement).toBe(trigger);
  });

  it('combines workspace and persisted work-location controls in the quick composer info bar', async () => {
    await act(async () => {
      root.render(
        <ConversationWorkspaceInfoBar
          projectId="gold-band"
          workspaceName="Fallback workspace"
          workspaces={workspaces}
          workLocation="main"
          busy={false}
          onWorkspaceChange={() => {}}
          onWorkLocationChange={() => {}}
        />,
      );
    });

    const infoBar = host.querySelector<HTMLElement>('[data-conversation-workspace-info="true"]');
    const workspaceTrigger = infoBar?.querySelector<HTMLElement>('[data-slot="select-trigger"]');
    expect(infoBar).not.toBeNull();
    expect(workspaceTrigger).not.toBeNull();
    expect(workspaceTrigger!.className).toContain('dark:bg-transparent');
    expect(workspaceTrigger!.className).not.toContain('dark:bg-gold-surface-high/35');
    expect(infoBar!.className).toContain('mx-12');
    expect(infoBar!.className).toContain('h-8');
    expect(infoBar!.className).toContain('items-center');
    expect(infoBar!.className).toContain('rounded-t-2xl');
    expect(infoBar!.className).toContain('[--conversation-workspace-info-surface:light-dark(var(--gold-surface-high),var(--gb-conversation-background))]');
    expect(infoBar!.className).toContain('before:-left-4');
    expect(infoBar!.className).toContain('before:[background:radial-gradient(circle_at_top_left,transparent_0_15px,var(--conversation-workspace-info-surface)_16px)]');
    expect(infoBar!.className).toContain('after:-right-4');
    expect(infoBar!.className).toContain('after:[background:radial-gradient(circle_at_top_right,transparent_0_15px,var(--conversation-workspace-info-surface)_16px)]');
    expect(infoBar!.className).not.toContain('bg-muted/45');
    expect(infoBar!.className).not.toContain('pt-2');
    expect(host.querySelector('[data-conversation-work-location-trigger="true"]')).not.toBeNull();
    expect(host.querySelector('[data-conversation-workspace-value="true"]')?.textContent).toBe('Gold Band');
  });
});
