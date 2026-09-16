import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useRightWorkspaceCommands } from '../right-workspace-context';
import { openWebTarget } from './open-web-target';

export function useOpenWebTarget() {
  const { t } = useTranslation();
  const workspace = useRightWorkspaceCommands();
  return (href: string) => openWebTarget(href, {
    projectId: workspace.projectId,
    scopeKey: workspace.scopeKey,
    openResource: workspace.openResource,
    browserTitle: t('workspace.browser.title'),
  });
}

export function BrowserNativeLifecycle({
  presented,
  autoCollapsedHidden,
  available,
  requestedOpen,
  activeIsBrowser,
}: {
  presented: boolean;
  autoCollapsedHidden: boolean;
  available: boolean;
  requestedOpen: boolean;
  activeIsBrowser: boolean;
}) {
  const browserWasPresentedRef = useRef(false);
  const ownerGenerationRef = useRef(0);
  const visibilityGenerationRef = useRef(0);
  useEffect(() => {
    ownerGenerationRef.current += 1;
    return () => {
      const cleanupGeneration = ++ownerGenerationRef.current;
      queueMicrotask(() => {
        if (
          ownerGenerationRef.current !== cleanupGeneration
          || !browserWasPresentedRef.current
        ) return;
        void import('./browser-webview-host').then(({ browserWebviewHost }) => (
          browserWebviewHost.discardAll()
        ));
      });
    };
  }, []);
  useEffect(() => {
    if (activeIsBrowser) browserWasPresentedRef.current = true;
    if (!browserWasPresentedRef.current) return;
    const generation = ++visibilityGenerationRef.current;
    if (!available || !requestedOpen) {
      queueMicrotask(() => {
        if (visibilityGenerationRef.current !== generation) return;
        void import('./browser-webview-host').then(({ browserWebviewHost }) => {
          if (visibilityGenerationRef.current !== generation) return;
          void browserWebviewHost.discardAll();
        });
      });
      return;
    }
    void import('./browser-webview-host').then(({ browserWebviewHost }) => {
      if (visibilityGenerationRef.current !== generation) return;
      if (autoCollapsedHidden || !presented || !activeIsBrowser || browserWebviewHost.hasBlockingOverlay()) {
        void browserWebviewHost.hideAll();
      }
    });
  }, [activeIsBrowser, autoCollapsedHidden, available, presented, requestedOpen]);
  return null;
}
