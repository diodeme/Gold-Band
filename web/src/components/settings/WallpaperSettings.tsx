import { useEffect, useRef, useState } from 'react';
import { Check, ImageIcon, ImagePlus, Loader2, Maximize2, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Dialog, DialogClose, DialogContent, DialogTitle, DialogTrigger } from '@/components/ui/dialog';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Slider } from '@/components/ui/slider';
import {
  MAX_WALLPAPER_OPACITY_PERCENT,
  MIN_WALLPAPER_OPACITY_PERCENT,
  WALLPAPER_OPACITY_STEP,
  normalizeWallpaperOpacityPercent,
  selectedWallpaper,
} from '@/lib/wallpaper';
import { cn } from '@/lib/utils';
import { previewWallpaperOpacity, type ResolvedThemeWallpaperDescriptor } from '@/theme';
import type { PersonalizationPreference, WallpaperPreferencesVm } from '@/types';

interface WallpaperSettingsProps {
  preferences: WallpaperPreferencesVm;
  personalization: PersonalizationPreference['wallpaper'];
  themeWallpaper?: ResolvedThemeWallpaperDescriptor;
  busy: boolean;
  onImportWallpaper: () => Promise<WallpaperPreferencesVm | undefined>;
  onSelectRecentWallpaper: (wallpaperId: string) => Promise<WallpaperPreferencesVm | undefined>;
  onSaveWallpaperOpacity: (opacityPercent: number) => Promise<WallpaperPreferencesVm | undefined>;
  onRestoreThemeWallpaper: () => Promise<WallpaperPreferencesVm | undefined>;
}

