import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { getThemePackage, themePackageSummaries } from '../src/theme';

const generatedThemeStyles = () => readFileSync(
  path.resolve(__dirname, '../src/themes/generated/builtin-themes.css'),
  'utf8',
);

describe('builtin.vista theme package', () => {
  it('ships the localized scenic theme with both color schemes', () => {
    const vista = getThemePackage('builtin.vista');
    expect(vista.name).toEqual({ 'zh-CN': '画境', en: 'Vista' });
    expect(vista.capabilities).toEqual(
      expect.arrayContaining(['tokens', 'component-recipes', 'wallpapers', 'visual-quality-profiles']),
    );
    // Typography inherits the app's canonical font registration instead of shipping font assets.
    expect(vista.capabilities).not.toContain('fonts');
    expect(vista.fonts).toBeUndefined();
  });

  it('anchors each scheme on the two-scene palette, free of logo colors', () => {
    const vista = getThemePackage('builtin.vista');

    // Dark scheme: near-black canvas with a golden-amber primary (snow summit night scene).
    expect(vista.schemes.dark.semantic).toMatchObject({
      background: '#0f0e10', primary: '#E8A54A', primaryForeground: '#221407',
      ring: '#E8A54A', link: '#8aa3e8', danger: '#e0564c', popover: '#1a191d',
    });
    // Light scheme: warm paper with a teal-slate primary (ink-wash scene) and a
    // desaturated brick danger — the logo cobalt/vermilion stay reserved for
    // a future brand theme.
    expect(vista.schemes.light.semantic).toMatchObject({
      background: '#faf8f4', primary: '#2F6157', primaryForeground: '#ffffff',
      ring: '#2F6157', link: '#2F6157', danger: '#B0483F', popover: '#ffffff',
    });
    expect(vista.schemes.dark.material.model).toBe('solid');
    expect(vista.schemes.light.material.model).toBe('solid');
  });

  it('declares symmetric wallpapers for both schemes and mutes them in performance mode', () => {
    const vista = getThemePackage('builtin.vista');

    for (const scheme of ['light', 'dark'] as const) {
      const schemeWallpapers = vista.wallpapers?.[scheme] ?? {};
      expect(Object.keys(schemeWallpapers).sort()).toEqual(['conversation', 'settings']);
      for (const descriptor of Object.values(schemeWallpapers)) {
        expect(descriptor.fit).toBe('cover');
        expect(descriptor.overlayColor).toBe('background');
      }
    }
    expect(vista.wallpapers?.light.conversation.assetId).toBe('vista-inkwash-light');
    expect(vista.wallpapers?.dark.conversation.assetId).toBe('vista-summit-dark');

    expect(vista.visualQualityProfiles?.performance.wallpapers).toEqual({ enabled: false });
    expect(generatedThemeStyles()).toContain(
      ":root[data-theme='builtin.vista'][data-visual-quality='performance']",
    );
  });

  it('registers the wallpaper assets with runtime URLs under the theme directory', () => {
    const vista = getThemePackage('builtin.vista');
    const wallpapers = vista.assets.records.filter((asset) => asset.kind === 'wallpaper');
    expect(wallpapers.map(({ id }) => id).sort()).toEqual(['vista-inkwash-light', 'vista-summit-dark']);
    for (const asset of wallpapers) {
      expect(asset.outputUrl).toMatch(/^\/theme-assets\/builtin\.vista\//u);
      expect(asset.mediaType).toBe('image/webp');
      expect(asset.width).toBe(2048);
      expect(asset.height).toBe(1280);
    }
  });

  it('keeps the theme selectable from the shared summaries', () => {
    expect(themePackageSummaries.map(({ id }) => id)).toContain('builtin.vista');
  });
});
