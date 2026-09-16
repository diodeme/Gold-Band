import './webview-bootstrap.css';
import { startWebviewBootstrap } from './lib/webview-bootstrap-core';
import {
  applyWebviewEnvironmentToDocument,
  initializeWebviewEnvironment,
} from './lib/webview-environment';

const snapshot = initializeWebviewEnvironment();
applyWebviewEnvironmentToDocument(snapshot);

void import('./lib/webview-runtime-diagnostics')
  .then(({ reportWebviewEnvironment }) => reportWebviewEnvironment(snapshot))
  .catch(() => null);

void startWebviewBootstrap({
  snapshot,
  loadApp: () => import('./main'),
});
