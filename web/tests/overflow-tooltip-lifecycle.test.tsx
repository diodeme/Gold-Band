/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { useOverflowTooltip } from '@/hooks/useOverflowTooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function OverflowControl({ label, always = false }: { label: string; always?: boolean }) {
  const { valueRef, tooltipOpen, handleTooltipFocus, hideTooltip, handleTooltipOpenChange } = useOverflowTooltip({ always });
  return (
    <Tooltip open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
      <TooltipTrigger asChild>
        <button
          data-control={label}
          onFocus={handleTooltipFocus}
          onBlur={hideTooltip}
          onPointerLeave={hideTooltip}
        >
          <span ref={valueRef}>{label}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function pointer(target: Element, type: string, pointerType = 'mouse') {
  const event = new MouseEvent(type, { bubbles: true, relatedTarget: document.body });
  Object.defineProperty(event, 'pointerType', { value: pointerType });
  target.dispatchEvent(event);
}

describe('overflow tooltip lifecycle', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    host.remove();
  });

  async function render({ always = false, overflow = true, delay = 0 } = {}) {
    await act(async () => root.render(
      <TooltipProvider delayDuration={delay}>
        <OverflowControl label="first" always={always} />
        <OverflowControl label="second" always={always} />
      </TooltipProvider>,
    ));
    const triggers = Array.from(host.querySelectorAll<HTMLButtonElement>('button'));
    for (const trigger of triggers) {
      Object.defineProperties(trigger.querySelector('span')!, {
        clientWidth: { configurable: true, value: 80 },
        scrollWidth: { configurable: true, get: () => overflow ? 240 : 80 },
      });
    }
    return triggers;
  }

  async function enter(trigger: Element, pointerType = 'mouse') {
    await act(async () => {
      pointer(trigger, 'pointerover', pointerType);
      pointer(trigger, 'pointermove', pointerType);
    });
    await act(async () => vi.advanceTimersByTimeAsync(0));
  }

  function tooltip() {
    return document.body.querySelector('[data-slot="tooltip-content"]');
  }

  it.each([false, true])('measures overflow only when Radix requests opening (overflow=%s)', async (overflow) => {
    const [trigger] = await render({ overflow });
    const measure = vi.spyOn(trigger.querySelector('span')!, 'scrollWidth', 'get');
    await enter(trigger);
    expect(Boolean(tooltip())).toBe(overflow);
    expect(measure).toHaveBeenCalledTimes(1);
    await act(async () => pointer(trigger, 'pointermove'));
    expect(measure).toHaveBeenCalledTimes(1);
  });

  it('keeps icon-only controls discoverable without requiring overflow', async () => {
    const [trigger] = await render({ always: true, overflow: false });
    await enter(trigger);
    expect(tooltip()?.textContent).toBe('first');
  });

  it('cancels a pending open when the pointer leaves before the provider delay', async () => {
    const [trigger] = await render({ delay: 100 });
    await enter(trigger);
    expect(tooltip()).toBeNull();
    await act(async () => pointer(trigger, 'pointerout'));
    await act(async () => vi.advanceTimersByTimeAsync(100));
    expect(tooltip()).toBeNull();
  });

  it('uses Radix mutual exclusion when another tooltip opens', async () => {
    const [first, second] = await render();
    await enter(first);
    expect(tooltip()?.textContent).toBe('first');
    // Leave the first tooltip mounted to exercise Radix's cross-tooltip dismissal.
    await enter(second);
    expect(Array.from(document.body.querySelectorAll('[data-slot="tooltip-content"]'))
      .map((element) => element.textContent)).toEqual(['second']);
  });

  it('does not open on touch pointer movement', async () => {
    const [trigger] = await render();
    await enter(trigger, 'touch');
    expect(tooltip()).toBeNull();
  });

  it('blocks pointer-restored focus before Radix broadcasts an open', async () => {
    const [trigger] = await render();
    const matches = trigger.matches.bind(trigger);
    vi.spyOn(trigger, 'matches').mockImplementation((selector) => selector === ':focus-visible' ? false : matches(selector));
    const broadcast = vi.spyOn(document, 'dispatchEvent');
    await act(async () => trigger.focus());
    expect(document.activeElement).toBe(trigger);
    expect(tooltip()).toBeNull();
    expect(broadcast.mock.calls.some(([event]) => event.type === 'tooltip.open')).toBe(false);
  });

  it('opens on keyboard focus and closes on blur', async () => {
    const [trigger] = await render();
    const matches = trigger.matches.bind(trigger);
    vi.spyOn(trigger, 'matches').mockImplementation((selector) => selector === ':focus-visible' ? true : matches(selector));
    await act(async () => trigger.focus());
    expect(tooltip()?.textContent).toBe('first');
    await act(async () => trigger.blur());
    expect(tooltip()).toBeNull();
  });

  it('closes on ancestor scroll and can reopen after leaving and entering again', async () => {
    const [trigger] = await render();
    await enter(trigger);
    expect(tooltip()?.textContent).toBe('first');
    await act(async () => host.dispatchEvent(new Event('scroll')));
    expect(tooltip()).toBeNull();
    await act(async () => pointer(trigger, 'pointerout'));
    await enter(trigger);
    expect(tooltip()?.textContent).toBe('first');
  });
});
