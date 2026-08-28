import type { WebviewCapabilities } from './webview-capabilities';

export type WebviewSupportTier = 'unsupported' | 'compatible' | 'full';

export interface WebviewFeaturePolicy {
  readonly tier: WebviewSupportTier;
  readonly themeRendering: 'fallback-tokens' | 'modern-css';
  readonly responsiveLayout: 'measured' | 'container-query';
  readonly codeHighlighting: 'plain' | 'wasm';
  readonly visualMaterial: 'solid' | 'native';
}

const CORE_CAPABILITIES = [
  'cssHasSelector',
  'cssOklch',
  'cssGrid',
  'cssCustomProperties',
  'resizeObserver',
  'structuredClone',
] as const satisfies readonly (keyof WebviewCapabilities)[];

const FULL_CAPABILITIES = [
  'regexpLookbehind',
  'cssColorMix',
  'cssContainerQueries',
  'cssBackdropFilter',
  'webAssembly',
] as const satisfies readonly (keyof WebviewCapabilities)[];

export function missingCoreWebviewCapabilities(capabilities: WebviewCapabilities) {
  return CORE_CAPABILITIES.filter((capability) => !capabilities[capability]);
}

export function resolveWebviewFeaturePolicy(capabilities: WebviewCapabilities): WebviewFeaturePolicy {
  const coreAvailable = missingCoreWebviewCapabilities(capabilities).length === 0;
  const fullAvailable = coreAvailable && FULL_CAPABILITIES.every((capability) => capabilities[capability]);
  const tier: WebviewSupportTier = !coreAvailable ? 'unsupported' : fullAvailable ? 'full' : 'compatible';
  return Object.freeze({
    tier,
    themeRendering: capabilities.cssColorMix ? 'modern-css' : 'fallback-tokens',
    responsiveLayout: capabilities.cssContainerQueries ? 'container-query' : 'measured',
    codeHighlighting: capabilities.webAssembly ? 'wasm' : 'plain',
    visualMaterial: capabilities.cssBackdropFilter && capabilities.cssColorMix ? 'native' : 'solid',
  });
}
