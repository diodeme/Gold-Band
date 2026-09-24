import { useLayoutEffect, useRef } from 'react';
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

type BrowserHostModule = typeof import('./browser-webview-host');

let browserHostModule: Promise<BrowserHostModule> | null = null;
let browserHostModuleSync: BrowserHostModule | null = null;

function loadBrowserHost() {
  browserHostModule ??= import('./browser-webview-host').then((module) => {
    browserHostModuleSync = module;
    return module;
  });
  return browserHostModule;
}

function loadedBrowserHost() {
  return browserHostModuleSync?.browserWebviewHost ?? null;
}

function suppressBrowserHost() {
  const host = loadedBrowserHost();
  if (host) {
    void host.suppress();
    return;
  }
  void loadBrowserHost().then(({ browserWebviewHost }) => browserWebviewHost.suppress());
}

function applyBrowserVisibility(host: BrowserHostModule['browserWebviewHost'], suppressed: boolean) {
  if (suppressed) {
    void host.suppress();
    return;
  }
  host.resume();
  if (host.hasBlockingOverlay()) void host.hideAll();
}

export function notifyBrowserLayoutFrame() {
  if (browserHostModule == null) return;
  void browserHostModule.then(({ browserWebviewHost }) => {
    browserWebviewHost.notifyLayoutFrame();
  });
}

export async function resolveBrowserResourceTransition(reason: RightWorkspaceResourceTransitionReason) {
  if (reason === 'scope-change') return true;
  const { browserWebviewHost } = await loadBrowserHost();
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
  useLayoutEffect(() => {
    ownerGenerationRef.current += 1;
    return () => {
      const cleanupGeneration = ++ownerGenerationRef.current;
      // 由 layout cleanup 登记，微任务在绘制前执行；同一提交里的 StrictMode 重挂载会使 generation 失效。
      queueMicrotask(() => {
        if (
          ownerGenerationRef.current !== cleanupGeneration
          || !browserWasPresentedRef.current
        ) return;
        suppressBrowserHost();
      });
    };
  }, []);
  useLayoutEffect(() => {
    if (activeIsBrowser) browserWasPresentedRef.current = true;
    if (!browserWasPresentedRef.current) return;
    const generation = ++visibilityGenerationRef.current;
    // scopeKey 只参与重新求值：切换会话时可见页并未变化，不得据此 hide / show。
    const suppressed = !available
      || !requestedOpen
      || autoCollapsedHidden
      || !presented
      || !activeIsBrowser;
    const host = loadedBrowserHost();
    if (host) {
      applyBrowserVisibility(host, suppressed);
      return;
    }
    void loadBrowserHost().then(({ browserWebviewHost }) => {
      if (visibilityGenerationRef.current !== generation) return;
      applyBrowserVisibility(browserWebviewHost, suppressed);
    });
  }, [activeIsBrowser, autoCollapsedHidden, available, presented, requestedOpen, scopeKey]);
  return null;
}
