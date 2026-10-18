/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../src/components/ui/tooltip';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const viewport = { width: 1024, height: 768 };
const visibleTriggerRect = { x: 400, y: 300, width: 20, height: 20 };
// `display: none` 元素的布局矩形。
const hiddenTriggerRect = { x: 0, y: 0, width: 0, height: 0 };

let root: Root | null = null;

function domRect({ x, y, width, height }: typeof visibleTriggerRect) {
  return {
    x, y, width, height,
    top: y, left: x, right: x + width, bottom: y + height,
    toJSON: () => ({}),
  } as DOMRect;
}

async function flushPositioning() {
  await act(async () => {
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
  });
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  Object.defineProperty(document.documentElement, 'clientWidth', { configurable: true, value: viewport.width });
  Object.defineProperty(document.documentElement, 'clientHeight', { configurable: true, value: viewport.height });
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  delete (document.documentElement as Partial<HTMLElement> & Record<string, unknown>).clientWidth;
  delete (document.documentElement as Partial<HTMLElement> & Record<string, unknown>).clientHeight;
});

describe('Tooltip with a trigger hidden while open', () => {
  it('hides the tooltip instead of pinning it to the viewport origin', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => {
      root?.render(
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button">rename</button>
            </TooltipTrigger>
            <TooltipContent>rename-tip</TooltipContent>
          </Tooltip>
        </TooltipProvider>,
      );
    });

    const trigger = container.querySelector('button')!;
    let triggerRect = visibleTriggerRect;
    trigger.getBoundingClientRect = () => domRect(triggerRect);

    await act(async () => {
      trigger.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await flushPositioning();

    const wrapper = () => document.querySelector<HTMLElement>('[data-radix-popper-content-wrapper]')!;
    expect(wrapper().textContent).toContain('rename-tip');
    expect(wrapper().style.visibility).not.toBe('hidden');

    // 触发器所在行失去 :hover 被 display:none，但光标未移动，Radix 收不到 pointerleave。
    triggerRect = hiddenTriggerRect;
    await act(async () => {
      window.dispatchEvent(new Event('resize'));
    });
    await flushPositioning();

    expect(wrapper().textContent).toContain('rename-tip');
    expect(wrapper().style.visibility).toBe('hidden');
  });
});
