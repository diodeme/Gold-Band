import { useCallback, useRef, useState } from 'react';

export function isOverflowing(element: HTMLElement | null) {
  return element !== null && element.scrollWidth > element.clientWidth + 1;
}

/** Measure only when the user asks for the complete value via hover or focus. */
export function useOverflowTooltip<TElement extends HTMLElement = HTMLSpanElement>() {
  const valueRef = useRef<TElement>(null);
  const [tooltipOpen, setTooltipOpen] = useState(false);

  const showTooltipIfOverflowing = useCallback(() => {
    setTooltipOpen(isOverflowing(valueRef.current));
  }, []);
  const hideTooltip = useCallback(() => setTooltipOpen(false), []);
  const handleTooltipOpenChange = useCallback((open: boolean) => {
    setTooltipOpen(open && isOverflowing(valueRef.current));
  }, []);

  return {
    valueRef,
    tooltipOpen,
    showTooltipIfOverflowing,
    hideTooltip,
    handleTooltipOpenChange,
  };
}
