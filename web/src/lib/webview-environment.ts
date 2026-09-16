import {
  detectWebviewCapabilities,
  developmentWebviewCapabilityOverride,
  type WebviewCapabilities,
} from './webview-capabilities';
import {
  resolveWebviewFeaturePolicy,
  type WebviewFeaturePolicy,
} from './webview-feature-policy';

export interface WebviewEnvironmentSnapshot {
  readonly capabilities: WebviewCapabilities;
  readonly policy: WebviewFeaturePolicy;
}

let currentSnapshot: WebviewEnvironmentSnapshot | null = null;

export function createWebviewEnvironmentSnapshot(
  capabilities: WebviewCapabilities,
): WebviewEnvironmentSnapshot {
  return Object.freeze({
    capabilities,
    policy: resolveWebviewFeaturePolicy(capabilities),
  });
}

export function initializeWebviewEnvironment(
  capabilities = developmentWebviewCapabilityOverride(window.location.search) ?? detectWebviewCapabilities(),
) {
  if (!currentSnapshot) currentSnapshot = createWebviewEnvironmentSnapshot(capabilities);
  return currentSnapshot;
}

export function getWebviewEnvironment() {
  return currentSnapshot ?? initializeWebviewEnvironment();
}

export function applyWebviewEnvironmentToDocument(
  snapshot: WebviewEnvironmentSnapshot,
  root: HTMLElement = document.documentElement,
) {
  root.dataset.webviewTier = snapshot.policy.tier;
  root.dataset.webviewThemeRendering = snapshot.policy.themeRendering;
  root.dataset.webviewResponsiveLayout = snapshot.policy.responsiveLayout;
  root.dataset.webviewCodeHighlighting = snapshot.policy.codeHighlighting;
  root.dataset.webviewVisualMaterial = snapshot.policy.visualMaterial;
}

export function resetWebviewEnvironmentForTests() {
  currentSnapshot = null;
}
