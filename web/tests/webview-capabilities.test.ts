import { describe, expect, it } from 'vitest';
import {
  detectWebviewCapabilities,
  fullWebviewCapabilityProfile,
  unsupportedWebviewCapabilityProfile,
  webkit613CapabilityProfile,
  type WebviewCapabilityProbeEnvironment,
} from '@/lib/webview-capabilities';
import { resolveWebviewFeaturePolicy } from '@/lib/webview-feature-policy';

function probeEnvironment(options: {
  lookbehind?: boolean;
  supportedCss?: readonly string[];
  cssCustomProperties?: boolean;
} = {}): WebviewCapabilityProbeEnvironment {
  const supportedCss = new Set(options.supportedCss ?? []);
  return {
    createRegExp(source, flags) {
      if (!options.lookbehind && source.includes('?<=')) throw new SyntaxError('lookbehind unsupported');
      return new RegExp(source, flags);
    },
    cssSupports(property, value) {
      return supportedCss.has(value === undefined ? property : `${property}:${value}`);
    },
    cssCustomProperties: options.cssCustomProperties
      ?? supportedCss.has('--gold-band-capability-probe:1'),
    resizeObserver: true,
    structuredClone: true,
    webAssembly: true,
  };
}

describe('WebView capabilities', () => {
  it('turns probe exceptions into immutable negative capabilities', () => {
    const capabilities = detectWebviewCapabilities(probeEnvironment({
      supportedCss: [
        'selector(:has(*))',
        'color:oklch(50% 0.1 90)',
        'display:grid',
        '--gold-band-capability-probe:1',
        '-webkit-backdrop-filter:blur(1px)',
      ],
    }));

    expect(capabilities).toMatchObject(webkit613CapabilityProfile);
    expect(Object.isFrozen(capabilities)).toBe(true);
  });

  it('classifies the Monterey WebKit 613 profile as compatible', () => {
    expect(resolveWebviewFeaturePolicy(webkit613CapabilityProfile)).toEqual({
      tier: 'compatible',
      themeRendering: 'fallback-tokens',
      responsiveLayout: 'measured',
      codeHighlighting: 'wasm',
      visualMaterial: 'solid',
    });
  });

  it('uses the semantic custom-property probe when CSS.supports reports a false negative', () => {
    const capabilities = detectWebviewCapabilities(probeEnvironment({
      cssCustomProperties: true,
      supportedCss: [
        'selector(:has(*))',
        'color:oklch(50% 0.1 90)',
        'display:grid',
        '-webkit-backdrop-filter:blur(1px)',
      ],
    }));

    expect(capabilities.cssCustomProperties).toBe(true);
    expect(resolveWebviewFeaturePolicy(capabilities).tier).toBe('compatible');
  });

  it('classifies the complete profile as full and freezes the policy', () => {
    const policy = resolveWebviewFeaturePolicy(fullWebviewCapabilityProfile);
    expect(policy).toEqual({
      tier: 'full',
      themeRendering: 'modern-css',
      responsiveLayout: 'container-query',
      codeHighlighting: 'wasm',
      visualMaterial: 'native',
    });
    expect(Object.isFrozen(policy)).toBe(true);
  });

  it('rejects a profile missing the minimum semantic CSS and platform APIs', () => {
    expect(resolveWebviewFeaturePolicy(unsupportedWebviewCapabilityProfile).tier).toBe('unsupported');
  });
});
