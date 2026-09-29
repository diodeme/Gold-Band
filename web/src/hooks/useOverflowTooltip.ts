import { useCallback, useRef, useState, type FocusEvent } from 'react';

export function isOverflowing(element: HTMLElement | null) {
  return element !== null && element.scrollWidth > element.clientWidth + 1;
}

/**
 * Measure only when the user asks for the complete value via hover or keyboard focus.
 *
 * The hook owns every open decision: Radix also opens on any trigger focus, including the
 * programmatic focus restored after a menu closes, so its open requests are ignored and only
 * its dismissals (Escape, click, blur, scroll) are honored.
 */
export function useOverflowTooltip<TElement extends HTMLElement = HTMLSpanElement>({
  always = false,
}: { always?: boolean } = {}) {
  const valueRef = useRef<TElement>(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  const showTooltipIfOverflowing = useCallback(() => {
    setTooltipOpen(always || isOverflowing(valueRef.current));
  }, [always]);
  const showTooltipOnKeyboardFocus = useCallback((event: FocusEvent<HTMLElement>) => {
    if (event.currentTarget.matches(':focus-visible')) showTooltipIfOverflowing();
  }, [showTooltipIfOverflowing]);
  const hideTooltip = useCallback(() => setTooltipOpen(false), []);
  const handleTooltipOpenChange = useCallback((open: boolean) => {
    if (!open) setTooltipOpen(false);
  }, []);

  return {
    valueRef,
    tooltipOpen,
    showTooltipIfOverflowing,
    showTooltipOnKeyboardFocus,
    hideTooltip,
    handleTooltipOpenChange,
  };
}
