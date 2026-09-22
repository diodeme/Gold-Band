import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useRightWorkspaceCommands, type RightWorkspaceResourceTransitionReason } from '../right-workspace-context';
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

export async function resolveBrowserResourceTransition(reason: RightWorkspaceResourceTransitionReason) {
  if (reason === 'scope-change') return true;
  const { browserWebviewHost } = await import('./browser-webview-host');
  if (reason === 'close') await browserWebviewHost.discardAll();
  else await browserWebviewHost.suppress();
  return true;
}

export function BrowserNativeLifecycle({
  scopeKey,
  presented,
  autoCollapsedHidden,
  available,
  requestedOpen,
  activeIsBrowser,
}: {
  scopeKey: string | null;
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
        void import('./browser-webview-host').then(({ browserWebviewHost }) => browserWebviewHost.suppress());
      });
    };
  }, []);
  useEffect(() => {
    if (activeIsBrowser) browserWasPresentedRef.current = true;
    if (!browserWasPresentedRef.current) return;
    const generation = ++visibilityGenerationRef.current;
    // scopeKey 只参与重新求值：切换会话时可见页并未变化，不得据此 hide / show。
    const suppressed = !available
      || !requestedOpen
      || autoCollapsedHidden
      || !presented
      || !activeIsBrowser;
    void import('./browser-webview-host').then(async ({ browserWebviewHost }) => {
      if (visibilityGenerationRef.current !== generation) return;
      if (suppressed) {
        await browserWebviewHost.suppress();
        return;
      }
      browserWebviewHost.resume();
      if (browserWebviewHost.hasBlockingOverlay()) await browserWebviewHost.hideAll();
    });
  }, [activeIsBrowser, autoCollapsedHidden, available, presented, requestedOpen, scopeKey]);
  return null;
}
