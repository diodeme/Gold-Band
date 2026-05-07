import { useEffect, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import type { ConcreteDesktopTheme, DesktopLanguage, DesktopThemePreference, PreferencesVm } from '../types';
import { desktopThemeGroups, resolveThemePreference, type DesktopThemeOption, type ThemePreviewPalette } from '../theme';
import { AppCard } from '@/components/AppCard';
import { Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';

interface SettingsPageProps {
  preferences: PreferencesVm;
  onSave: (theme: DesktopThemePreference, language: DesktopLanguage) => void;
}

export function SettingsPage({ preferences, onSave }: SettingsPageProps) {
  const { t } = useTranslation();
  const [theme, setTheme] = useState(preferences.theme);
  const [language, setLanguage] = useState(preferences.language);

  useEffect(() => setTheme(preferences.theme), [preferences.theme]);
  useEffect(() => setLanguage(preferences.language), [preferences.language]);

  const chooseTheme = (value: DesktopThemePreference) => {
    setTheme(value);
    onSave(value, language);
  };

  const chooseLanguage = (value: DesktopLanguage) => {
    setLanguage(value);
    onSave(theme, value);
  };

  const syncWithOs = theme === 'system';
  const resolvedTheme = resolveThemePreference(theme);

  return (
    <Page className="space-y-6 p-8">
      <div className="flex items-center justify-between rounded-xl border bg-background/60 px-4 py-3">
        <span className="font-mono text-xs text-muted-foreground">{t('settings.path')}</span>
        <div className="flex gap-2">
          <Button variant="outline" disabled>{t('common.export')}</Button>
          <Button disabled>{t('common.run')}</Button>
        </div>
      </div>

      <PageHeader title={t('settings.title')} />

      <AppCard className="gap-3 py-4">
        <CardHeader className="px-5">
          <CardTitle>{t('settings.appearance')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-5 px-5">
          <div className="flex items-center justify-between rounded-xl border bg-muted/35 px-4 py-3">
            <div className="space-y-1">
              <div className="text-sm font-semibold">{t('settings.syncWithOs')}</div>
              <div className="text-xs text-muted-foreground">{t('settings.syncWithOsDescription')}</div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={syncWithOs}
              className={cn(
                'relative h-6 w-11 rounded-full border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                syncWithOs ? 'border-primary bg-primary' : 'border-border bg-secondary',
              )}
              onClick={() => chooseTheme(syncWithOs ? resolvedTheme : 'system')}
            >
              <span
                className={cn(
                  'absolute top-0.5 size-5 rounded-full bg-background shadow-sm transition-transform',
                  syncWithOs ? 'translate-x-5' : 'translate-x-0.5',
                )}
              />
            </button>
          </div>

          <ThemeOptionGroup
            title={t('settings.lightThemes')}
            options={desktopThemeGroups.light}
            currentTheme={theme}
            resolvedTheme={resolvedTheme}
            onSelect={chooseTheme}
          />
          <ThemeOptionGroup
            title={t('settings.darkThemes')}
            options={desktopThemeGroups.dark}
            currentTheme={theme}
            resolvedTheme={resolvedTheme}
            onSelect={chooseTheme}
          />
        </CardContent>
      </AppCard>

      <AppCard className="gap-3 py-4">
        <CardHeader className="px-5">
          <CardTitle>{t('settings.language')}</CardTitle>
        </CardHeader>
        <CardContent className="px-5">
          <Select value={language} onValueChange={(value) => chooseLanguage(value as DesktopLanguage)}>
            <SelectTrigger className="w-56">
              <SelectValue aria-label={language} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="zh-cn">中文</SelectItem>
              <SelectItem value="en">English</SelectItem>
            </SelectContent>
          </Select>
        </CardContent>
      </AppCard>

      <Badge variant="outline" className="font-mono text-muted-foreground"><span className="mr-2 size-2 rounded-full bg-gold-success" /> CLIENT VERSION: 2.4.1-STABLE</Badge>
    </Page>
  );
}

interface ThemeOptionGroupProps {
  title: string;
  options: readonly DesktopThemeOption[];
  currentTheme: DesktopThemePreference;
  resolvedTheme: ConcreteDesktopTheme;
  onSelect: (theme: DesktopThemePreference) => void;
}

function ThemeOptionGroup({ title, options, currentTheme, resolvedTheme, onSelect }: ThemeOptionGroupProps) {
  return (
    <section className="grid gap-3 lg:grid-cols-[88px_minmax(0,1fr)]">
      <div className="pt-3 text-sm font-semibold text-muted-foreground">{title}</div>
      <div className="grid gap-3 md:grid-cols-2">
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
        'group flex min-h-32 gap-4 rounded-xl border bg-card/80 p-3 text-left transition hover:border-primary/70 hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        selected && 'border-primary bg-primary/10 text-primary shadow-sm',
        !selected && synced && 'border-primary/40',
      )}
      onClick={onSelect}
    >
      <TerminalPreview palette={option.preview} />
      <div className="flex min-w-0 flex-1 flex-col justify-center gap-1">
        <div className="flex items-center gap-2">
          <span className="font-semibold text-foreground">{t(option.labelKey)}</span>
          {synced && !selected ? <Badge variant="outline" className="px-1.5 py-0 text-[10px]">{t('settings.system')}</Badge> : null}
        </div>
        <span className="text-xs leading-relaxed text-muted-foreground">{t(option.descriptionKey)}</span>
      </div>
    </button>
  );
}

function TerminalPreview({ palette }: { palette: ThemePreviewPalette }) {
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
    <div className="h-[104px] w-[162px] shrink-0 overflow-hidden rounded-md border font-mono text-[9px] shadow-sm" style={shellStyle}>
      <div className="flex items-center gap-1 border-b px-2 py-1" style={surfaceStyle}>
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.danger }} />
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.primary }} />
        <span className="size-1.5 rounded-full" style={{ backgroundColor: palette.success }} />
      </div>
      <div className="space-y-2 px-3 py-2">
        <div style={{ color: palette.muted }}>$ gold-band run</div>
        <div><span style={{ color: palette.primary }}>workflow</span> ready</div>
        <div style={{ color: palette.success }}>verify passed</div>
        <div className="mt-3 h-3 w-0.5 animate-pulse" style={{ backgroundColor: palette.primary }} />
      </div>
    </div>
  );
}
