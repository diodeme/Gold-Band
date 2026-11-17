/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  COMPOSER_INPUT_AUTO_DROP_PX,
  COMPOSER_INPUT_HEIGHT_STORAGE_KEY,
  COMPOSER_INPUT_KEYBOARD_STEP_PX,
  COMPOSER_INPUT_MIN_HEIGHT_PX,
  clampComposerInputHeight,
  readStoredComposerInputHeight,
  storeComposerInputHeight,
} from '../src/lib/composer-resize';
import { useComposerResize, type ComposerResizeController } from '../src/hooks/use-composer-resize';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const setViewportHeight = (height: number) => {
  window.innerHeight = height;
};

function ResizeProbe({ controller, getBaseHeight }: {
  controller: { read: () => ComposerResizeController };
  getBaseHeight: () => number;
}) {
  const resize = useComposerResize({ getBaseHeight });
  controller.read = () => resize;
  return (
    <div
      data-testid="probe"
      onPointerDown={resize.gripPointerDown}
      onKeyDown={resize.gripKeyDown}
      data-manual={resize.manualHeight ?? ''}
      data-drag={resize.dragHeight ?? ''}
      data-style={JSON.stringify(resize.inputContainerStyle)}
      data-max-height={String(resize.promptInputMaxHeight)}
    />
  );
}

function firePointerEvent(target: HTMLElement, type: 'pointermove' | 'pointerup', init: { pointerId: number; clientY?: number }) {
  const listeners = (target as unknown as { __listeners?: Map<string, Array<(e: unknown) => void>> }).__listeners;
  const handler = listeners?.get(type)?.at(-1);
  expect(handler, `expected a ${type} listener`).toBeTypeOf('function');
  act(() => {
    handler?.({ pointerId: init.pointerId, clientY: init.clientY ?? 0 });
  });
}

describe('composer manual resize', () => {
  let host: HTMLDivElement;
  let root: Root;
  let probe: HTMLElement;
  let controller: { read: () => ComposerResizeController };

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    window.innerHeight = 768;
    window.localStorage.clear();
  });

  const mount = (getBaseHeight: () => number) => {
    controller = { read: () => (undefined as unknown as ComposerResizeController) };
    act(() => {
      root.render(<ResizeProbe controller={controller} getBaseHeight={getBaseHeight} />);
    });
    probe = host.querySelector<HTMLElement>('[data-testid="probe"]')!;
    // Route addEventListener into a capture map the test can dispatch through.
    const listeners = new Map<string, Array<(e: unknown) => void>>();
    (probe as unknown as { __listeners: Map<string, Array<(e: unknown) => void>> }).__listeners = listeners;
    probe.setPointerCapture = vi.fn();
    probe.releasePointerCapture = vi.fn();
    probe.hasPointerCapture = vi.fn(() => true);
    const nativeAdd = probe.addEventListener.bind(probe);
    probe.addEventListener = ((type: string, listener: EventListener) => {
      const bucket = listeners.get(type) ?? [];
      bucket.push(listener as (e: unknown) => void);
      listeners.set(type, bucket);
      nativeAdd(type, listener);
    }) as typeof probe.addEventListener;
  };

  const dragFrom = (startY: number, pointerId = 7) => {
    act(() => {
      // jsdom has no PointerEvent constructor; a MouseEvent with the pointerId
      // property attached carries everything the grip handler reads.
      const down = new MouseEvent('pointerdown', { button: 0, clientY: startY, bubbles: true });
      Object.defineProperty(down, 'pointerId', { value: pointerId });
      probe.dispatchEvent(down);
    });
    return pointerId;
  };

  it('clamps into the band between the floor and 40% of the viewport', () => {
    setViewportHeight(1000);
    expect(clampComposerInputHeight(20)).toBe(COMPOSER_INPUT_MIN_HEIGHT_PX);
    expect(clampComposerInputHeight(2000)).toBe(400);
    expect(clampComposerInputHeight(240)).toBe(240);
    setViewportHeight(500);
    expect(clampComposerInputHeight(240)).toBe(200);
  });

  it('persists a pinned height and forgets it when returning to autosize', () => {
    setViewportHeight(1000);
    storeComposerInputHeight(180);
    expect(window.localStorage.getItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY)).toBe('180');
    expect(readStoredComposerInputHeight()).toBe(180);
    storeComposerInputHeight(null);
    expect(window.localStorage.getItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY)).toBeNull();
    expect(readStoredComposerInputHeight()).toBeNull();
  });

  it('drops stored heights at or below the auto threshold back to autosize', () => {
    setViewportHeight(1000);
    window.localStorage.setItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY, String(COMPOSER_INPUT_AUTO_DROP_PX));
    expect(readStoredComposerInputHeight()).toBeNull();
  });

  it('drags from the current content height and commits above the drop threshold', () => {
    setViewportHeight(1000);
    mount(() => 96);
    expect(controller.read().manualHeight).toBeNull();
    expect(controller.read().promptInputMaxHeight).toBeUndefined();
    expect(controller.read().inputContainerStyle).toBeNull();

    const pointerId = dragFrom(500);
    firePointerEvent(probe, 'pointermove', { pointerId, clientY: 400 });
    expect(controller.read().inputContainerStyle).toEqual({ height: 196, maxHeight: 196 });
    expect(controller.read().promptInputMaxHeight).toBeNull();

    firePointerEvent(probe, 'pointerup', { pointerId });
    expect(controller.read().manualHeight).toBe(196);
    expect(controller.read().dragHeight).toBeNull();
    expect(window.localStorage.getItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY)).toBe('196');
  });

  it('releases below the drop threshold back to autosize', () => {
    setViewportHeight(1000);
    mount(() => 96);
    const pointerId = dragFrom(500);
    firePointerEvent(probe, 'pointermove', { pointerId, clientY: 560 });
    firePointerEvent(probe, 'pointerup', { pointerId });
    expect(controller.read().manualHeight).toBeNull();
    expect(window.localStorage.getItem(COMPOSER_INPUT_HEIGHT_STORAGE_KEY)).toBeNull();
  });

  it('nudges the pinned height with the keyboard and stops at autosize', () => {
    setViewportHeight(1000);
    mount(() => 96);
    act(() => {
      probe.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    });
    expect(controller.read().manualHeight).toBe(96 + COMPOSER_INPUT_KEYBOARD_STEP_PX);
    act(() => {
      probe.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    });
    expect(controller.read().manualHeight).toBe(96);
    // 72 still sits above the 64px drop threshold and stays pinned.
    act(() => {
      probe.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    });
    expect(controller.read().manualHeight).toBe(72);
    // Stepping below the threshold returns to autosize.
    act(() => {
      probe.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    });
    expect(controller.read().manualHeight).toBeNull();
  });
});
