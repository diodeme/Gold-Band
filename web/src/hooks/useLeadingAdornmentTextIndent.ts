import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';

const LEADING_ADORNMENT_GAP = '0.25rem';

export function useLeadingAdornmentTextIndent(enabled: boolean) {
  const adornmentRef = useRef<HTMLSpanElement>(null);
  const [adornmentWidth, setAdornmentWidth] = useState(0);

  useLayoutEffect(() => {
    if (!enabled || !adornmentRef.current) return undefined;
    const adornment = adornmentRef.current;
    const updateWidth = () => setAdornmentWidth(adornment.getBoundingClientRect().width);
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(adornment);
    return () => observer.disconnect();
  }, [enabled]);

  const textareaStyle: CSSProperties | undefined = enabled && adornmentWidth > 0
    ? { textIndent: `calc(${adornmentWidth}px + ${LEADING_ADORNMENT_GAP})` }
    : undefined;

  return { adornmentRef, textareaStyle };
}
