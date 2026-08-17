import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ThemePackage } from '../src/theme-contract';

vi.mock('../src/themes/builtin-themes', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../src/themes/builtin-themes')>();
  const fixture = structuredClone(actual.builtinThemes[0]) as ThemePackage;
  fixture.capabilities = [...fixture.capabilities, 'icons', 'wallpapers', 'visual-quality-profiles'];
  fixture.assets.records.push(
    {
      id: 'icon-default', kind: 'icon', mediaType: 'image/png', bytes: 68, sha256: '1'.repeat(64),
      outputUrl: '/theme-assets/builtin.gold-band/icon-default.png', width: 16, height: 16,
      required: true, licenseId: 'fixture',
    },
    {
      id: 'icon-dark', kind: 'icon', mediaType: 'image/png', bytes: 68, sha256: '2'.repeat(64),
      outputUrl: '/theme-assets/builtin.gold-band/icon-dark.png', width: 20, height: 20,
      required: true, licenseId: 'fixture',
    },
    {
      id: 'wallpaper-light', kind: 'wallpaper', mediaType: 'image/png', bytes: 68, sha256: '3'.repeat(64),
      outputUrl: '/theme-assets/builtin.gold-band/wallpaper-light.png', width: 1, height: 1,
      required: true, licenseId: 'fixture',
    },
    {
      id: 'wallpaper-dark', kind: 'wallpaper', mediaType: 'image/png', bytes: 68, sha256: '4'.repeat(64),
      outputUrl: '/theme-assets/builtin.gold-band/wallpaper-dark.png', width: 1, height: 1,
      required: true, licenseId: 'fixture',
    },
  );
  fixture.assets.count = fixture.assets.records.length;
  fixture.assets.totalBytes = fixture.assets.records.reduce((total, asset) => total + asset.bytes, 0);
  fixture.icons = {
    defaults: {
      'navigation.search': { assetId: 'icon-default', renderMode: 'mask', nativeSize: 16, imageRendering: 'auto' },
      'navigation.agent': { assetId: 'wallpaper-light', renderMode: 'mask', nativeSize: 16, imageRendering: 'auto' },
    },
    schemes: {
      dark: {
        'navigation.search': { assetId: 'icon-dark', renderMode: 'image', nativeSize: 20, imageRendering: 'pixelated' },
      },
    },
  };
  const lightWallpaper = {
    assetId: 'wallpaper-light', fit: 'cover' as const, position: 'center' as const, repeat: 'no-repeat' as const,
    opacity: 0.8, overlayColor: 'background' as const, overlayOpacity: 0.4,
  };
  const darkWallpaper = {
    assetId: 'wallpaper-dark', fit: 'tile' as const, position: 'top-left' as const, repeat: 'repeat' as const,
    opacity: 0.7, overlayColor: 'transparent' as const, overlayOpacity: 0,
  };
  fixture.wallpapers = {
    light: {
      app: lightWallpaper,
      settings: { ...lightWallpaper, assetId: 'wallpaper-dark' },
      workspace: { ...lightWallpaper, assetId: 'icon-default' },
    },
    dark: { app: darkWallpaper },
  };
  fixture.visualQualityProfiles = {
    default: 'full',
    supported: ['full', 'performance'],
    performance: { blur: 0, saturate: 100, textureOpacity: 0, wallpapers: { enabled: false } },
  };
  return { builtinThemes: [fixture] };
});

import type { AppearancePreference } from '../src/types';
import {
  applyAppearance,
  applyWallpaperPersonalization,
  defaultAppearancePreference,
  getCurrentThemeIconSnapshot,
  previewWallpaperOpacity,
  refreshVisibleThemeWallpapers,
  resetWallpaperProjectionForTests,
  resolveAppearance,
} from '../src/theme';

const preference = (overrides: Partial<AppearancePreference> = {}): AppearancePreference => ({
  ...defaultAppearancePreference,
  colorScheme: 'light',
  ...overrides,
});

