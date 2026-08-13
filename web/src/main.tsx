import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './i18n';
import { installUiErrorDiagnostics, logUiErrorDiagnostic, shouldLogUiError } from '@/lib/ui-error-diagnostics';
import { disposeAcpComposerDrafts } from '@/lib/acp-composer-draft';
import { installDesktopPageZoomGuard } from '@/lib/desktop-page-zoom';
import '@xyflow/react/dist/style.css';
import './styles.css';

installUiErrorDiagnostics();
const disposeDesktopPageZoomGuard = installDesktopPageZoomGuard();
window.addEventListener('pagehide', () => {
  disposeDesktopPageZoomGuard();
  disposeAcpComposerDrafts();
});

createRoot(document.getElementById('root') as HTMLElement, {
  onUncaughtError(error, errorInfo) {
    logUiErrorDiagnostic(error, {
      componentStack: errorInfo.componentStack || null,
    });
    if (!shouldLogUiError(error)) console.error(error);
  },
}).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
