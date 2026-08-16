import type { WallpaperImageVm, WallpaperPreferencesVm } from '@/types';

export const DEFAULT_WALLPAPER_OPACITY_PERCENT = 60;
export const MIN_WALLPAPER_OPACITY_PERCENT = 20;
export const MAX_WALLPAPER_OPACITY_PERCENT = 100;
export const WALLPAPER_OPACITY_STEP = 1;

export function createDefaultWallpaperPreferences(): WallpaperPreferencesVm {
  return {
    selectedWallpaperId: null,
    recentWallpapers: [],
  };
}

export function selectedWallpaper(
  preferences: WallpaperPreferencesVm,
): WallpaperImageVm | null {
  if (!preferences.selectedWallpaperId) return null;
  return preferences.recentWallpapers.find(
    (wallpaper) => wallpaper.id === preferences.selectedWallpaperId,
  ) ?? null;
}

export function normalizeWallpaperOpacityPercent(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_WALLPAPER_OPACITY_PERCENT;
  const stepped = Math.round(value / WALLPAPER_OPACITY_STEP) * WALLPAPER_OPACITY_STEP;
  return Math.min(MAX_WALLPAPER_OPACITY_PERCENT, Math.max(MIN_WALLPAPER_OPACITY_PERCENT, stepped));
}
