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
  readonly cssCustomProperties: boolean;
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

const CSS_CUSTOM_PROPERTY_PROBE_NAME = '--gold-band-capability-probe';
const CSS_CUSTOM_PROPERTY_PROBE_VALUE = 'rgb(1, 2, 3)';
const CSS_CUSTOM_PROPERTY_PROBE_FALLBACK = 'rgb(4, 5, 6)';

function semanticallySupportsCssCustomProperties() {
  if (
    typeof document === 'undefined'
    || typeof document.createElement !== 'function'
    || typeof getComputedStyle !== 'function'
  ) {
    return false;
  }

  const mount = document.body ?? document.documentElement;
  if (!mount) return false;

  const host = document.createElement('div');
  const target = document.createElement('span');
  const expected = document.createElement('span');
  host.setAttribute('aria-hidden', 'true');
  host.style.cssText = 'position:absolute;width:0;height:0;overflow:hidden;visibility:hidden;pointer-events:none;';
  host.style.setProperty(CSS_CUSTOM_PROPERTY_PROBE_NAME, CSS_CUSTOM_PROPERTY_PROBE_VALUE);
  target.style.color = `var(${CSS_CUSTOM_PROPERTY_PROBE_NAME}, ${CSS_CUSTOM_PROPERTY_PROBE_FALLBACK})`;
  expected.style.color = CSS_CUSTOM_PROPERTY_PROBE_VALUE;
  host.appendChild(target);
  host.appendChild(expected);

  try {
    mount.appendChild(host);
    const actualColor = getComputedStyle(target).color;
    const expectedColor = getComputedStyle(expected).color;
    return actualColor.length > 0 && actualColor === expectedColor;
  } finally {
    host.parentNode?.removeChild(host);
  }
}

export function browserWebviewCapabilityEnvironment(): WebviewCapabilityProbeEnvironment {
  return {
    createRegExp: (source, flags) => new RegExp(source, flags),
    cssSupports: typeof CSS !== 'undefined' && typeof CSS.supports === 'function'
      ? CSS.supports.bind(CSS)
      : null,
    cssCustomProperties: safelyProbe(semanticallySupportsCssCustomProperties),
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
    cssCustomProperties: environment.cssCustomProperties,
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
