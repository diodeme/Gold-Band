// @vitest-environment jsdom

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const themeMocks = vi.hoisted(() => ({
  snapshot: {
    themeId: 'builtin.gold-band',
    icons: {
      'navigation.search': {
        assetId: 'search-mask', renderMode: 'mask', nativeSize: 20, imageRendering: 'auto',
        url: '/theme-assets/search-mask.png',
      },
    },
  },
  refreshVisibleThemeWallpapers: vi.fn(),
}));

vi.mock('@/theme', () => ({
  getCurrentThemeIconSnapshot: () => themeMocks.snapshot,
  refreshVisibleThemeWallpapers: themeMocks.refreshVisibleThemeWallpapers,
}));

import {
  ThemeAssetsProvider,
  ThemeIcon,
  useThemeWallpaperSurface,
} from '../src/components/theme/ThemeAssetsContext';

class FakeImage {
  static instances: FakeImage[] = [];
  onload: null | (() => void) = null;
  onerror: null | (() => void) = null;
  src = '';

  constructor() {
    FakeImage.instances.push(this);
  }
}

function FallbackIcon(props: React.SVGProps<SVGSVGElement>) {
  return <svg data-testid="fallback" {...props} />;
}

describe('ThemeAssetsContext', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    FakeImage.instances = [];
    themeMocks.refreshVisibleThemeWallpapers.mockReset();
    vi.stubGlobal('Image', FakeImage);
    container = document.createElement('div');
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    globalThis.IS_REACT_ACT_ENVIRONMENT = false;
  });

  it('preloads themed icons, preserves accessible fallback, and isolates non-icon children from context updates', async () => {
    let heavyRenders = 0;
    function HeavyTimeline() {
      heavyRenders += 1;
      return <div data-testid="timeline">stable timeline</div>;
    }

    await act(async () => {
      root.render(
        <ThemeAssetsProvider>
          <HeavyTimeline />
          <button aria-label="Search">
            <ThemeIcon slot="navigation.search" fallback={FallbackIcon} aria-hidden="true" />
          </button>
        </ThemeAssetsProvider>,
      );
    });

    expect(container.querySelector('[data-testid="fallback"]')).not.toBeNull();
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Search');
    expect(FakeImage.instances).toHaveLength(1);
    expect(heavyRenders).toBe(1);

    await act(async () => FakeImage.instances[0].onload?.());
    const mask = container.querySelector('button span');
    expect(mask).not.toBeNull();
    expect(mask?.getAttribute('aria-hidden')).toBe('true');
    expect((mask as HTMLElement).style.maskImage).toContain('/theme-assets/search-mask.png');

    await act(async () => {
      window.dispatchEvent(new CustomEvent('gold-band-theme-icons-changed', {
        detail: {
          themeId: 'builtin.tech-neutral',
          icons: {
            'navigation.search': {
              assetId: 'search-image', renderMode: 'image', nativeSize: 24, imageRendering: 'pixelated',
              url: '/theme-assets/search-image.png',
            },
          },
        },
      }));
    });
    expect(heavyRenders).toBe(1);
    expect(FakeImage.instances).toHaveLength(2);
    expect(container.querySelector('[data-testid="fallback"]')).not.toBeNull();

    await act(async () => FakeImage.instances[1].onerror?.());
    expect(container.querySelector('[data-testid="fallback"]')).not.toBeNull();
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Search');
  });

  it('renders a loaded image descriptor without changing the caller-owned accessible name', async () => {
    await act(async () => {
      root.render(
        <ThemeAssetsProvider>
          <button aria-label="Search">
            <ThemeIcon slot="navigation.search" fallback={FallbackIcon} aria-hidden="true" />
          </button>
        </ThemeAssetsProvider>,
      );
    });
    await act(async () => {
      window.dispatchEvent(new CustomEvent('gold-band-theme-icons-changed', {
        detail: {
          themeId: 'builtin.gold-band',
          icons: {
            'navigation.search': {
              assetId: 'search-image', renderMode: 'image', nativeSize: 24, imageRendering: 'pixelated',
              url: '/theme-assets/search-image.png',
            },
          },
        },
      }));
    });
    await act(async () => FakeImage.instances.at(-1)?.onload?.());
    const image = container.querySelector('img');
    expect(image?.getAttribute('src')).toBe('/theme-assets/search-image.png');
    expect(image?.getAttribute('aria-hidden')).toBe('true');
    expect(image?.style.imageRendering).toBe('pixelated');
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Search');
  });

  it('reconciles a newly mounted wallpaper surface before its first paint', async () => {
    function Surface() {
      useThemeWallpaperSurface();
      return <div data-theme-wallpaper-slot="settings" />;
    }

    await act(async () => root.render(<Surface />));
    expect(themeMocks.refreshVisibleThemeWallpapers).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    root = createRoot(container);
  });
});
