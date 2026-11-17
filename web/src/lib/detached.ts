import type { DetachedWindowKind } from '@/types';

export interface DetachedWindowRoute {
  isDetached: boolean;
  kind: DetachedWindowKind | null;
  sessionId: string | null;
}

/** The backend expects labels of the form `detached-<kind>[-<sessionId>]`. */
const DETACHED_PREFIX = 'detached-';
const DETACHED_TERMINAL_PREFIX = `${DETACHED_PREFIX}terminal-`;

/** Extracts the session id from a detached terminal window label, or null. */
export function detachedTerminalSessionId(label: string): string | null {
  if (!label.startsWith(DETACHED_TERMINAL_PREFIX)) return null;
  return label.slice(DETACHED_TERMINAL_PREFIX.length) || null;
}

/** True when a label denotes a detached window (regardless of kind). */
export function isDetachedLabel(label: string): boolean {
  return label.startsWith(DETACHED_PREFIX);
}

/**
 * Reads `window.location.search` to decide whether this webview is a pop-out
 * "detached" window (loaded by the backend with `?window=terminal&session=<id>`
 * or `?window=connections`), and if so which resource it hosts. In the main
 * window this returns `{ isDetached: false, kind: null, sessionId: null }`.
 */
export function parseDetachedRoute(): DetachedWindowRoute {
  const { search } = window.location;
  if (!search) return { isDetached: false, kind: null, sessionId: null };

  const params = new URLSearchParams(search);
  const windowParam = params.get('window');
  if (windowParam !== 'terminal' && windowParam !== 'connections') {
    return { isDetached: false, kind: null, sessionId: null };
  }

  return {
    isDetached: true,
    kind: windowParam,
    sessionId: params.get('session'),
  };
}
