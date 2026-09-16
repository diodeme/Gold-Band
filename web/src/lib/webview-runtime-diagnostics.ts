import { invoke } from '@tauri-apps/api/core';
import type { WebviewEnvironmentSnapshot } from './webview-environment';

export interface WebviewRuntimeFacts {
  readonly platform: string;
  readonly architecture: string;
  readonly osVersion: string | null;
  readonly webkitBundleVersion: string | null;
}

export async function reportWebviewEnvironment(snapshot: WebviewEnvironmentSnapshot) {
  if (!('__TAURI_INTERNALS__' in window)) return null;
  return invoke<WebviewRuntimeFacts>('report_webview_environment', {
    input: {
      userAgent: navigator.userAgent,
      capabilities: snapshot.capabilities,
      policy: snapshot.policy,
    },
  }).catch(() => null);
}
