import { useCallback, useEffect, useRef, useState } from 'react';
import {
  COMPOSER_INPUT_AUTO_DROP_PX,
  COMPOSER_INPUT_KEYBOARD_STEP_PX,
  clampComposerInputHeight,
  readStoredComposerInputHeight,
  storeComposerInputHeight,
} from '@/lib/composer-resize';

export interface UseComposerResizeOptions {
  /** Returns the input scroll container's current rendered height; used as the
   * drag origin when resizing from autosize so the first drag step does not
   * snap the box down to the minimum. */
  getBaseHeight: () => number;
}

export interface ComposerResizeController {
  /** Pinned input height in px, or null while in autosize mode. */
  manualHeight: number | null;
  /** Live height during a drag (px); rendered instead of manualHeight. */
  dragHeight: number | null;
  /** Style for the input scroll container: fixed height while pinned/dragging. */
  inputContainerStyle: { height: number; maxHeight: number } | null;
  /** `null` keeps each page's default autosize cap; uncapped while pinned so
   * the fixed-height scroll container owns scrolling. */
  promptInputMaxHeight: number | null | undefined;
  isResizing: boolean;
  gripPointerDown: (event: React.PointerEvent<HTMLElement>) => void;
  gripKeyDown: (event: React.KeyboardEvent<HTMLElement>) => void;
}

export function useComposerResize({ getBaseHeight }: UseComposerResizeOptions): ComposerResizeController {
  const [manualHeight, setManualHeight] = useState<number | null>(() => readStoredComposerInputHeight());
  const [dragHeight, setDragHeight] = useState<number | null>(null);
  const dragStateRef = useRef<{ pointerId: number; startY: number; startHeight: number } | null>(null);

  // Re-clamp a stored height when the viewport shrinks below it.
  useEffect(() => {
    if (manualHeight === null) return;
    const clamped = clampComposerInputHeight(manualHeight);
    if (clamped !== manualHeight) setManualHeight(clamped);
  }, [manualHeight]);

  const commit = useCallback((next: number | null) => {
    setManualHeight(next);
    storeComposerInputHeight(next);
  }, []);

  const gripPointerDown = useCallback((event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    const target = event.currentTarget;
    dragStateRef.current = {
      pointerId: event.pointerId,
      startY: event.clientY,
      startHeight: manualHeight ?? Math.max(COMPOSER_INPUT_AUTO_DROP_PX + 1, getBaseHeight()),
    };
    target.setPointerCapture(event.pointerId);
    setDragHeight(dragStateRef.current.startHeight);

    const onMove = (moveEvent: PointerEvent) => {
      const drag = dragStateRef.current;
      if (!drag || moveEvent.pointerId !== drag.pointerId) return;
      setDragHeight(clampComposerInputHeight(drag.startHeight + drag.startY - moveEvent.clientY));
    };
    const finish = (upEvent: PointerEvent) => {
      const drag = dragStateRef.current;
      if (!drag || upEvent.pointerId !== drag.pointerId) return;
      dragStateRef.current = null;
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', finish);
      target.removeEventListener('pointercancel', finish);
      if (target.hasPointerCapture?.(drag.pointerId)) target.releasePointerCapture(drag.pointerId);
      setDragHeight((live) => {
        // Releasing at or below the drop threshold returns to autosize.
        commit(live !== null && live > COMPOSER_INPUT_AUTO_DROP_PX ? live : null);
        return null;
      });
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', finish);
    target.addEventListener('pointercancel', finish);
    event.preventDefault();
  }, [commit, getBaseHeight, manualHeight]);

  const gripKeyDown = useCallback((event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    event.preventDefault();
    const current = manualHeight ?? Math.max(COMPOSER_INPUT_AUTO_DROP_PX + 1, getBaseHeight());
    const delta = event.key === 'ArrowUp' ? COMPOSER_INPUT_KEYBOARD_STEP_PX : -COMPOSER_INPUT_KEYBOARD_STEP_PX;
    const next = current + delta;
    commit(next > COMPOSER_INPUT_AUTO_DROP_PX ? clampComposerInputHeight(next) : null);
  }, [commit, getBaseHeight, manualHeight]);

  const activeHeight = dragHeight ?? manualHeight;
  return {
    manualHeight,
    dragHeight,
    inputContainerStyle: activeHeight !== null
      ? { height: activeHeight, maxHeight: activeHeight }
      : null,
    promptInputMaxHeight: activeHeight !== null ? null : undefined,
    isResizing: dragHeight !== null,
    gripPointerDown,
    gripKeyDown,
  };
}
