import { useEffect, useRef, useState } from 'react';
import {
  MERMAID_ERROR_CODES,
  acquireMermaidDiagram,
  isMermaidDiagramError,
  mermaidDiagramKey,
  peekMermaidDiagram,
  type MermaidDiagramAsset,
  type MermaidDiagramError,
} from '@/lib/mermaid-diagram';
import { useMermaidTheme, type MermaidTheme } from '@/lib/mermaid-theme';

export type MermaidDiagramView =
  | { status: 'idle' | 'loading' }
  | { status: 'ready'; asset: MermaidDiagramAsset }
  | { status: 'error'; error: MermaidDiagramError };

type KeyedView = MermaidDiagramView & { key: string };

function toDiagramError(error: unknown): MermaidDiagramError {
  return isMermaidDiagramError(error) ? error : { code: MERMAID_ERROR_CODES.renderFailed, params: {} };
}

function initialView(key: string, source: string, theme: MermaidTheme, enabled: boolean): KeyedView {
  const asset = peekMermaidDiagram(source, theme);
  if (asset) return { key, status: 'ready', asset };
  return { key, status: enabled ? 'loading' : 'idle' };
}

/**
 * Leases the rendered diagram for `source` in the current Gold Band theme. A cached
 * diagram is ready on the first render, so remounts keep their final size. After a
 * theme change the previous diagram stays visible until its replacement is ready.
 */
export function useMermaidDiagram(source: string, enabled: boolean): MermaidDiagramView {
  const theme = useMermaidTheme();
  const key = mermaidDiagramKey(source, theme);
  const [state, setState] = useState<KeyedView>(() => initialView(key, source, theme, enabled));
  const retainedReleaseRef = useRef<(() => void) | null>(null);

  useEffect(() => () => {
    retainedReleaseRef.current?.();
    retainedReleaseRef.current = null;
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let lease: ReturnType<typeof acquireMermaidDiagram>;
    try {
      lease = acquireMermaidDiagram(source, theme);
    } catch (error) {
      setState({ key, status: 'error', error: toDiagramError(error) });
      return;
    }
    lease.promise.then(
      (asset) => {
        if (!active) {
          lease.release();
          return;
        }
        // The displayed URL stays leased until its replacement is on screen.
        const previousRelease = retainedReleaseRef.current;
        retainedReleaseRef.current = lease.release;
        previousRelease?.();
        setState({ key, status: 'ready', asset });
      },
      (error: unknown) => {
        lease.release();
        if (active) setState({ key, status: 'error', error: toDiagramError(error) });
      },
    );
    return () => {
      active = false;
    };
  }, [enabled, key, theme, source]);

  if (state.key === key) return state;
  const fresh = initialView(key, source, theme, enabled);
  return fresh.status !== 'ready' && state.status === 'ready' ? state : fresh;
}