class FakeImage {
  static instances: FakeImage[] = [];
  decoding = '';
  onload: null | (() => void) = null;
  onerror: null | (() => void) = null;
  src = '';

  constructor() {
    FakeImage.instances.push(this);
  }
}

function rootStub() {
  const properties = new Map<string, string>();
  return {
    properties,
    element: {
      dataset: {} as Record<string, string>,
      classList: { toggle: vi.fn() },
      style: {
        colorScheme: '',
        setProperty: (name: string, value: string) => properties.set(name, value),
      },
    },
  };
}

function wallpaperSurface(slot = 'app') {
  const properties = new Map<string, string>();
  const removeProperty = vi.fn((name: string) => properties.delete(name));
  return {
    properties,
    removeProperty,
    element: {
      dataset: { themeWallpaperSlot: slot },
      isConnected: true,
      getClientRects: () => [{}],
      style: {
        setProperty: (name: string, value: string) => properties.set(name, value),
        removeProperty,
      },
    },
  };
}

describe('theme runtime asset projection', () => {
  beforeEach(() => {
    FakeImage.instances = [];
    resetWallpaperProjectionForTests();
    vi.stubGlobal('Image', FakeImage);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('resolves scheme overrides, rejects wrong asset kinds, and disables only wallpapers in performance mode', () => {
    const light = resolveAppearance(preference());
    expect(light.icons['navigation.search']).toMatchObject({
      url: '/theme-assets/builtin.gold-band/icon-default.png', renderMode: 'mask', nativeSize: 16,
    });
    expect(light.icons['navigation.agent']).toBeUndefined();
    expect(light.wallpapers.app?.url).toBe('/theme-assets/builtin.gold-band/wallpaper-light.png');
    expect(light.wallpapers.workspace).toBeUndefined();

    const dark = resolveAppearance(preference({ colorScheme: 'dark' }));
    expect(dark.icons['navigation.search']).toMatchObject({
      url: '/theme-assets/builtin.gold-band/icon-dark.png', renderMode: 'image', nativeSize: 20,
    });
    expect(dark.wallpapers.app?.url).toBe('/theme-assets/builtin.gold-band/wallpaper-dark.png');

    const performance = resolveAppearance(preference({
      visualQualityByTheme: { 'builtin.gold-band': 'performance' },
    }));
    expect(performance.wallpapers).toEqual({});
    expect(performance.icons).toEqual(light.icons);
    expect(performance.semantic).toBe(light.semantic);
    expect(performance.shape).toBe(light.shape);
    expect(performance.material.blur).toBe(0);
  });

  it('projects a visible wallpaper only after preload succeeds and clears it on current-load failure', () => {
    const root = rootStub();
    const surface = wallpaperSurface();
    vi.stubGlobal('document', {
      documentElement: root.element,
      querySelectorAll: () => [surface.element],
    });
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    vi.stubGlobal('CustomEvent', class { constructor(public type: string, public init: unknown) {} });

    applyAppearance(preference());
    expect(FakeImage.instances).toHaveLength(1);
    expect(surface.properties.has('--gb-wallpaper-image')).toBe(false);
    FakeImage.instances[0].onload?.();
    expect(surface.properties.get('--gb-wallpaper-image')).toBe('url("/theme-assets/builtin.gold-band/wallpaper-light.png")');
    expect(surface.properties.get('--gb-wallpaper-size')).toBe('cover');
    expect(surface.properties.get('--gb-wallpaper-overlay-color')).toBe('var(--background)');

    applyAppearance(preference({ colorScheme: 'dark' }));
    expect(surface.properties.get('--gb-wallpaper-image')).toContain('wallpaper-light.png');
    FakeImage.instances[1].onerror?.();
    expect(surface.properties.has('--gb-wallpaper-image')).toBe(false);
    expect(getCurrentThemeIconSnapshot().icons['navigation.search']?.url)
      .toBe('/theme-assets/builtin.gold-band/icon-dark.png');
  });

  it('does not let a late failure from the previous theme erase the current wallpaper', () => {
    const root = rootStub();
    const surface = wallpaperSurface();
    vi.stubGlobal('document', {
      documentElement: root.element,
      querySelectorAll: () => [surface.element],
    });
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    vi.stubGlobal('CustomEvent', class { constructor(public type: string, public init: unknown) {} });

    applyAppearance(preference());
    const oldImage = FakeImage.instances[0];
    applyAppearance(preference({ colorScheme: 'dark' }));
    const currentImage = FakeImage.instances[1];
    currentImage.onload?.();
    expect(surface.properties.get('--gb-wallpaper-image')).toContain('wallpaper-dark.png');

    oldImage.onerror?.();
    expect(
      surface.properties.get('--gb-wallpaper-image'),
      'a stale failure must not clear the current generation wallpaper',
    ).toBe('url("/theme-assets/builtin.gold-band/wallpaper-dark.png")');
  });

  it('isolates user wallpapers by color scheme while keeping the theme scrim independent', () => {
    const root = rootStub();
    const surface = wallpaperSurface();
    vi.stubGlobal('document', {
      documentElement: root.element,
      querySelectorAll: () => [surface.element],
    });
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    vi.stubGlobal('CustomEvent', class { constructor(public type: string, public init: unknown) {} });

    applyAppearance(preference());
    applyWallpaperPersonalization(
      {
        byColorScheme: {
          light: { image: { source: 'user', assetId: 'user-wallpaper-light' }, opacityPercent: 60 },
          dark: { image: { source: 'user', assetId: 'user-wallpaper-dark' }, opacityPercent: 75 },
        },
      },
      {
        recentWallpapers: [
          {
            id: 'user-wallpaper-light',
            imageUrl: 'gold-band-wallpaper://user-wallpaper-light/full',
            thumbnailUrl: 'gold-band-wallpaper://user-wallpaper-light/thumbnail',
            createdAt: '2026-08-17T00:00:00Z',
            width: 1600,
            height: 900,
          },
          {
            id: 'user-wallpaper-dark',
            imageUrl: 'gold-band-wallpaper://user-wallpaper-dark/full',
            thumbnailUrl: 'gold-band-wallpaper://user-wallpaper-dark/thumbnail',
            createdAt: '2026-08-17T00:01:00Z',
            width: 1600,
            height: 900,
          },
        ],
      },
    );
    const customImage = FakeImage.instances.at(-1)!;
    expect(customImage.src).toContain('user-wallpaper-light/full');
    customImage.onload?.();

    expect(surface.properties.get('--gb-wallpaper-image')).toContain('user-wallpaper-light/full');
    expect(surface.properties.get('--gb-wallpaper-size')).toBe('cover');
    expect(surface.properties.get('--gb-wallpaper-position')).toBe('center');
    expect(surface.properties.get('--gb-wallpaper-opacity')).toBe('0.6');
    expect(surface.properties.get('--gb-wallpaper-overlay-color')).toBe('var(--background)');
    expect(surface.properties.get('--gb-wallpaper-overlay-opacity')).toBe('0.4');

    previewWallpaperOpacity('light', 35);
    expect(surface.properties.get('--gb-wallpaper-opacity')).toBe('0.35');
    previewWallpaperOpacity('dark', 25);
    expect(surface.properties.get('--gb-wallpaper-opacity')).toBe('0.35');

    applyAppearance(preference({ colorScheme: 'dark' }));
    const darkCustomImage = FakeImage.instances.at(-1)!;
    expect(darkCustomImage.src).toContain('user-wallpaper-dark/full');
    darkCustomImage.onload?.();
    expect(surface.properties.get('--gb-wallpaper-image')).toContain('user-wallpaper-dark/full');
    expect(surface.properties.get('--gb-wallpaper-opacity')).toBe('0.75');

    applyWallpaperPersonalization(
      {
        byColorScheme: {
          light: { image: { source: 'theme' }, opacityPercent: 60 },
          dark: { image: { source: 'theme' }, opacityPercent: 60 },
        },
      },
      { recentWallpapers: [] },
    );
  });

  it('reuses a ready user wallpaper for a newly mounted surface without clearing or reloading it', () => {
    const root = rootStub();
    const conversationSurface = wallpaperSurface('conversation');
    let surfaces = [conversationSurface.element];
    vi.stubGlobal('document', {
      documentElement: root.element,
      querySelectorAll: () => surfaces,
    });
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    vi.stubGlobal('CustomEvent', class { constructor(public type: string, public init: unknown) {} });

    applyAppearance(preference());
    applyWallpaperPersonalization(
      {
        byColorScheme: {
          light: { image: { source: 'user', assetId: 'shared-wallpaper' }, opacityPercent: 60 },
          dark: { image: { source: 'theme' }, opacityPercent: 60 },
        },
      },
      {
        recentWallpapers: [{
          id: 'shared-wallpaper',
          imageUrl: 'gold-band-wallpaper://shared-wallpaper/full',
          thumbnailUrl: 'gold-band-wallpaper://shared-wallpaper/thumbnail',
          createdAt: '2026-08-17T00:00:00Z',
          width: 1600,
          height: 900,
        }],
      },
    );
    const sharedImage = FakeImage.instances.at(-1)!;
    sharedImage.onload?.();
    expect(conversationSurface.properties.get('--gb-wallpaper-image')).toContain('shared-wallpaper/full');

    const loadedImageCount = FakeImage.instances.length;
    conversationSurface.removeProperty.mockClear();
    refreshVisibleThemeWallpapers();
    expect(FakeImage.instances).toHaveLength(loadedImageCount);
    expect(conversationSurface.removeProperty).not.toHaveBeenCalled();

    const settingsSurface = wallpaperSurface('settings');
    surfaces = [settingsSurface.element];
    refreshVisibleThemeWallpapers();
    expect(FakeImage.instances).toHaveLength(loadedImageCount);
    expect(settingsSurface.removeProperty).not.toHaveBeenCalled();
    expect(settingsSurface.properties.get('--gb-wallpaper-image')).toContain('shared-wallpaper/full');
    expect(settingsSurface.properties.get('--gb-wallpaper-opacity')).toBe('0.6');
  });

  it('loads a distinct theme surface once and reuses it after the first successful projection', () => {
    const root = rootStub();
    const appSurface = wallpaperSurface('app');
    let surfaces = [appSurface.element];
    vi.stubGlobal('document', {
      documentElement: root.element,
      querySelectorAll: () => surfaces,
    });
    vi.stubGlobal('window', { dispatchEvent: vi.fn() });
    vi.stubGlobal('CustomEvent', class { constructor(public type: string, public init: unknown) {} });

    applyAppearance(preference());
    FakeImage.instances[0].onload?.();
    expect(appSurface.properties.get('--gb-wallpaper-image')).toContain('wallpaper-light.png');

    const firstSettingsSurface = wallpaperSurface('settings');
    surfaces = [firstSettingsSurface.element];
    refreshVisibleThemeWallpapers();
    expect(FakeImage.instances).toHaveLength(2);
    expect(FakeImage.instances[1].src).toContain('wallpaper-dark.png');
    expect(firstSettingsSurface.properties.has('--gb-wallpaper-image')).toBe(false);
    FakeImage.instances[1].onload?.();
    expect(firstSettingsSurface.properties.get('--gb-wallpaper-image')).toContain('wallpaper-dark.png');

    const remountedSettingsSurface = wallpaperSurface('settings');
    surfaces = [remountedSettingsSurface.element];
    refreshVisibleThemeWallpapers();
    expect(FakeImage.instances).toHaveLength(2);
    expect(remountedSettingsSurface.properties.get('--gb-wallpaper-image')).toContain('wallpaper-dark.png');
  });
});
