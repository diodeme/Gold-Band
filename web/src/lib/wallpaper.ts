import type { WallpaperImagePreference, WallpaperImageVm, WallpaperPreferencesVm } from '@/types';

export const DEFAULT_WALLPAPER_OPACITY_PERCENT = 60;
export const MIN_WALLPAPER_OPACITY_PERCENT = 20;
export const MAX_WALLPAPER_OPACITY_PERCENT = 100;
export const WALLPAPER_OPACITY_STEP = 1;
export const MAX_RECENT_WALLPAPERS = 10;

export function createDefaultWallpaperPreferences(): WallpaperPreferencesVm {
  return {
    recentWallpapers: [],
  };
}

export function selectedWallpaper(
  preferences: WallpaperPreferencesVm,
  image: WallpaperImagePreference,
): WallpaperImageVm | null {
  if (image.source !== 'user') return null;
  return preferences.recentWallpapers.find(
    (wallpaper) => wallpaper.id === image.assetId,
  ) ?? null;
}

export interface ThemeWallpaperOption {
  assetId: string;
  url: string;
}

/**
 * Selectable theme wallpapers for one color scheme: the distinct assets the
 * theme assigns to that scheme's slots, in slot declaration order. Themes
 * without the wallpapers capability yield an empty list.
 */
export function themeWallpaperOptions(
  descriptors: Iterable<{ assetId: string; url: string } | undefined>,
): ThemeWallpaperOption[] {
  const seen = new Set<string>();
  const options: ThemeWallpaperOption[] = [];
  for (const descriptor of descriptors) {
    if (!descriptor || seen.has(descriptor.assetId)) continue;
    seen.add(descriptor.assetId);
    options.push({ assetId: descriptor.assetId, url: descriptor.url });
  }
  return options;
}

/** The pinned theme wallpaper asset id, or null for the default assignment. */
export function themeWallpaperPin(image: WallpaperImagePreference): string | null {
  return image.source === 'theme' && image.assetId?.trim() ? image.assetId : null;
}

export function boundedRecentWallpapers(
  wallpapers: readonly WallpaperImageVm[],
  retainedAssetIds: readonly string[],
): WallpaperImageVm[] {
  const seen = new Set<string>();
  const recent = wallpapers.filter((wallpaper) => {
    if (seen.has(wallpaper.id)) return false;
    seen.add(wallpaper.id);
    return true;
  });
  const retained = new Set(retainedAssetIds);
  while (recent.length > MAX_RECENT_WALLPAPERS) {
    let removeIndex = recent.length - 1;
    while (removeIndex > 0 && retained.has(recent[removeIndex].id)) removeIndex -= 1;
    if (removeIndex === 0) removeIndex = recent.length - 1;
    recent.splice(removeIndex, 1);
  }
  return recent;
}

export function normalizeWallpaperOpacityPercent(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_WALLPAPER_OPACITY_PERCENT;
  const stepped = Math.round(value / WALLPAPER_OPACITY_STEP) * WALLPAPER_OPACITY_STEP;
  return Math.min(MAX_WALLPAPER_OPACITY_PERCENT, Math.max(MIN_WALLPAPER_OPACITY_PERCENT, stepped));
}
