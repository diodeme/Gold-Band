import { useEffect, useRef } from 'react';
import { setDesktopPageZoomDriver } from '@/lib/desktop-page-zoom';
import {
  INTERFACE_ZOOM_DEFAULT,
  clampInterfaceZoomPercent,
  normalizeInterfaceZoomPercent,
  previewInterfaceZoomPercent,
} from '@/lib/desktop-zoom';
import { getRuntimeApi } from '@/api/client';
import type { PreferencesVm } from '@/types';

/** Ctrl+wheel fires many events per second; persist at most this often. */
const ZOOM_PERSIST_DEBOUNCE_MS = 600;

/**
 * Bridges the native page-zoom gestures (intercepted by
 * `installDesktopPageZoomGuard`) to the owned interface zoom preference:
 * each step applies immediately, persistence is debounced through the
 * single-field zoom save (`save_interface_zoom_percent`), which changes one
 * field of the canonical personalization instead of round-tripping a possibly
 * stale full preference object.
 *
 * `enabled` gates stepping until the persisted preferences have hydrated:
 * before the bootstrap IPC lands the frontend only holds startup defaults,
 * so a step would jump the webview off the persisted zoom (backend applied it
 * before first paint) and persist a value computed from a base the user
 * never saw.
 */
export function useDesktopZoomShortcuts(
  preferences: PreferencesVm,
  onPersistZoomPercent: (percent: number) => void,
  enabled = true,
) {
  const appliedPercentRef = useRef(preferences.personalization.interfaceZoomPercent);
  const persistTimerRef = useRef<number | null>(null);
  // The last percent this hook itself committed — its persisted echo must
  // not cancel a newer pending step.
  const selfCommitRef = useRef<number | null>(null);
  const latestRef = useRef({ onPersistZoomPercent });
  latestRef.current = { onPersistZoomPercent };

  // The applied marker follows persisted changes (settings page commit); in
  // between, it advances locally so consecutive shortcut steps accumulate.
  // An external commit also cancels any pending shortcut debounce — the newest
  // persisted value must never be overwritten by a stale step target. A
  // cancelled step had already previewed a different percent to the webview,
  // so the adopted value is re-applied to keep display and preference in sync.
  // The hook's own commit echo is exempt: while a newer step is pending, the
  // echo of the older commit must not cancel it (the webview would visibly
  // snap back to the older value and the newer step would be lost).
  useEffect(() => {
    const next = preferences.personalization.interfaceZoomPercent;
    const pendingStepTimer = persistTimerRef.current;
    if (pendingStepTimer !== null && next === selfCommitRef.current) return;
    appliedPercentRef.current = next;
    if (pendingStepTimer !== null) {
      window.clearTimeout(pendingStepTimer);
      persistTimerRef.current = null;
      // Clamp-only re-apply: a manually committed percent keeps its exact
      // integer (the manual-input contract); step snapping here would desync
      // the webview from the persisted preference by one step.
      void getRuntimeApi().applyDesktopZoom(clampInterfaceZoomPercent(next));
    }
  }, [preferences.personalization.interfaceZoomPercent]);

  useEffect(() => () => {
    if (persistTimerRef.current !== null) window.clearTimeout(persistTimerRef.current);
  }, []);

  useEffect(() => {
    const persist = (target: number) => {
      persistTimerRef.current = null;
      selfCommitRef.current = target;
      latestRef.current.onPersistZoomPercent(target);
    };
    const schedulePersist = (target: number, immediate: boolean) => {
      if (persistTimerRef.current !== null) {
        window.clearTimeout(persistTimerRef.current);
        persistTimerRef.current = null;
      }
      if (immediate) {
        persist(target);
        return;
      }
      persistTimerRef.current = window.setTimeout(() => persist(target), ZOOM_PERSIST_DEBOUNCE_MS);
    };
    const apply = (target: number, immediate: boolean) => {
      if (!enabled) return;
      const next = normalizeInterfaceZoomPercent(target);
      if (next === appliedPercentRef.current && !immediate) return;
      appliedPercentRef.current = next;
      void previewInterfaceZoomPercent(next);
      schedulePersist(next, immediate);
    };

    setDesktopPageZoomDriver({
      adjust: (deltaPercent) => apply(appliedPercentRef.current + deltaPercent, false),
      reset: () => apply(INTERFACE_ZOOM_DEFAULT, true),
    });
    return () => setDesktopPageZoomDriver(null);
  }, [enabled]);
}
