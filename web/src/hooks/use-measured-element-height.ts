import { useEffect, useRef, useState } from 'react';

export type ElementHeightMeasure = (element: HTMLDivElement) => number;

const clientHeight: ElementHeightMeasure = (element) => element.clientHeight;

export function useMeasuredElementHeight(
  initialHeight = 1,
  measure: ElementHeightMeasure = clientHeight,
) {
  const ref = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(initialHeight);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => {
      const nextHeight = Math.max(1, Math.floor(measure(element)));
      setHeight((currentHeight) => currentHeight === nextHeight ? currentHeight : nextHeight);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [measure]);

  return { ref, height };
}
