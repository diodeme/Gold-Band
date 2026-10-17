/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AcpModelThoughtSelects } from '@/components/acp/AcpModelThoughtSelects';
import { TooltipProvider } from '@/components/ui/tooltip';
import { CollisionBoundaryContext, overlayFitsBeside } from '@/lib/portal-container';
import i18n from '@/i18n';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MENU_RECT = { left: 100, right: 400 };
let boundaryRight = 0;
let host: HTMLDivElement;
let boundary: HTMLElement;
let root: Root;
const onModel = vi.fn();

function rect(left: number, right: number): DOMRect {
  return { left, right, x: left, y: 0, top: 0, bottom: 300, width: right - left, height: 300, toJSON() {} } as DOMRect;
}

beforeEach(async () => {
  await i18n.changeLanguage('zh-CN');
  onModel.mockReset();
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  const original = Element.prototype.getBoundingClientRect;
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    if (this === boundary) return rect(0, boundaryRight);
    if (this.getAttribute('data-slot') === 'dropdown-menu-content') return rect(MENU_RECT.left, MENU_RECT.right);
    return original.call(this);
  });
  boundary = document.createElement('main');
  host = document.createElement('div');
  boundary.append(host);
  document.body.append(boundary);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  boundary.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function openComposite() {
  await act(async () => root.render(
    <CollisionBoundaryContext.Provider value={boundary}>
      <TooltipProvider>
        <AcpModelThoughtSelects
          models={[{ id: 'a', name: 'Model A' }, { id: 'b', name: 'Model B' }]}
          modelValue="a"
          thoughtLevel={{ id: 'effort', category: 'thought_level', currentValue: 'low', options: [{ value: 'low', name: 'Low' }] }}
          thoughtValue="low"
          onModelChange={onModel}
          onThoughtChange={vi.fn()}
        />
      </TooltipProvider>
    </CollisionBoundaryContext.Provider>,
  ));
  const trigger = host.querySelector<HTMLButtonElement>('button[data-slot="dropdown-menu-trigger"]')!;
  await act(async () => {
    const event = new MouseEvent('pointerdown', { bubbles: true, button: 0 });
    Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' } });
    trigger.dispatchEvent(event);
  });
}

const menu = () => document.querySelector('[data-slot="dropdown-menu-content"]')!;
const modelTrigger = () => menu().querySelector<HTMLElement>('[data-slot="dropdown-menu-sub-trigger"]')!;
const flyout = () => document.querySelector('[data-slot="dropdown-menu-sub-content"]');

describe('composite config submenu placement', () => {
  it('expands the section inline when neither side of the menu can fit the submenu', async () => {
    boundaryRight = 480;
    await openComposite();
    await act(async () => modelTrigger().click());
    expect(flyout()).toBeNull();
    expect(modelTrigger().getAttribute('aria-expanded')).toBe('true');
    const inlineItems = menu().querySelectorAll('[data-acp-composite-inline="model"] [role="menuitemradio"]');
    expect(Array.from(inlineItems, (item) => item.textContent)).toContain('Model B');

    await act(async () => (inlineItems[inlineItems.length - 1] as HTMLElement).click());
    expect(onModel).toHaveBeenCalledWith('b');
    expect(menu()).not.toBeNull();

    await act(async () => modelTrigger().click());
    expect(menu().querySelector('[data-acp-composite-inline="model"]')).toBeNull();
  });

  it('expands with Enter and collapses with ArrowLeft when the flyout cannot fit', async () => {
    boundaryRight = 480;
    await openComposite();
    const press = (key: string) => act(async () => {
      modelTrigger().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    });
    await press('Enter');
    expect(flyout()).toBeNull();
    expect(menu().querySelector('[data-acp-composite-inline="model"]')).not.toBeNull();
    await press('ArrowLeft');
    expect(menu().querySelector('[data-acp-composite-inline="model"]')).toBeNull();
    expect(flyout()).toBeNull();
  });

  it('keeps the side flyout when the conversation pane has room beside the menu', async () => {
    boundaryRight = 2000;
    await openComposite();
    await act(async () => modelTrigger().click());
    expect(flyout()?.textContent).toContain('Model B');
    expect(menu().querySelector('[data-acp-composite-inline]')).toBeNull();
  });
});

describe('overlayFitsBeside', () => {
  const boundaryRect = { left: 0, right: 1000 };
  it('fits when either side leaves room for the overlay plus offset and padding', () => {
    expect(overlayFitsBeside({ left: 100, right: 400 }, boundaryRect, 352, 6, 8)).toBe(true);
    expect(overlayFitsBeside({ left: 600, right: 900 }, boundaryRect, 352, 6, 8)).toBe(true);
  });
  it('does not fit when both sides are too narrow', () => {
    expect(overlayFitsBeside({ left: 300, right: 700 }, boundaryRect, 352, 6, 8)).toBe(false);
  });
});
