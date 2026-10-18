import { useCallback, useRef, useState, type FocusEvent } from 'react';

export function isOverflowing(element: HTMLElement | null) {
  return element !== null && element.scrollWidth > element.clientWidth + 1;
}

/**
 * Let Radix own opening, timing and dismissal; only gate whether the value needs a tooltip.
 * Filter restored pointer focus before Radix starts opening, not in its open callback.
 */
export function useOverflowTooltip<TElement extends HTMLElement = HTMLSpanElement>({
  always = false,
}: { always?: boolean } = {}) {
  const valueRef = useRef<TElement>(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  const handleTooltipFocus = useCallback((event: FocusEvent<HTMLElement>) => {
    if (!event.currentTarget.matches(':focus-visible')) event.preventDefault();
  }, []);
  const hideTooltip = useCallback(() => setTooltipOpen(false), []);
  const handleTooltipOpenChange = useCallback((open: boolean) => {
    setTooltipOpen(open && (always || isOverflowing(valueRef.current)));
  }, [always]);

  return {
    valueRef,
    tooltipOpen,
    handleTooltipFocus,
    hideTooltip,
    handleTooltipOpenChange,
  };
}
