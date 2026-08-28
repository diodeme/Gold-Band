export interface WebviewCapabilities {
  readonly regexpLookbehind: boolean;
  readonly cssColorMix: boolean;
  readonly cssContainerQueries: boolean;
  readonly cssHasSelector: boolean;
  readonly cssBackdropFilter: boolean;
  readonly cssOklch: boolean;
  readonly cssGrid: boolean;
  readonly cssCustomProperties: boolean;
  readonly resizeObserver: boolean;
  readonly structuredClone: boolean;
  readonly webAssembly: boolean;
}

export interface WebviewCapabilityProbeEnvironment {
  readonly createRegExp: (source: string, flags?: string) => RegExp;
  readonly cssSupports: ((property: string, value?: string) => boolean) | null;
  readonly resizeObserver: boolean;
  readonly structuredClone: boolean;
  readonly webAssembly: boolean;
}

function safelyProbe(probe: () => boolean) {
  try {
    return probe();
  } catch {
    return false;
  }
}

function supportsCss(
  environment: WebviewCapabilityProbeEnvironment,
  property: string,
  value: string,
) {
  return environment.cssSupports
    ? safelyProbe(() => environment.cssSupports?.(property, value) === true)
    : false;
}

function supportsCssCondition(
  environment: WebviewCapabilityProbeEnvironment,
  condition: string,
) {
  return environment.cssSupports
    ? safelyProbe(() => environment.cssSupports?.(condition) === true)
    : false;
}

export function browserWebviewCapabilityEnvironment(): WebviewCapabilityProbeEnvironment {
  return {
    createRegExp: (source, flags) => new RegExp(source, flags),
    cssSupports: typeof CSS !== 'undefined' && typeof CSS.supports === 'function'
      ? CSS.supports.bind(CSS)
      : null,
    resizeObserver: typeof ResizeObserver === 'function',
    structuredClone: typeof structuredClone === 'function',
    webAssembly: typeof WebAssembly === 'object' && typeof WebAssembly.instantiate === 'function',
  };
}

export function detectWebviewCapabilities(
  environment: WebviewCapabilityProbeEnvironment = browserWebviewCapabilityEnvironment(),
): WebviewCapabilities {
  const capabilities: WebviewCapabilities = {
    regexpLookbehind: safelyProbe(() => environment.createRegExp('(?<=gold-)band', 'u').test('gold-band')),
    cssColorMix: supportsCss(environment, 'color', 'color-mix(in srgb, black 50%, white)'),
    cssContainerQueries: supportsCss(environment, 'container-type', 'inline-size'),
    cssHasSelector: supportsCssCondition(environment, 'selector(:has(*))'),
    cssBackdropFilter: supportsCss(environment, 'backdrop-filter', 'blur(1px)')
      || supportsCss(environment, '-webkit-backdrop-filter', 'blur(1px)'),
    cssOklch: supportsCss(environment, 'color', 'oklch(50% 0.1 90)'),
    cssGrid: supportsCss(environment, 'display', 'grid'),
    cssCustomProperties: supportsCss(environment, '--gold-band-capability-probe', '1'),
    resizeObserver: environment.resizeObserver,
    structuredClone: environment.structuredClone,
    webAssembly: environment.webAssembly,
  };
  return Object.freeze(capabilities);
}

export const webkit613CapabilityProfile: WebviewCapabilities = Object.freeze({
  regexpLookbehind: false,
  cssColorMix: false,
  cssContainerQueries: false,
  cssHasSelector: true,
  cssBackdropFilter: true,
  cssOklch: true,
  cssGrid: true,
  cssCustomProperties: true,
  resizeObserver: true,
  structuredClone: true,
  webAssembly: true,
});

export const fullWebviewCapabilityProfile: WebviewCapabilities = Object.freeze({
  regexpLookbehind: true,
  cssColorMix: true,
  cssContainerQueries: true,
  cssHasSelector: true,
  cssBackdropFilter: true,
  cssOklch: true,
  cssGrid: true,
  cssCustomProperties: true,
  resizeObserver: true,
  structuredClone: true,
  webAssembly: true,
});

export const unsupportedWebviewCapabilityProfile: WebviewCapabilities = Object.freeze({
  ...webkit613CapabilityProfile,
  cssHasSelector: false,
  cssOklch: false,
  structuredClone: false,
});

export function developmentWebviewCapabilityOverride(search: string): WebviewCapabilities | null {
  if (!import.meta.env.DEV) return null;
  const profile = new URLSearchParams(search).get('webview-profile');
  if (profile === 'unsupported') return unsupportedWebviewCapabilityProfile;
  if (profile === 'monterey') return webkit613CapabilityProfile;
  if (profile === 'full') return fullWebviewCapabilityProfile;
  return null;
}
