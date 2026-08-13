import { useLayoutEffect, useRef, useState } from 'react';
import {
  reduceFileWorkspaceResponsiveState,
  resolveFileWorkspaceResizeDirection,
  type FileWorkspaceResizeDirection,
  type FileWorkspaceResponsiveState,
} from './workspace-layout';

const INITIAL_RESPONSIVE_STATE: FileWorkspaceResponsiveState = {
  split: false,
  widthAtTransition: 0,
};

export function useWorkspaceResponsiveState(splitMinWidth: number) {
  const ref = useRef<HTMLDivElement>(null);
  const widthRef = useRef(0);
  const shellWidthRef = useRef(0);
  const shellResizeDirectionRef = useRef<FileWorkspaceResizeDirection>('stationary');
  const shellResizeObservedAtRef = useRef(0);
  const resizeFrameRef = useRef<number | null>(null);
  const responsiveStateRef = useRef(INITIAL_RESPONSIVE_STATE);
  const [responsiveState, setResponsiveState] = useState(INITIAL_RESPONSIVE_STATE);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => {
      const width = Math.round(element.clientWidth);
      const shellWidth = Math.round(element.ownerDocument.documentElement.clientWidth);
      const now = Date.now();
      const shellWidthChanged = shellWidthRef.current > 0 && shellWidth !== shellWidthRef.current;
      const direction = resolveFileWorkspaceResizeDirection({
        previousShellWidth: shellWidthRef.current,
        shellWidth,
        previousDirection: shellResizeDirectionRef.current,
        elapsedSinceShellResizeMs: now - shellResizeObservedAtRef.current,
      });
      widthRef.current = width;
      shellWidthRef.current = shellWidth;
      shellResizeDirectionRef.current = direction;
      if (shellWidthChanged) shellResizeObservedAtRef.current = now;
      const next = reduceFileWorkspaceResponsiveState(responsiveStateRef.current, width, splitMinWidth, direction);
      if (next === responsiveStateRef.current) return;
      responsiveStateRef.current = next;
      setResponsiveState(next);
    };
    const scheduleUpdate = () => {
      if (resizeFrameRef.current !== null) return;
      resizeFrameRef.current = requestAnimationFrame(() => {
        resizeFrameRef.current = null;
        update();
      });
    };
    update();
    const observer = new ResizeObserver(scheduleUpdate);
    observer.observe(element);
    return () => {
      observer.disconnect();
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
      resizeFrameRef.current = null;
    };
  }, [splitMinWidth]);
  return {
    ref,
    responsiveState,
    currentWidth: () => widthRef.current || Math.round(ref.current?.clientWidth ?? 0),
  };
}
