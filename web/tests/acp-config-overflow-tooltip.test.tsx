/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('ACP config overflow tooltip', () => {
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

  async function renderPermissionMenu() {
    await act(async () => {
      root.render(
        <TooltipProvider>
          <AcpSingleConfigMenu
            label="权限"
            value="agent-full-access"
            options={[{ id: 'agent-full-access', name: 'Agent full access' }]}
            unspecifiedLabel="不指定"
            onValueChange={() => {}}
          />
        </TooltipProvider>,
      );
    });

    const button = host.querySelector<HTMLButtonElement>('[data-slot="dropdown-menu-trigger"]');
    const value = host.querySelector<HTMLElement>('[data-acp-config-value="true"]');
    expect(button).not.toBeNull();
    expect(value).not.toBeNull();
    return { button: button!, value: value! };
  }

  it('shows the complete selected value on hover when it is truncated', async () => {
    const { button, value } = await renderPermissionMenu();
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 96 },
      scrollWidth: { configurable: true, value: 220 },
    });

    await act(async () => {
      button.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
    });

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('Agent full access');
  });

  it('also shows the complete truncated value on keyboard focus', async () => {
    const { button, value } = await renderPermissionMenu();
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 96 },
      scrollWidth: { configurable: true, value: 220 },
    });

    await act(async () => button.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent)
      .toBe('Agent full access');
  });

  it('does not show a tooltip when the selected value fits', async () => {
    const { button, value } = await renderPermissionMenu();
    Object.defineProperties(value, {
      clientWidth: { configurable: true, value: 160 },
      scrollWidth: { configurable: true, value: 160 },
    });

    await act(async () => button.focus());

    expect(document.body.querySelector('[data-slot="tooltip-content"]')).toBeNull();
  });
});
