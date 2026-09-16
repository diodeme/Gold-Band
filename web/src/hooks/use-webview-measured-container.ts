import { useCallback, useEffect, useState } from 'react';
import { getWebviewEnvironment } from '@/lib/webview-environment';
import { observeMeasuredWebviewContainer } from '@/lib/webview-measured-container';

export function useWebviewMeasuredContainer<T extends HTMLElement>(name: string) {
  const [element, setElement] = useState<T | null>(null);
  const ref = useCallback((nextElement: T | null) => setElement(nextElement), []);

  useEffect(() => {
    if (!element) return undefined;
    element.dataset.webviewContainer = name;
    if (getWebviewEnvironment().policy.responsiveLayout !== 'measured') {
      return () => { delete element.dataset.webviewContainer; };
    }
    return observeMeasuredWebviewContainer(element, name);
  }, [element, name]);

  return ref;
}
