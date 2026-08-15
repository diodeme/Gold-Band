import { afterEach, describe, expect, it } from 'vitest';
import { browserApi } from '../src/api/browser';
import { browserPreviewState } from '../src/api/browserState';
import type { AppearancePreference, PersonalizationPreference } from '../src/types';

describe('theme preference API contract', () => {
  const original = browserPreviewState.getPreferences();

  afterEach(() => {
    browserPreviewState.setPreferences(original);
  });

  it('returns the canonical complete preference after saving theme, scheme, and quality together', async () => {
    const appearance: AppearancePreference = {
      schemaVersion: 2,
      themeId: 'builtin.tech-neutral',
      colorScheme: 'dark',
      visualQualityByTheme: {},
    };

    const personalization: PersonalizationPreference = {
      ...original.personalization,
      typography: {
        ...original.personalization.typography,
        ui: {
          font: { source: 'local', family: 'Microsoft YaHei UI' },
          fontSize: { source: 'custom', px: 16 },
        },
      },
    };

    const saved = await browserApi.saveDesktopPreferences(
      appearance,
      personalization,
      original.language,
      original.useLocalClaude,
      original.verboseLogging,
    );

    expect(saved.appearance).toEqual(appearance);
    expect(browserPreviewState.getPreferences().appearance).toEqual(appearance);
    expect(saved.appearance).not.toBe(appearance);
    expect(saved.appearance.visualQualityByTheme).not.toBe(appearance.visualQualityByTheme);
    expect(saved.personalization).toEqual(personalization);
    expect(saved.personalization).not.toBe(personalization);
    expect(saved.personalization.typography.ui).not.toBe(personalization.typography.ui);
  });

  it('roundtrips the default theme without manufacturing quality preferences', async () => {
    const saved = await browserApi.saveDesktopPreferences(
      {
        schemaVersion: 2,
        themeId: 'builtin.gold-band',
        colorScheme: 'system',
        visualQualityByTheme: {},
      },
      original.personalization,
      original.language,
      original.useLocalClaude,
      original.verboseLogging,
    );

    expect(saved.appearance.themeId).toBe('builtin.gold-band');
    expect(saved.appearance.visualQualityByTheme).toEqual({});
  });
});
