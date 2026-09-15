/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ChatContainerRoot, ChatContainerContent, type ChatContainerContext } from '@/components/prompt-kit/chat-container';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

function pointer(type: string, clientX = 295, pointerId = 1) {
  const event = new MouseEvent(type, { bubbles: true, clientX, clientY: 60, button: 0 });
  Object.defineProperties(event, {
    pointerId: { value: pointerId }, pointerType: { value: 'mouse' },
  });
  return event;
}

async function mount() {
  vi.stubGlobal('ResizeObserver', class {
    observe() {} unobserve() {} disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (id: number) => window.clearTimeout(id));
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  const context = React.createRef<ChatContainerContext>();
  const changes = vi.fn();
  await act(async () => root.render(React.createElement(ChatContainerRoot, {
    contextRef: context, onFollowIntentChange: changes, resize: 'instant', initial: 'instant',
  }, React.createElement(ChatContainerContent, { scrollClassName: 'overflow-y-auto' },
    React.createElement('div', { 'data-detail': true, style: { overflowY: 'auto' } }, 'tool output'),
    React.createElement('textarea', { 'aria-label': 'answer' }),
  ))));
  const viewport = context.current!.scrollRef.current!;
  let top = 199;
  Object.defineProperties(viewport, {
    clientHeight: { configurable: true, value: 100 },
    scrollHeight: { configurable: true, value: 300 },
    clientWidth: { configurable: true, value: 284 },
    offsetWidth: { configurable: true, value: 300 },
    clientLeft: { configurable: true, value: 0 },
    scrollTop: { configurable: true, get: () => top, set: (value: number) => { top = value; } },
  });
  vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue({
    left: 0, right: 300, top: 0, bottom: 100, width: 300, height: 100,
  } as DOMRect);
  await act(async () => viewport.dispatchEvent(new Event('scroll')));
  const detail = host.querySelector<HTMLElement>('[data-detail]')!;
  Object.defineProperties(detail, {
    clientHeight: { configurable: true, value: 40 },
    scrollHeight: { configurable: true, value: 200 },
  });
  detail.scrollTop = 60;
  return { root, host, context, changes, viewport, detail };
}

describe('chat scroll input ownership', () => {
  it.each([
    ['ltr', 5, true], ['ltr', 295, false],
    ['rtl', 5, false], ['rtl', 295, true],
  ] as const)('distinguishes the %s scrollbar from a mirrored gutter at x=%s', async (direction, x, follows) => {
    const { root, viewport, context } = await mount();
    try {
      viewport.style.direction = direction;
      Object.defineProperties(viewport, {
        clientLeft: { configurable: true, value: 16 },
        clientWidth: { configurable: true, value: 268 },
      });
      await act(async () => {
        viewport.dispatchEvent(pointer('pointerdown', x));
        viewport.scrollTop = 150;
        viewport.dispatchEvent(new Event('scroll'));
      });
      expect(context.current!.isAtBottom).toBe(follows);
    } finally { await act(async () => root.unmount()); }
  });

  it.each(['blank', 'blur', 'cancel', 'release'] as const)(
    'keeps following a layout displacement after %s', async (scenario) => {
      const { root, context, viewport, changes } = await mount();
      try {
        await act(async () => {
          viewport.dispatchEvent(pointer('pointerdown', scenario === 'blank' ? 50 : 295));
          if (scenario === 'blur') window.dispatchEvent(new Event('blur'));
          if (scenario === 'cancel') window.dispatchEvent(pointer('pointercancel'));
          if (scenario === 'release') window.dispatchEvent(pointer('pointerup'));
          viewport.scrollTop = 150;
          viewport.dispatchEvent(new Event('scroll'));
        });
        expect(changes.mock.calls.filter(([following]) => !following)).toEqual([]);
        await act(async () => { await new Promise(resolve => window.setTimeout(resolve, 40)); });
        expect(context.current!.isAtBottom).toBe(true);
        expect(viewport.scrollTop).toBe(199);
      } finally { await act(async () => root.unmount()); }
    },
  );

  it('preserves a real scrollbar reading position and ignores another pointer release', async () => {
    const { root, context, viewport } = await mount();
    try {
      await act(async () => {
        viewport.dispatchEvent(pointer('pointerdown'));
        window.dispatchEvent(pointer('pointerup', 295, 2));
        viewport.scrollTop = 100;
        viewport.dispatchEvent(new Event('scroll'));
      });
      expect(context.current!.isAtBottom).toBe(false);
      expect(viewport.scrollTop).toBe(100);
      await act(async () => {
        viewport.scrollTop = 199;
        viewport.dispatchEvent(new Event('scroll'));
        viewport.dispatchEvent(new Event('scrollend'));
        window.dispatchEvent(pointer('pointerup'));
      });
      expect(context.current!.isAtBottom).toBe(true);
    } finally { await act(async () => root.unmount()); }
  });

  it.each(['wheel', 'key', 'contained-edge', 'editable', 'zoom', 'prevented'] as const)(
    'does not detach the conversation for %s input owned elsewhere', async (scenario) => {
      const { root, context, viewport, detail, host, changes } = await mount();
      try {
        if (scenario === 'contained-edge') {
          detail.scrollTop = 0;
          detail.style.overscrollBehaviorY = 'contain';
        }
        await act(async () => {
          if (scenario === 'editable') {
            host.querySelector('textarea')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }));
          } else if (scenario === 'key') {
            detail.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageUp', bubbles: true }));
          } else {
            const event = new WheelEvent('wheel', { deltaY: -20, bubbles: true, cancelable: true, ctrlKey: scenario === 'zoom' });
            if (scenario === 'prevented') event.preventDefault();
            (scenario === 'zoom' ? viewport : detail).dispatchEvent(event);
          }
        });
        expect(context.current!.isAtBottom).toBe(true);
        expect(changes.mock.calls.filter(([following]) => !following)).toEqual([]);
        expect(viewport.scrollTop).toBe(199);
      } finally { await act(async () => root.unmount()); }
    },
  );

  it('detaches when an inner wheel reaches an uncontained edge', async () => {
    const { root, context, detail } = await mount();
    try {
      detail.scrollTop = 0;
      await act(async () => detail.dispatchEvent(new WheelEvent('wheel', { deltaY: -20, bubbles: true })));
      expect(context.current!.isAtBottom).toBe(false);
    } finally { await act(async () => root.unmount()); }
  });
});
