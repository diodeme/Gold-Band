import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { AppProviders } from '@/components/AppProviders';
import './i18n';
import { installUiErrorDiagnostics, shouldLogUiError } from '@/lib/ui-error-diagnostics';
import '@/lib/acp-streaming-diagnostics';
import { disposeAcpComposerDrafts } from '@/lib/acp-composer-draft';
import { installDesktopPageZoomGuard } from '@/lib/desktop-page-zoom';
import '@xyflow/react/dist/style.css';
import './styles.css';

const uiErrorDiagnostics = installUiErrorDiagnostics();
const disposeDesktopPageZoomGuard = installDesktopPageZoomGuard();
window.addEventListener('pagehide', () => {
  uiErrorDiagnostics?.dispose();
  disposeDesktopPageZoomGuard();
  disposeAcpComposerDrafts();
});

createRoot(document.getElementById('root') as HTMLElement, {
  onUncaughtError(error, errorInfo) {
    uiErrorDiagnostics?.report('react-uncaught', error, {
      componentStack: errorInfo.componentStack || null,
    });
    if (!shouldLogUiError(error)) console.error(error);
  },
}).render(
  <React.StrictMode>
    <AppProviders><App /></AppProviders>
  </React.StrictMode>,
);
