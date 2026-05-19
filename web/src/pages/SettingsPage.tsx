import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { ConcreteDesktopTheme, DesktopFontPreference, DesktopLanguage, DesktopThemeMode, DesktopThemePreference, PreferencesVm } from '../types';
import {
  applyFont,
  applyTheme,
  desktopFontOptions,
  desktopThemeGroups,
  fontFamilyForPreference,
  desktopThemeOptions,
  preferredThemeForMode,
  rememberConcreteThemePreference,
  resolveThemePreference,
  type DesktopThemeOption,
  type ThemePreviewPalette,
} from '../theme';
import { AppCard } from '@/components/AppCard';
import { Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { ChevronDown } from 'lucide-react';
import { getSystemFonts } from '../api';
import { cn } from '@/lib/utils';

type ThemeDrawerMode = 'all' | DesktopThemeMode;

interface SettingsPageProps {
  preferences: PreferencesVm;
  onSave: (theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference) => void;
}

export function SettingsPage({ preferences, onSave }: SettingsPageProps) {
  const { t } = useTranslation();
  const [theme, setTheme] = useState(preferences.theme);
  const [language, setLanguage] = useState(preferences.language);
  const [font, setFont] = useState(preferences.font);
  const [systemFonts, setSystemFonts] = useState<string[]>([]);
  const [themeDrawerMode, setThemeDrawerMode] = useState<ThemeDrawerMode>('all');
  const [themeSheetOpen, setThemeSheetOpen] = useState(false);
  const [preferenceVersion, setPreferenceVersion] = useState(0);

  useEffect(() => setTheme(preferences.theme), [preferences.theme]);
  useEffect(() => setLanguage(preferences.language), [preferences.language]);
  useEffect(() => setFont(preferences.font), [preferences.font]);

  useEffect(() => {
    getSystemFonts().then(setSystemFonts).catch(() => setSystemFonts([]));
  }, []);

  const chooseTheme = (value: DesktopThemePreference) => {
    if (value !== 'system') rememberConcreteThemePreference(value);
    setTheme(value);
    onSave(value, language, font);
  };

  const chooseConcreteThemeFromSheet = (value: ConcreteDesktopTheme) => {
    rememberConcreteThemePreference(value);
    setPreferenceVersion((version) => version + 1);
    if (theme === 'system') {
      applyTheme('system');
      setTheme('system');
      onSave('system', language, font);
    } else {
      setTheme(value);
      onSave(value, language, font);
    }
    setThemeSheetOpen(false);
  };

  const chooseLanguage = (value: DesktopLanguage) => {
    setLanguage(value);
    onSave(theme, value, font);
  };

  const chooseFont = (value: DesktopFontPreference) => {
    setFont(value);
    applyFont(value);
    onSave(theme, language, value);
  };

  const openThemeDrawer = (mode: ThemeDrawerMode) => {
    setThemeDrawerMode(mode);
    setThemeSheetOpen(true);
  };

  const installedFontOptions = useMemo(() => {
    const presetIds = new Set<string>(desktopFontOptions.map((option) => option.id));
    return systemFonts.filter((family) => !presetIds.has(family));
  }, [systemFonts]);

  const syncWithOs = theme === 'system';
  const resolvedTheme = resolveThemePreference(theme);
  const currentTheme = getThemeOption(resolvedTheme);
  const preferredLightTheme = getThemeOption(preferredThemeForMode('light'));
  const preferredDarkTheme = getThemeOption(preferredThemeForMode('dark'));
  const defaultFontOption = desktopFontOptions[0];
  const usingBuiltInFont = font === defaultFontOption.id;
  const selectedLocalFont = usingBuiltInFont ? null : font;
  void preferenceVersion;

  return (
    <Page className="space-y-6 p-8">
      <div className="flex items-center justify-between gap-4">
        <span className="font-mono text-xs text-muted-foreground">{t('settings.path')}</span>
        <div className="flex gap-2">
          <Button variant="ghost" disabled>{t('common.export')}</Button>
          <Button disabled>{t('common.run')}</Button>
        </div>
      </div>

      <PageHeader title={t('settings.title')} />

      <AppCard className="gap-0 overflow-hidden py-0">
        <SettingsSection title={t('settings.appearance')}>
          <div className="flex items-center justify-between gap-4 py-2">
            <div className="min-w-0 space-y-1">
              <div className="text-sm font-semibold">{t('settings.syncWithOs')}</div>
              <div className="text-xs text-muted-foreground">{t('settings.syncWithOsDescription')}</div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={syncWithOs}
              className={cn(
                'relative h-6 w-11 shrink-0 overflow-hidden rounded-full border p-0.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                syncWithOs ? 'border-primary bg-primary' : 'border-border/70 bg-muted-foreground/20',
              )}
              onClick={() => chooseTheme(syncWithOs ? resolvedTheme : 'system')}
            >
              <span
                className={cn(
                  'block size-5 rounded-full bg-background shadow-sm transition-transform',
                  syncWithOs && 'translate-x-5',
                )}
              />
            </button>
          </div>

          <Sheet open={themeSheetOpen} onOpenChange={setThemeSheetOpen}>
            {syncWithOs ? (
              <div className="grid gap-3 md:grid-cols-2">
                <ThemeSummaryCard
                  eyebrow={t('settings.lightDefaultTheme')}
                  option={preferredLightTheme}
                  active={resolvedTheme === preferredLightTheme.id}
                  buttonLabel={t('settings.chooseLightTheme')}
                  onOpen={() => openThemeDrawer('light')}
                />
                <ThemeSummaryCard
                  eyebrow={t('settings.darkDefaultTheme')}
                  option={preferredDarkTheme}
                  active={resolvedTheme === preferredDarkTheme.id}
                  buttonLabel={t('settings.chooseDarkTheme')}
                  onOpen={() => openThemeDrawer('dark')}
                />
              </div>
            ) : (
              <ThemeSummaryCard
                eyebrow={t('settings.currentTheme')}
                option={currentTheme}
                buttonLabel={t('settings.chooseTheme')}
                onOpen={() => openThemeDrawer('all')}
              />
            )}
            <SheetContent className="w-[760px] max-w-[92vw] sm:max-w-[760px]" closeLabel={t('common.close')}>
              <SheetHeader className="border-b px-5 py-4">
                <SheetTitle>{themeDrawerMode === 'light' ? t('settings.chooseLightTheme') : themeDrawerMode === 'dark' ? t('settings.chooseDarkTheme') : t('settings.themeDrawerTitle')}</SheetTitle>
              </SheetHeader>
              <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-6 pt-2">
                {(themeDrawerMode === 'all' || themeDrawerMode === 'light') ? (
                  <ThemeOptionGroup
                    title={t('settings.lightThemes')}
                    options={desktopThemeGroups.light}
                    currentTheme={theme}
                    resolvedTheme={resolvedTheme}
                    onSelect={chooseConcreteThemeFromSheet}
                  />
                ) : null}
                {(themeDrawerMode === 'all' || themeDrawerMode === 'dark') ? (
                  <ThemeOptionGroup
                    title={t('settings.darkThemes')}
                    options={desktopThemeGroups.dark}
                    currentTheme={theme}
                    resolvedTheme={resolvedTheme}
                    onSelect={chooseConcreteThemeFromSheet}
                  />
                ) : null}
              </div>
            </SheetContent>
          </Sheet>
        </SettingsSection>

        <SettingsSection title={t('settings.typography')} divided>
          <button
            type="button"
            aria-pressed={usingBuiltInFont}
            className={cn(
              'max-w-xl rounded-lg border border-border/45 bg-transparent p-3 text-left transition hover:border-primary/60 hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              usingBuiltInFont && 'border-primary/65 bg-primary/[0.07]',
            )}
            onClick={() => chooseFont(defaultFontOption.id)}
          >
            <div className="text-sm font-semibold">{t(defaultFontOption.labelKey)}</div>
            <FontPreviewSample sample={defaultFontOption.preview} fontFamily={defaultFontOption.stack} />
          </button>
          <div className={cn('max-w-xl rounded-lg border border-border/35 bg-transparent p-3', selectedLocalFont && 'border-primary/45 bg-primary/[0.04]')}>
            <div className="space-y-1">
              <div className="text-sm font-semibold">{t('settings.localFonts')}</div>
              <div className="text-xs text-muted-foreground">{t('settings.localFontsDescription', { count: installedFontOptions.length })}</div>
            </div>
            <div className="relative mt-3">
              <select
                value={selectedLocalFont ?? ''}
                className="h-10 w-full appearance-none rounded-md border border-border/45 bg-background px-3 pr-10 text-sm text-foreground shadow-sm outline-none transition focus:border-primary focus:ring-2 focus:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-60"
                onChange={(event) => chooseFont(event.target.value as DesktopFontPreference)}
                disabled={installedFontOptions.length === 0}
              >
                <option value="" disabled>{t('settings.chooseLocalFont')}</option>
                {installedFontOptions.map((family) => (
                  <option key={family} value={family}>{family}</option>
                ))}
              </select>
              <ChevronDown className="pointer-events-none absolute right-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            </div>
            {selectedLocalFont ? <FontPreviewSample sample="任务编排 / AI Workflow" fontFamily={fontFamilyForPreference(selectedLocalFont)} /> : null}
          </div>
        </SettingsSection>

        <SettingsSection title={t('settings.language')} divided>
          <Select value={language} onValueChange={(value) => chooseLanguage(value as DesktopLanguage)}>
            <SelectTrigger className="w-56">
              <SelectValue aria-label={language} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="zh-cn">中文</SelectItem>
              <SelectItem value="en">English</SelectItem>
            </SelectContent>
          </Select>
        </SettingsSection>
      </AppCard>

      <Badge variant="outline" className="font-mono text-muted-foreground"><span className="mr-2 size-2 rounded-full bg-gold-success" /> CLIENT VERSION: 2.4.1-STABLE</Badge>
    </Page>
  );
}

function SettingsSection({ title, children, divided = false }: { title: string; children: ReactNode; divided?: boolean }) {
  return (
    <section className={cn('grid gap-4 px-5 py-5 lg:grid-cols-[160px_minmax(0,1fr)]', divided && 'border-t border-border/45')}>
      <h2 className="text-base font-semibold text-foreground">{title}</h2>
      <div className="min-w-0 space-y-4">{children}</div>
    </section>
  );
}

interface ThemeSummaryCardProps {
  eyebrow: string;
  option: DesktopThemeOption;
  active?: boolean;
  buttonLabel: string;
  onOpen: () => void;
}

function ThemeSummaryCard({ eyebrow, option, active = false, buttonLabel, onOpen }: ThemeSummaryCardProps) {
  const { t } = useTranslation();
  return (
    <div className={cn('flex items-center justify-between gap-4 rounded-lg border border-border/35 bg-transparent p-3 transition-colors', active && 'border-primary/45 bg-primary/[0.04]')}>
      <div className="flex min-w-0 items-center gap-4">
        <TerminalPreview palette={option.preview} compact />
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-muted-foreground">{eyebrow}</span>
            {active ? <Badge variant="outline" className="px-1.5 py-0 text-[10px]">{t('settings.activeTheme')}</Badge> : null}
          </div>
          <div className="text-base font-semibold">{t(option.labelKey)}</div>
          <div className="text-xs text-muted-foreground">{t(option.descriptionKey)}</div>
        </div>
      </div>
      <Button variant="outline" className="shrink-0" onClick={onOpen}>{buttonLabel}</Button>
    </div>
  );
}

interface ThemeOptionGroupProps {
  title: string;
  options: readonly DesktopThemeOption[];
  currentTheme: DesktopThemePreference;
  resolvedTheme: ConcreteDesktopTheme;
  onSelect: (theme: ConcreteDesktopTheme) => void;
}

function ThemeOptionGroup({ title, options, currentTheme, resolvedTheme, onSelect }: ThemeOptionGroupProps) {
  return (
    <section className="grid gap-3 py-4 lg:grid-cols-[72px_minmax(0,1fr)]">
      <div className="pt-3 text-sm font-semibold text-muted-foreground">{title}</div>
      <div className="grid gap-3">
        {options.map((option) => (
          <ThemeOptionCard
            key={option.id}
            option={option}
            selected={currentTheme === option.id}
            synced={currentTheme === 'system' && resolvedTheme === option.id}
            onSelect={() => onSelect(option.id)}
          />
        ))}
      </div>
    </section>
  );
}

interface ThemeOptionCardProps {
  option: DesktopThemeOption;
  selected: boolean;
  synced: boolean;
  onSelect: () => void;
}

function ThemeOptionCard({ option, selected, synced, onSelect }: ThemeOptionCardProps) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      aria-pressed={selected}
      className={cn(
        'group flex min-h-32 gap-4 rounded-lg border border-border/40 bg-transparent p-3 text-left transition hover:border-primary/60 hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        selected && 'border-primary/65 bg-primary/[0.07] text-primary',
        !selected && synced && 'border-primary/40',
      )}
      onClick={onSelect}
    >
      <TerminalPreview palette={option.preview} />
      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-semibold text-foreground">{t(option.labelKey)}</span>
          {synced && !selected ? <Badge variant="outline" className="px-1.5 py-0 text-[10px]">{t('settings.activeTheme')}</Badge> : null}
        </div>
        <span className="text-xs leading-relaxed text-muted-foreground">{t(option.descriptionKey)}</span>
      </div>
    </button>
  );
}