export function WallpaperSettings({
  preferences,
  personalization,
  themeWallpaper,
  busy,
  onImportWallpaper,
  onSelectRecentWallpaper,
  onSaveWallpaperOpacity,
  onRestoreThemeWallpaper,
}: WallpaperSettingsProps) {
  const { t } = useTranslation();
  const [pickerOpen, setPickerOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [opacityPercent, setOpacityPercent] = useState(() => normalizeWallpaperOpacityPercent(personalization.opacityPercent));
  const updatingRef = useRef(false);
  const customWallpaper = personalization.image.source === 'user' ? selectedWallpaper(preferences) : null;
  const previewUrl = customWallpaper?.imageUrl ?? themeWallpaper?.url ?? null;
  const previewOpacity = customWallpaper ? opacityPercent / 100 : themeWallpaper?.opacity ?? 1;
  const disabled = busy || updating;

  useEffect(() => {
    setOpacityPercent(normalizeWallpaperOpacityPercent(personalization.opacityPercent));
  }, [personalization.opacityPercent]);

  const runUpdate = async (action: () => Promise<WallpaperPreferencesVm | undefined>) => {
    if (updatingRef.current) return undefined;
    updatingRef.current = true;
    setUpdating(true);
    try {
      return await action();
    } finally {
      updatingRef.current = false;
      setUpdating(false);
    }
  };

  const importWallpaper = async () => {
    const saved = await runUpdate(onImportWallpaper);
    if (saved) setPickerOpen(false);
  };

  const selectRecent = async (wallpaperId: string) => {
    const saved = await runUpdate(() => onSelectRecentWallpaper(wallpaperId));
    if (saved) setPickerOpen(false);
  };

  const restoreTheme = async () => {
    await runUpdate(onRestoreThemeWallpaper);
  };

  const commitOpacity = async (value: number) => {
    const normalized = normalizeWallpaperOpacityPercent(value);
    const saved = await runUpdate(() => onSaveWallpaperOpacity(normalized));
    if (!saved) {
      const persisted = normalizeWallpaperOpacityPercent(personalization.opacityPercent);
      setOpacityPercent(persisted);
      previewWallpaperOpacity(persisted);
    }
  };

  return (
    <div data-testid="wallpaper-settings" className="@container/wallpaper-settings flex min-w-0 flex-col gap-4 @3xl/wallpaper-settings:flex-row @3xl/wallpaper-settings:items-center">
      {previewUrl ? (
        <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
          <DialogTrigger asChild>
            <button
              type="button"
              aria-label={t('settings.wallpaper.expandPreview')}
              className="group relative aspect-video w-64 max-w-full shrink-0 cursor-zoom-in overflow-hidden rounded-lg border border-border/45 bg-muted/35 text-left outline-none transition-[border-color,box-shadow] hover:border-border focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
            >
              <img
                src={previewUrl}
                alt=""
                width={320}
                height={180}
                className="size-full object-cover"
                style={{ opacity: previewOpacity }}
              />
              {themeWallpaper?.overlayOpacity ? (
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-0 bg-background"
                  style={{ opacity: themeWallpaper.overlayOpacity }}
                />
              ) : null}
              <span className="absolute right-2 top-2 flex size-7 items-center justify-center rounded-md bg-background/85 text-foreground opacity-0 shadow-sm backdrop-blur-sm transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
                <Maximize2 className="size-3.5" aria-hidden="true" />
              </span>
              <span className="absolute bottom-2 left-2 rounded-md bg-background/85 px-2 py-1 text-ui-micro font-medium text-foreground shadow-sm backdrop-blur-sm">
                {customWallpaper ? t('settings.wallpaper.customActive') : t('settings.wallpaper.themeActive')}
              </span>
            </button>
          </DialogTrigger>
          <DialogContent
            className="w-fit max-w-[calc(100vw-2rem)] border-0 bg-transparent p-0 shadow-none sm:max-w-[min(72rem,calc(100vw-4rem))]"
            showCloseButton={false}
          >
            <DialogTitle className="sr-only">{t('settings.wallpaper.previewTitle')}</DialogTitle>
            <DialogClose asChild>
              <button
                type="button"
                aria-label={t('settings.wallpaper.collapsePreview')}
                className="relative max-h-[calc(100vh-4rem)] max-w-full cursor-zoom-out overflow-hidden rounded-lg bg-black/30 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              >
                <img
                  src={previewUrl}
                  alt=""
                  className="block max-h-[calc(100vh-4rem)] max-w-full object-contain"
                  style={{ opacity: previewOpacity }}
                />
                {themeWallpaper?.overlayOpacity ? (
                  <span
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-0 bg-background"
                    style={{ opacity: themeWallpaper.overlayOpacity }}
                  />
                ) : null}
              </button>
            </DialogClose>
          </DialogContent>
        </Dialog>
      ) : (
        <div className="relative aspect-video w-64 max-w-full shrink-0 overflow-hidden rounded-lg border border-border/45 bg-muted/35">
          <div className="flex size-full flex-col items-center justify-center gap-2 text-muted-foreground">
            <ImageIcon className="size-5" aria-hidden="true" />
            <span className="text-xs">{t('settings.wallpaper.themeEmpty')}</span>
          </div>
          <span className="absolute bottom-2 left-2 rounded-md bg-background/85 px-2 py-1 text-ui-micro font-medium text-foreground shadow-sm backdrop-blur-sm">
            {t('settings.wallpaper.themeActive')}
          </span>
        </div>
      )}

      <div className="min-w-0 flex-1 space-y-4">
        <div className="flex min-w-0 flex-wrap items-start justify-between gap-3">
          <div className="min-w-0 space-y-1">
            <div className="text-sm font-semibold text-foreground">{t('settings.wallpaper.chooseTitle')}</div>
            <div className="text-xs text-muted-foreground">{t('settings.wallpaper.description')}</div>
          </div>
          <div className="flex flex-wrap gap-2">
            <Popover open={pickerOpen} onOpenChange={setPickerOpen}>
              <PopoverTrigger asChild>
                <Button type="button" variant="outline" disabled={disabled}>
                  {updating ? <Loader2 className="animate-spin" /> : <ImagePlus />}
                  {t('settings.wallpaper.choose')}
                </Button>
              </PopoverTrigger>
              <PopoverContent align="end" className="w-[min(22rem,calc(100vw-2rem))] p-2">
                <div className="px-1 pb-2 text-xs font-medium text-muted-foreground">
                  {t('settings.wallpaper.recent')}
                </div>
                {preferences.recentWallpapers.length > 0 ? (
                  <div className="grid grid-cols-2 gap-2">
                    {preferences.recentWallpapers.map((wallpaper) => {
                      const selected = wallpaper.id === preferences.selectedWallpaperId;
                      return (
                        <button
                          key={wallpaper.id}
                          type="button"
                          aria-label={t('settings.wallpaper.useRecent')}
                          aria-pressed={selected}
                          className={cn(
                            'group relative aspect-video min-w-0 overflow-hidden rounded-md border border-border/45 bg-muted outline-none transition-[border-color,box-shadow,filter] hover:brightness-95 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
                            selected && 'border-primary ring-1 ring-primary/45',
                          )}
                          disabled={disabled}
                          onClick={() => void selectRecent(wallpaper.id)}
                        >
                          <img
                            src={wallpaper.thumbnailUrl}
                            alt=""
                            width={320}
                            height={180}
                            loading="lazy"
                            className="size-full object-cover"
                          />
                          {selected ? (
                            <span className="absolute right-1.5 top-1.5 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-sm">
                              <Check className="size-3" aria-hidden="true" />
                            </span>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                ) : (
                  <div className="px-1 py-4 text-center text-xs text-muted-foreground">
                    {t('settings.wallpaper.noRecent')}
                  </div>
                )}
                <div className="mt-2 border-t border-border/45 pt-2">
                  <Button type="button" variant="ghost" className="w-full justify-start" disabled={disabled} onClick={() => void importWallpaper()}>
                    <ImagePlus />
                    {t('settings.wallpaper.import')}
                  </Button>
                </div>
              </PopoverContent>
            </Popover>
            {personalization.image.source === 'user' ? (
              <Button type="button" variant="ghost" disabled={disabled} onClick={() => void restoreTheme()}>
                <RotateCcw />
                {t('settings.wallpaper.restoreTheme')}
              </Button>
            ) : null}
          </div>
        </div>

        {customWallpaper ? (
          <div className="grid min-w-0 grid-cols-[auto_minmax(120px,1fr)_3ch] items-center gap-3">
            <label htmlFor="wallpaper-opacity" className="text-sm text-foreground">
              {t('settings.wallpaper.visibility')}
            </label>
            <Slider
              id="wallpaper-opacity"
              value={[opacityPercent]}
              min={MIN_WALLPAPER_OPACITY_PERCENT}
              max={MAX_WALLPAPER_OPACITY_PERCENT}
              step={WALLPAPER_OPACITY_STEP}
              disabled={disabled}
              aria-label={t('settings.wallpaper.visibility')}
              onValueChange={([value]) => {
                const normalized = normalizeWallpaperOpacityPercent(value);
                setOpacityPercent(normalized);
                previewWallpaperOpacity(normalized);
              }}
              onValueCommit={([value]) => void commitOpacity(value)}
            />
            <span className="text-right text-sm tabular-nums text-muted-foreground">{opacityPercent}%</span>
          </div>
        ) : null}
      </div>
    </div>
  );
}
