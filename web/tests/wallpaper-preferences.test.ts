import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { clearMocks, mockConvertFileSrc } from '@tauri-apps/api/mocks';
import { browserApi } from '../src/api/browser';
import { wallpaperAssetUrl } from '../src/api/desktop';
import {
  createDefaultWallpaperPreferences,
  normalizeWallpaperOpacityPercent,
  selectedWallpaper,
} from '../src/lib/wallpaper';

describe('desktop wallpaper preferences', () => {
  it('starts on the theme wallpaper and normalizes the bounded visibility scale', () => {
    expect(createDefaultWallpaperPreferences()).toEqual({
      selectedWallpaperId: null,
      recentWallpapers: [],
    });
    expect(normalizeWallpaperOpacityPercent(0)).toBe(20);
    expect(normalizeWallpaperOpacityPercent(62)).toBe(62);
    expect(normalizeWallpaperOpacityPercent(99)).toBe(99);
  });

  it('persists import, recent selection, opacity, restore, and the recent-10 limit through the browser API', async () => {
    let preferences = await browserApi.restoreThemeDesktopWallpaper();
    for (let index = 0; index < 11; index += 1) {
      preferences = (await browserApi.importDesktopWallpaper())!;
    }
    expect(preferences.wallpapers.recentWallpapers).toHaveLength(10);
    expect(selectedWallpaper(preferences.wallpapers)?.id).toBe(preferences.wallpapers.recentWallpapers[0].id);

    const selectedId = preferences.wallpapers.recentWallpapers[5].id;
    preferences = await browserApi.selectRecentDesktopWallpaper(selectedId);
    expect(preferences.wallpapers.selectedWallpaperId).toBe(selectedId);
    expect(preferences.wallpapers.recentWallpapers[0].id).toBe(selectedId);

    preferences = await browserApi.saveDesktopWallpaperOpacity(45);
    expect(preferences.personalization.wallpaper.opacityPercent).toBe(45);
    const recentIds = preferences.wallpapers.recentWallpapers.map((wallpaper) => wallpaper.id);

    preferences = await browserApi.restoreThemeDesktopWallpaper();
    expect(preferences.personalization.wallpaper.image).toEqual({ source: 'theme' });
    expect(preferences.wallpapers.selectedWallpaperId).toBeNull();
    expect(preferences.wallpapers.recentWallpapers.map((wallpaper) => wallpaper.id)).toEqual(recentIds);
  });

  it('keeps wallpaper protocol tokens in one Windows URL path segment', () => {
    Reflect.set(globalThis, 'window', {});
    mockConvertFileSrc('windows');
    try {
      const url = wallpaperAssetUrl('883ce81d-e205-4045-b80c-251be8443b2f.full');
      expect(url).toBe('http://gold-band-wallpaper.localhost/883ce81d-e205-4045-b80c-251be8443b2f.full');
      expect(url).not.toContain('%2F');
    } finally {
      clearMocks();
      Reflect.deleteProperty(globalThis, 'window');
    }
  });

  it('places the wallpaper section between typography and avatars with bounded responsive controls', () => {
    const settingsSource = fs.readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    const wallpaperSource = fs.readFileSync(path.resolve(__dirname, '../src/components/settings/WallpaperSettings.tsx'), 'utf8');
    const conversationRunSource = fs.readFileSync(path.resolve(__dirname, '../src/pages/ConversationRunPage.tsx'), 'utf8');
    const acpChatSource = fs.readFileSync(path.resolve(__dirname, '../src/components/acp/ACPChatDialog.tsx'), 'utf8');
    const composerSource = fs.readFileSync(path.resolve(__dirname, '../src/components/conversation/AcpConversationComposer.tsx'), 'utf8');
    const promptQueueSource = fs.readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationPromptQueue.tsx'), 'utf8');
    const generatedThemeCss = fs.readFileSync(path.resolve(__dirname, '../src/themes/generated/builtin-themes.css'), 'utf8');
    const typographyIndex = settingsSource.indexOf("<SettingsSection title={t('settings.typography')} divided>");
    const wallpaperIndex = settingsSource.indexOf("<SettingsSection title={t('settings.wallpaper.title')} divided>");
    const avatarIndex = settingsSource.indexOf("<SettingsSection title={t('settings.avatar.title')} divided>");

    expect(wallpaperIndex).toBeGreaterThan(typographyIndex);
    expect(avatarIndex).toBeGreaterThan(wallpaperIndex);
    expect(wallpaperSource).toContain('className="group relative aspect-video w-64 max-w-full');
    expect(wallpaperSource).toContain("<Dialog open={previewOpen} onOpenChange={setPreviewOpen}>");
    expect(wallpaperSource).toContain("aria-label={t('settings.wallpaper.collapsePreview')}");
    expect(wallpaperSource).toContain('loading="lazy"');
    expect(wallpaperSource).toContain('step={WALLPAPER_OPACITY_STEP}');
    expect(wallpaperSource).toContain('onValueChange={([value]) => {');
    expect(wallpaperSource).toContain('previewWallpaperOpacity(normalized)');
    expect(wallpaperSource).toContain('onValueCommit={([value]) => void commitOpacity(value)}');
    expect(conversationRunSource).toContain('useThemeWallpaperSurface();');
    expect(conversationRunSource).toContain('data-theme-wallpaper-slot="conversation"');
    expect(conversationRunSource).toContain('wallpaperSurface');
    expect(acpChatSource).toContain('wallpaperSurface ? "bg-transparent" : "bg-background"');
    expect(acpChatSource).toContain('wallpaperSurface ? "bg-transparent" : "bg-background"');
    expect(acpChatSource).not.toContain('wallpaperSurface={wallpaperSurface}');
    expect(composerSource).toContain("'bg-card !shadow-none transition-colors'");
    expect(promptQueueSource).toContain("'overflow-hidden border border-b-0 border-border bg-card'");
    expect(generatedThemeCss).toContain('::before{z-index:-2;background-image:var(--gb-wallpaper-image,none)');
    expect(generatedThemeCss).toContain('::after{z-index:-1;background:color-mix(in srgb,var(--gb-wallpaper-overlay-color,transparent)');
  });
});
