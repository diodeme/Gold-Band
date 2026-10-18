import { useEffect, useState, type RefObject } from 'react';

/**
 * Becomes true once the element comes within `margin` of the visible area and
 * stays true. `scrollMargin` extends nested scroll containers' clip, so the
 * margin also applies inside the chat viewport, not only the window.
 */
export function useNearViewport(ref: RefObject<Element | null>, margin: string) {
  const [near, setNear] = useState(() => typeof IntersectionObserver === 'undefined');
  useEffect(() => {
    const element = ref.current;
    if (near || !element || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setNear(true);
    }, { rootMargin: margin, scrollMargin: margin } as IntersectionObserverInit);
    observer.observe(element);
    return () => observer.disconnect();
  }, [margin, near, ref]);
  return near;
}
