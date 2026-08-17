import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import type { ThemeIconSlot } from '@/theme-contract';
import { getCurrentThemeIconSnapshot, refreshVisibleThemeWallpapers, type ResolvedThemeIconDescriptor } from '@/theme';
import { cn } from '@/lib/utils';

type ThemeIconMap = Partial<Record<ThemeIconSlot, ResolvedThemeIconDescriptor>>;
interface ThemeIconContextValue { themeId: string; icons: ThemeIconMap }

const emptyValue: ThemeIconContextValue = { themeId: 'builtin.gold-band', icons: {} };
const ThemeIconContext = createContext<ThemeIconContextValue>(emptyValue);

export function ThemeAssetsProvider({ children }: { children: ReactNode }) {
  const [value, setValue] = useState<ThemeIconContextValue>(emptyValue);
  useEffect(() => {
    const update = (event: Event) => {
      const detail = (event as CustomEvent<ThemeIconContextValue>).detail;
      if (detail?.themeId && detail.icons) setValue(detail);
    };
    window.addEventListener('gold-band-theme-icons-changed', update);
    setValue(getCurrentThemeIconSnapshot());
    return () => window.removeEventListener('gold-band-theme-icons-changed', update);
  }, []);
  const stableValue = useMemo(() => value, [value]);
  return <ThemeIconContext.Provider value={stableValue}>{children}</ThemeIconContext.Provider>;
}

export function useThemeWallpaperSurface() {
  useLayoutEffect(() => {
    refreshVisibleThemeWallpapers();
  }, []);
}

export interface ThemeIconProps {
  slot: ThemeIconSlot;
  fallback: LucideIcon;
  className?: string;
  'aria-hidden'?: boolean | 'true' | 'false';
  'aria-label'?: string;
}

export function ThemeIcon({ slot, fallback: Fallback, className, ...accessibility }: ThemeIconProps) {
  const { icons } = useContext(ThemeIconContext);
  const descriptor = icons[slot];
  const [loadedUrl, setLoadedUrl] = useState<string | null>(null);
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!descriptor) return;
    let cancelled = false;
    const image = new Image();
    image.onload = () => { if (!cancelled) setLoadedUrl(descriptor.url); };
    image.onerror = () => { if (!cancelled) setFailedUrl(descriptor.url); };
    image.src = descriptor.url;
    return () => { cancelled = true; };
  }, [descriptor]);
  if (!descriptor || loadedUrl !== descriptor.url || failedUrl === descriptor.url) return <Fallback className={className} {...accessibility} />;
  const size = `${descriptor.nativeSize}px`;
  if (descriptor.renderMode === 'mask') {
    return (
      <span
        className={cn('inline-block shrink-0 bg-current', className)}
        style={{ width: size, height: size, maskImage: `url("${descriptor.url}")`, WebkitMaskImage: `url("${descriptor.url}")`, maskRepeat: 'no-repeat', WebkitMaskRepeat: 'no-repeat', maskPosition: 'center', WebkitMaskPosition: 'center', maskSize: 'contain', WebkitMaskSize: 'contain' }}
        {...accessibility}
      />
    );
  }
  return (
    <img
      src={descriptor.url}
      alt={accessibility['aria-label'] ?? ''}
      aria-hidden={accessibility['aria-hidden']}
      className={cn('inline-block shrink-0 object-contain', className)}
      style={{ width: size, height: size, imageRendering: descriptor.imageRendering }}
      onError={() => setFailedUrl(descriptor.url)}
    />
  );
}
