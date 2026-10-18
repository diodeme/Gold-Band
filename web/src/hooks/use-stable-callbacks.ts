import { useLayoutEffect, useRef, useState } from 'react';

/**
 * Returns an object whose functions keep one identity for the component's lifetime and always
 * call the latest handlers. Lets memoized children receive commands without re-rendering when the
 * parent re-renders for unrelated state. Keys must stay the same between renders; a handler that
 * is `undefined` stays callable as a no-op, so expose availability as a separate prop.
 */
export function useStableCallbacks<T extends Record<keyof T, ((...args: never[]) => unknown) | undefined>>(handlers: T): { [K in keyof T]-?: NonNullable<T[K]> } {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });
  const [stable] = useState(() => {
    const result: Record<string, (...args: never[]) => unknown> = {};
    for (const key of Object.keys(handlers)) {
      result[key] = (...args: never[]) => (latest.current[key as keyof T] as ((...a: never[]) => unknown) | undefined)?.(...args);
    }
    return result;
  });
  return stable as { [K in keyof T]-?: NonNullable<T[K]> };
}
