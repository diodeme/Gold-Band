import { useLayoutEffect, useMemo, useRef, useState } from 'react';

import {
  calculateSkillAgentOverflowLayout,
  type SkillAgentOverflowLayout,
} from '@/lib/skill-agent-overflow';

const unmeasuredLayout: SkillAgentOverflowLayout = {
  visibleSourceCount: Number.POSITIVE_INFINITY,
  visibleSyncCount: Number.POSITIVE_INFINITY,
  hiddenCount: 0,
};

export function useTwoRowAgentOverflow(sourceCount: number, syncCount: number) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [availableWidth, setAvailableWidth] = useState<number | null>(null);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const updateWidth = () => {
      const nextWidth = Math.floor(container.getBoundingClientRect().width || container.clientWidth);
      setAvailableWidth((current) => current === nextWidth ? current : nextWidth);
    };
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const layout = useMemo(() => availableWidth === null
    ? unmeasuredLayout
    : calculateSkillAgentOverflowLayout(availableWidth, sourceCount, syncCount),
  [availableWidth, sourceCount, syncCount]);

  return { containerRef, layout };
}
