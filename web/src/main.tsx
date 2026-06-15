import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './i18n';
import { installUiErrorDiagnostics, logUiErrorDiagnostic, shouldLogUiError } from '@/lib/ui-error-diagnostics';
import '@xyflow/react/dist/style.css';
import './styles.css';

installUiErrorDiagnostics();

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
