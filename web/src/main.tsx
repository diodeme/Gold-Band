import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { AppProviders } from '@/components/AppProviders';
import { DetachedApp } from '@/components/terminal/DetachedApp';
import { parseDetachedRoute } from '@/lib/detached';
import './i18n';
import { installUiErrorDiagnostics, shouldLogUiError } from '@/lib/ui-error-diagnostics';
import '@/lib/acp-streaming-diagnostics';
import { disposeAcpComposerDrafts } from '@/lib/acp-composer-draft';
import { installDesktopPageZoomGuard } from '@/lib/desktop-page-zoom';
import '@xyflow/react/dist/style.css';
import './styles.css';
import './webview-compatibility.css';

const uiErrorDiagnostics = installUiErrorDiagnostics();
const disposeDesktopPageZoomGuard = installDesktopPageZoomGuard();
window.addEventListener('pagehide', () => {
  uiErrorDiagnostics?.dispose();
  disposeDesktopPageZoomGuard();
  disposeAcpComposerDrafts();
});

// A pop-out "detached" webview window (created by `open_detached_window`)
// renders an isolated tree instead of the full app shell.
const detachedRoute = parseDetachedRoute();

createRoot(document.getElementById('root') as HTMLElement, {
  onUncaughtError(error, errorInfo) {
    uiErrorDiagnostics?.report('react-uncaught', error, {
      componentStack: errorInfo.componentStack || null,
    });
    if (!shouldLogUiError(error)) console.error(error);
  },
}).render(
  <React.StrictMode>
    {detachedRoute.isDetached && detachedRoute.kind ? (
      <AppProviders>
        <DetachedApp kind={detachedRoute.kind} sessionId={detachedRoute.sessionId} />
      </AppProviders>
    ) : (
      <AppProviders><App /></AppProviders>
    )}
  </React.StrictMode>,
);
