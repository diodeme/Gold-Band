import { describe, expect, it } from 'vitest';
import { getThemePackage, themePackageSummaries } from '../src/theme';
import type { ThemePackage } from '../src/theme-contract';

/** WCAG relative luminance for a hex color. */
function luminance(hex: string): number {
  const channels = [0, 2, 4].map((offset) => {
    const raw = Number.parseInt(hex.slice(offset + 1, offset + 3), 16) / 255;
    return raw <= 0.03928 ? raw / 12.92 : ((raw + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(foreground: string, background: string): number {
  const light = luminance(foreground);
  const dark = luminance(background);
  return (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05);
}

/** Foreground/background pairs that carry reading text in each scheme. */
const AA_PAIRS: ReadonlyArray<readonly [string, string]> = [
  ['foreground', 'background'],
  ['foreground', 'card'],
  ['cardForeground', 'card'],
  ['popoverForeground', 'popover'],
  ['mutedForeground', 'background'],
  ['mutedForeground', 'card'],
  ['mutedForeground', 'muted'],
  ['primaryForeground', 'primary'],
  ['secondaryForeground', 'secondary'],
  ['accentForeground', 'accent'],
  ['selectionForeground', 'selection'],
  ['messageUserForeground', 'messageUser'],
  ['messageAssistantForeground', 'conversationBackground'],
  ['conversationForeground', 'conversationBackground'],
  ['contentHeaderForeground', 'contentHeader'],
  ['composerForeground', 'composer'],
  ['activityForeground', 'activity'],
  ['toolCardForeground', 'toolCard'],
  ['permissionCardForeground', 'permissionCard'],
  ['workspaceTabForeground', 'workspaceTab'],
  ['resourceHeaderForeground', 'resourceHeader'],
  ['fileTreeForeground', 'fileTree'],
  ['sidebarForeground', 'sidebar'],
  ['editorForeground', 'editor'],
  ['diffAddedForeground', 'editor'],
  ['diffRemovedForeground', 'editor'],
  ['diffModifiedForeground', 'editor'],
  ['link', 'background'],
  ['link', 'card'],
  ['running', 'background'],
  ['success', 'background'],
  ['warning', 'background'],
  ['danger', 'background'],
  ['titlebarForeground', 'titlebar'],
];

describe('builtin.meadow theme package', () => {
  it('ships the localized eye-care theme with spring wallpapers', () => {
    const meadow = getThemePackage('builtin.meadow');
    expect(meadow.name).toEqual({ 'zh-CN': '春野', en: 'Meadow' });
    expect(meadow.capabilities).toEqual(['tokens', 'component-recipes', 'avatars', 'wallpapers']);
    expect(meadow.fonts).toBeUndefined();
    expect(meadow.visualQualityProfiles).toBeUndefined();
    expect(meadow.wallpapers?.light.conversation.assetId).toBe('meadow-kite-light');
    expect(meadow.wallpapers?.dark.conversation.assetId).toBe('meadow-bamboo-dark');
    expect(meadow.assets.records.filter((asset) => asset.kind === 'wallpaper').map(({ id }) => id).sort())
      .toEqual(['meadow-bamboo-dark', 'meadow-kite-light']);
    expect(themePackageSummaries.map(({ id }) => id)).toContain('builtin.meadow');
  });

  it('anchors each scheme on the spring-meadow palette', () => {
    const meadow = getThemePackage('builtin.meadow');

    // Light: a spring-green canvas with real eye-care luminance; floating
    // surfaces stay near-white so the palette reads fresh, not inky.
    expect(meadow.schemes.light.semantic).toMatchObject({
      background: '#E4EEDF', primary: '#358050', primaryForeground: '#FFFFFF',
      link: '#38618F', danger: '#973A31', popover: '#F5F9F2',
    });
    // Dark: soft pine night with a sprout primary and bright white-green
    // text; blue-suppressed except links.
    expect(meadow.schemes.dark.semantic).toMatchObject({
      background: '#202624', primary: '#7FC494', primaryForeground: '#12251A',
      link: '#90B2D6', danger: '#DBA49E', popover: '#2B3330',
    });
    expect(meadow.schemes.light.material.model).toBe('solid');
    expect(meadow.schemes.dark.material.model).toBe('solid');
  });

  it('keeps every reading surface at WCAG AA in both schemes', () => {
    const meadow = getThemePackage('builtin.meadow');
    for (const scheme of ['light', 'dark'] as const) {
      const tokens: ThemePackage['schemes']['light']['semantic'] = meadow.schemes[scheme].semantic;
      for (const [foreground, background] of AA_PAIRS) {
        const fg = tokens[foreground as keyof typeof tokens];
        const bg = tokens[background as keyof typeof tokens];
        expect(fg, `${scheme} ${foreground}`).toMatch(/^#[0-9a-f]{6}$/i);
        expect(bg, `${scheme} ${background}`).toMatch(/^#[0-9a-f]{6}$/i);
        const ratio = contrast(fg, bg);
        expect(
          ratio,
          `${scheme} ${foreground} on ${background} must reach WCAG AA (4.5:1)`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('lifts the code paper above the conversation canvas for shiki palettes', () => {
    const meadow = getThemePackage('builtin.meadow');
    const light = meadow.schemes.light.semantic;
    const dark = meadow.schemes.dark.semantic;

    // The code paper must stay near-white (shiki github-light assumes a bright
    // canvas) and a perceptibly brighter step than the near-white canvas.
    expect(luminance(light.editor)).toBeGreaterThanOrEqual(0.9);
    expect(luminance(light.editor) - luminance(light.conversationBackground))
      .toBeGreaterThanOrEqual(0.08);
    // In the dark scheme the paper is one step brighter than the night canvas.
    expect(luminance(dark.editor) - luminance(dark.conversationBackground))
      .toBeGreaterThanOrEqual(0.02);
  });

  it('keeps the light canvas in the eye-care luminance band with perceptible green', () => {
    const meadow = getThemePackage('builtin.meadow');
    const canvas = meadow.schemes.light.semantic.background;
    const canvasLuminance = luminance(canvas);

    // Eye care physically means less incoming light than white...
    expect(canvasLuminance).toBeLessThanOrEqual(0.86);
    // ...but not the inky bean-paste depth the team rejected.
    expect(canvasLuminance).toBeGreaterThanOrEqual(0.80);
    // The green must be perceptible (G above B and R) yet unsaturated.
    const [r, g, b] = [1, 3, 5].map((offset) => Number.parseInt(canvas.slice(offset, offset + 2), 16));
    expect(g - b).toBeGreaterThanOrEqual(12);
    expect(g - r).toBeGreaterThanOrEqual(6);
    // Floating surfaces stay near-white so the theme still reads spring-fresh.
    expect(luminance(meadow.schemes.light.semantic.popover)).toBeGreaterThanOrEqual(0.88);
  });
});