function FontPreviewSample({ sample, fontFamily }: { sample: string; fontFamily: string }) {
  const { t } = useTranslation();
  const [leading, trailing] = sample.split(' / ');
  return (
    <div className="mt-3 rounded-md border border-border/35 bg-background/60 px-3 py-2">
      <div className="text-[11px] font-medium text-muted-foreground">{t('settings.fontPreview')}</div>
      <div className="mt-1 text-sm font-medium" style={{ fontFamily }}>
        {trailing ? (
          <>
            <span className="text-primary">{leading}</span>
            <span className="mx-1 text-muted-foreground">/</span>
            <span className="text-gold-success">{trailing}</span>
          </>
        ) : (
          <span className="text-primary">{sample}</span>
        )}
      </div>
    </div>
  );
}

function TerminalPreview({ palette, compact = false }: { palette: ThemePreviewPalette; compact?: boolean }) {
  const shellStyle = {
    backgroundColor: palette.background,
    borderColor: palette.border,
    color: palette.foreground,
  } satisfies CSSProperties;

  const surfaceStyle = {
    backgroundColor: palette.surface,
    borderColor: palette.border,
  } satisfies CSSProperties;

  return (
    <div
      className={cn(
        'shrink-0 overflow-hidden rounded-md border font-mono shadow-sm',
        compact ? 'h-[72px] w-[112px] text-[7px]' : 'h-[104px] w-[162px] text-[9px]',
      )}
      style={shellStyle}
    >
      <div className="flex items-center gap-1 border-b px-2 py-1" style={surfaceStyle}>
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.danger }} />
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.primary }} />
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.success }} />
      </div>
      <div className={cn('space-y-2', compact ? 'px-2 py-1.5' : 'px-3 py-2')}>
        <div style={{ color: palette.muted }}>$ gold-band run</div>
        <div><span style={{ color: palette.primary }}>workflow</span> ready</div>
        {!compact ? <div style={{ color: palette.success }}>validation passed</div> : null}
        <div className={cn('h-3 w-0.5 animate-pulse', compact ? 'mt-1' : 'mt-3')} style={{ backgroundColor: palette.primary }} />
      </div>
    </div>
  );
}

function getThemeOption(theme: ConcreteDesktopTheme): DesktopThemeOption {
  return desktopThemeOptions.find((option) => option.id === theme) ?? desktopThemeOptions[0];
}
