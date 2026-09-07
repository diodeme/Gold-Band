import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import { statusBadgeClass, toneSurfaceClass } from '../src/lib/status';
import { builtinThemes } from '../src/themes/builtin-themes';

const compatibilityStyles = readFileSync(
  new URL('../src/webview-compatibility.css', import.meta.url),
  'utf8',
);

const fallbackSurfaces = {
  running: { value: 'running', surface: '--gb-status-running-surface', border: '--gb-status-running-border' },
  success: { value: 'success', surface: '--gb-status-success-surface', border: '--gb-status-success-border' },
  warning: { value: 'warning', surface: '--gb-status-warning-surface', border: '--gb-status-warning-border' },
  danger: { value: 'error', surface: '--gb-status-danger-surface', border: '--gb-status-danger-border' },
} as const;

describe('WebView compatible status surfaces', () => {
  it.each(Object.entries(fallbackSurfaces))(
    'provides a theme-owned %s surface without color-mix',
    (tone, fallback) => {
      const marker = `webview-status-surface-${tone}`;

      expect(statusBadgeClass(fallback.value)).toContain(marker);
      expect(toneSurfaceClass(fallback.value)).toContain(marker);

      const selector = `:root[data-webview-theme-rendering='fallback-tokens'] .${marker}`;
      const start = compatibilityStyles.indexOf(selector);
      const end = compatibilityStyles.indexOf('}', start);
      expect(start, `${tone} fallback selector`).toBeGreaterThanOrEqual(0);

      const rule = compatibilityStyles.slice(start, end + 1);
      expect(rule).toContain(`background-color: var(${fallback.surface})`);
      expect(rule).toContain(`border-color: var(${fallback.border})`);
      expect(rule).not.toContain('color-mix(');
    },
  );

  it('requires every theme and color scheme to own the compatible status palette', () => {
    const requiredTokens = Object.keys(fallbackSurfaces).flatMap((tone) => [
      `status${tone[0].toUpperCase()}${tone.slice(1)}Surface`,
      `status${tone[0].toUpperCase()}${tone.slice(1)}Border`,
    ]);

    for (const theme of builtinThemes) {
      for (const scheme of ['light', 'dark'] as const) {
        for (const token of requiredTokens) {
          expect(theme.schemes[scheme].semantic, `${theme.id}/${scheme} is missing ${token}`)
            .toHaveProperty(token);
        }
      }
    }
  });
});
