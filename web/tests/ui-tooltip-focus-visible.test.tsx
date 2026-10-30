/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../src/components/ui/tooltip';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
// jsdom matches `:focus-visible` for every focus; this flag stands in for the
// browser's keyboard-modality heuristic.
let keyboardModality = true;

beforeEach(() => {
  keyboardModality = true;
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  const matches = Element.prototype.matches;
  vi.spyOn(Element.prototype, 'matches').mockImplementation(function (this: Element, selector: string) {
    if (selector === ':focus-visible') return keyboardModality && document.activeElement === this;
    return matches.call(this, selector);
  });
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function renderTooltip() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(
      <TooltipProvider delayDuration={0}>
        <Tooltip>
          <TooltipTrigger asChild>
            <button type="button">Drafts</button>
          </TooltipTrigger>
          <TooltipContent>Drafts tooltip</TooltipContent>
        </Tooltip>
      </TooltipProvider>,
    );
  });
  return container.querySelector('button')!;
}

function tooltipOpen() {
  return document.body.querySelector('[data-slot="tooltip-content"]') !== null;
}

describe('Tooltip focus modality', () => {
  it('stays closed when focus moves to the trigger programmatically after a pointer action', async () => {
    const trigger = await renderTooltip();
    keyboardModality = false;
    // e.g. a popover returning focus to its trigger, or auto-focusing its first control.
    await act(async () => trigger.focus());
    expect(tooltipOpen()).toBe(false);
  });

  it('opens when the trigger receives keyboard focus', async () => {
    const trigger = await renderTooltip();
    await act(async () => trigger.focus());
    expect(tooltipOpen()).toBe(true);
  });
});
