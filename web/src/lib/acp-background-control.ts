import { useEffect, useState } from 'react';
import type { DirectBackgroundControl } from '@/types';

/**
 * The single "is the retained Direct background busy" judgment. The backend
 * only reports facts (running tools, grace-window expiry); every consumer —
 * Stop control, live activity group, sidebar indicators — must decide here.
 */
export function isDirectBackgroundActive(control: DirectBackgroundControl | null | undefined, now: number) {
  return Boolean(control && (control.activeTools > 0 || control.expiresAtMs > now));
}

/** Re-evaluates {@link isDirectBackgroundActive} when the grace window expires. Visibility only. */
export function useDirectBackgroundActive(control: DirectBackgroundControl | null | undefined, scope: string) {
  const [, refresh] = useState(0);
  useEffect(() => {
    if (!control || control.activeTools > 0) return;
    const remaining = control.expiresAtMs - Date.now();
    if (remaining <= 0) return;
    const timer = window.setTimeout(() => refresh((value) => value + 1), remaining);
    return () => window.clearTimeout(timer);
  }, [control?.activeTools, control?.expiresAtMs, control?.sessionId, control?.connectionGeneration, scope]);
  return isDirectBackgroundActive(control, Date.now());
}
