import { useEffect, useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { AppearancePreference, AppInfoVm, AvatarKind, AvatarPreferencesVm, AvatarShape, ColorSchemePreference, DesktopLanguage, MetricsSettingsVm, PersonalizationPreference, PreferencesVm, ResolvedColorScheme, SaveDesktopAvatarInput, UpdateInfoVm, UpdateStatusVm, UpdaterSettingsVm, VisualQuality, WallpaperPreferencesVm } from '../types';
import {
  appearanceWithQuality,
  appearanceWithTheme,
  applyAppearance,
  applyPersonalization,
  desktopFontOptions,
  desktopEditorFontOptions,
  desktopTypography,
  fontFamilyForStack,
  editorFontFamilyForStack,
  getThemePackage,
  resolveAppearance,
  normalizeFontFamilies,
  moveFontFamily,
  themeFontStackDisplayName,
  toggleFontFamily,
  themePackageSummaries,
  type DesktopFontOption,
  type ThemePreviewPalette,
} from '../theme';
import { AppCard } from '@/components/AppCard';
import { Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Badge } from '@/components/ui/badge';
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from '@/components/ui/sheet';
import { ArrowDown, ArrowUp, Check, ChevronsUpDown, ChevronDown, CircleHelp, Loader2, Pencil, RotateCcw, Save, X } from 'lucide-react';
import { getMetricsSettings, getSystemFonts, saveMetricsSettings } from '../api';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { cn } from '@/lib/utils';
import { useThemeWallpaperSurface } from '@/components/theme/ThemeAssetsContext';
import { formatLocalDateTime } from '@/lib/datetime';
import { normalizeFontCatalogFamilies } from '@/lib/font-families';
import { ScheduledRuntimeSettings } from '@/components/scheduled-tasks/ScheduledRuntimeSettings';
import { AvatarSettings } from '@/components/settings/AvatarSettings';
import { WallpaperSettings } from '@/components/settings/WallpaperSettings';
import { useWebviewMeasuredContainer } from '@/hooks/use-webview-measured-container';

type TypographySection = 'ui' | 'editor';

const typographyDisclosureSessionKey = 'gold-band:settings:typography-disclosure:v1';

function effectiveTypographySize(appearance: AppearancePreference, personalization: PersonalizationPreference, kind: TypographySection) {
  const preference = personalization.typography[kind].fontSize;
  if (preference.source === 'custom') return preference.px;
  return resolveAppearance(appearance).typography[kind].size;
}

function withTypographySize(
  personalization: PersonalizationPreference,
  kind: TypographySection,
  fontSize: PersonalizationPreference['typography']['ui']['fontSize'],
): PersonalizationPreference {
  return {
    ...personalization,
    typography: {
      ...personalization.typography,
      [kind]: { ...personalization.typography[kind], fontSize },
    },
  };
}

function initialTypographyDisclosure(): Record<TypographySection, boolean> {
  try {
    const saved = JSON.parse(window.sessionStorage.getItem(typographyDisclosureSessionKey) ?? '{}') as Partial<Record<TypographySection, boolean>>;
    return { ui: saved.ui ?? true, editor: saved.editor ?? false };
  } catch {
    return { ui: true, editor: false };
  }
}

interface SettingsPageProps {
  preferences: PreferencesVm;
  appInfo: AppInfoVm;
  updaterSettings: UpdaterSettingsVm;
  updateStatus: UpdateStatusVm;
  availableUpdate?: UpdateInfoVm | null;
  showAdvancedUpdateDot: boolean;
  showUpdatesSectionDot: boolean;
  downloadProgress: { downloaded: number; total: number | null } | null;
  clientVersion: string;
  busy: boolean;
  initialTab?: 'general' | 'appearance' | 'advanced';
  onSave: (appearance: AppearancePreference, personalization: PersonalizationPreference, language: DesktopLanguage, useLocalClaude: boolean, verboseLogging: boolean) => void;
  onSaveAvatar: (input: SaveDesktopAvatarInput) => Promise<AvatarPreferencesVm | undefined>;
  onSelectRecentAvatar: (kind: AvatarKind, avatarId: string) => Promise<AvatarPreferencesVm | undefined>;
  onSaveAvatarShape: (kind: AvatarKind, shape: AvatarShape | null) => Promise<AvatarPreferencesVm | undefined>;
  onClearAvatar: (kind: AvatarKind) => Promise<AvatarPreferencesVm | undefined>;
  onImportWallpaper: (colorScheme: ResolvedColorScheme) => Promise<WallpaperPreferencesVm | undefined>;
  onSelectRecentWallpaper: (colorScheme: ResolvedColorScheme, wallpaperId: string) => Promise<WallpaperPreferencesVm | undefined>;
  onSaveWallpaperOpacity: (colorScheme: ResolvedColorScheme, opacityPercent: number) => Promise<WallpaperPreferencesVm | undefined>;
  onRestoreThemeWallpaper: (colorScheme: ResolvedColorScheme) => Promise<WallpaperPreferencesVm | undefined>;
  metricsSettings?: MetricsSettingsVm | null;
  onSaveMetricsSettings?: (enabled: boolean, metricsBaseUrl: string | null, apiKey: string | null) => Promise<MetricsSettingsVm | undefined>;
  onSaveUpdaterSettings: (overrideUrl: string | null) => Promise<UpdaterSettingsVm | undefined>;
  onCheckUpdate: () => Promise<UpdateStatusVm | undefined>;
  onInstallUpdate: () => Promise<void>;
  onViewSettings: () => Promise<void> | void;
  onViewAdvanced: () => Promise<void> | void;
}

export function SettingsPage({ preferences, appInfo, updaterSettings, metricsSettings = null, onSaveMetricsSettings, updateStatus, availableUpdate = null, showAdvancedUpdateDot, showUpdatesSectionDot, downloadProgress, clientVersion, busy, initialTab, onSave, onSaveAvatar, onSelectRecentAvatar, onSaveAvatarShape, onClearAvatar, onImportWallpaper, onSelectRecentWallpaper, onSaveWallpaperOpacity, onRestoreThemeWallpaper, onSaveUpdaterSettings, onCheckUpdate, onInstallUpdate, onViewSettings, onViewAdvanced }: SettingsPageProps) {
  const measuredThemeDrawerRef = useWebviewMeasuredContainer<HTMLDivElement>('theme-drawer');
  useThemeWallpaperSurface();
  const { t } = useTranslation();
  const [appearance, setAppearance] = useState(preferences.appearance);
  const [personalization, setPersonalization] = useState(preferences.personalization);
  const [language, setLanguage] = useState(preferences.language);
  const [uiFontSize, setUiFontSize] = useState(() => effectiveTypographySize(preferences.appearance, preferences.personalization, 'ui'));
  const [editorFontSize, setEditorFontSize] = useState(() => effectiveTypographySize(preferences.appearance, preferences.personalization, 'editor'));
  const useLocalClaude = false;
  const [verboseLogging, setVerboseLogging] = useState(preferences.verboseLogging);
  const [systemFonts, setSystemFonts] = useState<string[]>([]);
  const [themeSheetOpen, setThemeSheetOpen] = useState(false);
  const [typographyDisclosure, setTypographyDisclosure] = useState(initialTypographyDisclosure);
  const [updaterOverrideUrl, setUpdaterOverrideUrl] = useState(updaterSettings.overrideUrl ?? '');
  const [editingUpdaterUrl, setEditingUpdaterUrl] = useState(false);
  const [activeTab, setActiveTab] = useState<'general' | 'appearance' | 'advanced'>(initialTab ?? 'general');

  useEffect(() => setAppearance(preferences.appearance), [preferences.appearance]);
  useEffect(() => setPersonalization(preferences.personalization), [preferences.personalization]);
  useEffect(() => setLanguage(preferences.language), [preferences.language]);
  useEffect(() => setUiFontSize(effectiveTypographySize(preferences.appearance, preferences.personalization, 'ui')), [preferences.appearance, preferences.personalization]);
  useEffect(() => setEditorFontSize(effectiveTypographySize(preferences.appearance, preferences.personalization, 'editor')), [preferences.appearance, preferences.personalization]);
  useEffect(() => setVerboseLogging(preferences.verboseLogging), [preferences.verboseLogging]);
  useEffect(() => setUpdaterOverrideUrl(updaterSettings.overrideUrl ?? ''), [updaterSettings.overrideUrl]);

  // ── Metrics ──
  const [metricsEnabled, setMetricsEnabled] = useState(metricsSettings?.enabled ?? false);
  const [metricsBaseUrl, setMetricsBaseUrl] = useState(metricsSettings?.metricsBaseUrl ?? '');
  const [metricsApiKey, setMetricsApiKey] = useState('');
  const [metricsSaving, setMetricsSaving] = useState(false);
  useEffect(() => {
    setMetricsEnabled(metricsSettings?.enabled ?? false);
    setMetricsBaseUrl(metricsSettings?.metricsBaseUrl ?? '');
  }, [metricsSettings?.enabled, metricsSettings?.metricsBaseUrl]);

  async function handleSaveMetrics() {
    setMetricsSaving(true);
    try {
      if (onSaveMetricsSettings) {
        const saved = await onSaveMetricsSettings(metricsEnabled, metricsBaseUrl || null, metricsApiKey || null);
        if (saved) {
          setMetricsEnabled(saved.enabled);
          setMetricsBaseUrl(saved.metricsBaseUrl ?? '');
        }
      } else {
        // Fallback: save directly via barrel API
        const saved = await saveMetricsSettings(metricsEnabled, metricsBaseUrl || null, metricsApiKey || null);
        if (saved) {
          setMetricsEnabled(saved.enabled);
          setMetricsBaseUrl(saved.metricsBaseUrl ?? '');
        }
      }
    } finally {
      setMetricsSaving(false);
    }
  }

  // Load metrics settings if not provided via props
  useEffect(() => {
    if (metricsSettings) return;
    getMetricsSettings().then((s) => {
      if (s) {
        setMetricsEnabled(s.enabled);
        setMetricsBaseUrl(s.metricsBaseUrl ?? '');
      }
    }).catch(() => {});
  }, [metricsSettings]);

  useEffect(() => {
    getSystemFonts().then(setSystemFonts).catch(() => setSystemFonts([]));
  }, []);

  useEffect(() => {
    void onViewSettings();
  }, [onViewSettings]);

  useEffect(() => {
    if (activeTab !== 'advanced') return;
    void onViewAdvanced();
  }, [activeTab, onViewAdvanced]);

  const saveAppearance = (next: AppearancePreference) => {
    setAppearance(next);
    applyAppearance(next);
    applyPersonalization(personalization);
    onSave(next, personalization, language, useLocalClaude, verboseLogging);
  };

  const chooseThemeFromSheet = (themeId: string) => {
    saveAppearance(appearanceWithTheme(appearance, themeId));
    setThemeSheetOpen(false);
  };

  const chooseLanguage = (value: DesktopLanguage) => {
    setLanguage(value);
    onSave(appearance, personalization, value, useLocalClaude, verboseLogging);
  };

  const chooseFontStack = (kind: TypographySection, families: readonly string[]) => {
    const next = withTypographyFontStack(personalization, kind, families);
    setPersonalization(next);
    applyPersonalization(next);
    onSave(appearance, next, language, useLocalClaude, verboseLogging);
  };

  const chooseTypographySize = (kind: 'ui' | 'editor', value: number) => {
    const next = withTypographySize(personalization, kind, { source: 'custom', px: value });
    setPersonalization(next);
    setUiFontSize(effectiveTypographySize(appearance, next, 'ui'));
    setEditorFontSize(effectiveTypographySize(appearance, next, 'editor'));
    applyPersonalization(next);
    onSave(appearance, next, language, useLocalClaude, verboseLogging);
  };

  const previewTypographySize = (kind: 'ui' | 'editor', value: number) => {
    if (kind === 'ui') setUiFontSize(value); else setEditorFontSize(value);
    applyPersonalization(withTypographySize(personalization, kind, { source: 'custom', px: value }));
  };

  const resetTypographySize = (kind: 'ui' | 'editor') => {
    const next = withTypographySize(personalization, kind, { source: 'theme' });
    setPersonalization(next);
    setUiFontSize(effectiveTypographySize(appearance, next, 'ui'));
    setEditorFontSize(effectiveTypographySize(appearance, next, 'editor'));
    applyPersonalization(next);
    onSave(appearance, next, language, useLocalClaude, verboseLogging);
  };


  const saveUpdaterOverride = async () => {
    const saved = await onSaveUpdaterSettings(updaterOverrideUrl);
    if (saved) {
      setUpdaterOverrideUrl(saved.overrideUrl ?? '');
      setEditingUpdaterUrl(false);
    }
  };

  const resetUpdaterOverride = async () => {
    setUpdaterOverrideUrl('');
    const saved = await onSaveUpdaterSettings(null);
    if (saved) setEditingUpdaterUrl(false);
  };

  const installedFontOptions = useMemo(() => {
    const presetIds = new Set<string>([...desktopFontOptions, ...desktopEditorFontOptions].map((option) => option.id));
    return systemFonts.filter((family) => !presetIds.has(family));
  }, [systemFonts]);

  const setTypographySectionOpen = (section: TypographySection, open: boolean) => {
    setTypographyDisclosure((current) => {
      const next = { ...current, [section]: open };
      window.sessionStorage.setItem(typographyDisclosureSessionKey, JSON.stringify(next));
      return next;
    });
  };

  const effectiveAppearance = resolveAppearance(appearance);
  const themeWallpapersByColorScheme = useMemo(() => ({
    light: resolveAppearance({ ...appearance, colorScheme: 'light' }).wallpapers.settings,
    dark: resolveAppearance({ ...appearance, colorScheme: 'dark' }).wallpapers.settings,
  }), [appearance]);
  const currentTheme = getThemePackage(appearance.themeId);
  const defaultUiFontDisplayName = themeFontStackDisplayName(
    effectiveAppearance.themeId,
    effectiveAppearance.scheme.typography.uiStackId,
    language,
  );
  const defaultEditorFontDisplayName = themeFontStackDisplayName(
    effectiveAppearance.themeId,
    effectiveAppearance.scheme.typography.editorStackId,
    language,
  );
  const currentThemeSummary = themePackageSummaries.find(({ id }) => id === effectiveAppearance.themeId)
    ?? themePackageSummaries[0];
  const defaultFontOption = desktopFontOptions[0];
  const selectedUiFonts = personalization.typography.ui.fontStack.source === 'custom' ? personalization.typography.ui.fontStack.families : [];
  const defaultEditorFontOption = desktopEditorFontOptions[0];
  const selectedEditorFonts = personalization.typography.editor.fontStack.source === 'custom' ? personalization.typography.editor.fontStack.families : [];

  return (
    <Page flush className="flex flex-col" data-theme-wallpaper-slot="settings">
      <PageHeader
        title={<span className="text-title">{t('settings.title')}</span>}
      />

      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto p-5 xl:p-6">
        <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as 'general' | 'appearance' | 'advanced')} className="space-y-4">
        <TabsList className="grid w-full max-w-md grid-cols-3">
          <TabsTrigger value="general">{t('settings.tabs.general')}</TabsTrigger>
          <TabsTrigger value="appearance">{t('settings.tabs.appearance')}</TabsTrigger>
          <TabsTrigger value="advanced">
            <span className="inline-flex items-center gap-2">
              <span>{t('settings.tabs.advanced')}</span>
              {showAdvancedUpdateDot ? <UpdateDot /> : null}
            </span>
          </TabsTrigger>
        </TabsList>

        <TabsContent value="general" className="m-0">
          <AppCard className="gap-0 overflow-hidden py-0">
            <SettingsSection title={t('settings.language')}>
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
            <SettingsSection title={t('scheduled.settings.title')} divided>
              <ScheduledRuntimeSettings />
            </SettingsSection>
          </AppCard>
        </TabsContent>

        <TabsContent value="appearance" className="m-0">
          <AppCard className="gap-0 overflow-hidden py-0">
            <SettingsSection title={t('settings.appearance')}>
              <div className="space-y-4">
                <Sheet open={themeSheetOpen} onOpenChange={setThemeSheetOpen}>
                  <CurrentThemeSummary
                    summary={currentThemeSummary}
                    scheme={effectiveAppearance.colorScheme}
                  />
                  <SheetContent
                    className="overflow-hidden"
                    resizeStorageKey="settings/theme-package-drawer"
                    defaultSize={760}
                    minSize={480}
                    maxSize={980}
                    closeLabel={t('common.close')}
                  >
                    <SheetHeader className="border-b px-5 py-4">
                      <SheetTitle>{t('settings.themeDrawerTitle')}</SheetTitle>
                    </SheetHeader>
                    <div ref={measuredThemeDrawerRef} className="@container/theme-drawer min-h-0 flex-1 overflow-y-auto p-5">
                      <div className="grid gap-3 @2xl/theme-drawer:grid-cols-2">
                        {themePackageSummaries.map((summary) => (
                          <ThemePackageCard
                            key={summary.id}
                            summary={summary}
                            selected={appearance.themeId === summary.id}
                            scheme={effectiveAppearance.colorScheme}
                            onSelect={() => chooseThemeFromSheet(summary.id)}
                          />
                        ))}
                      </div>
                    </div>
                  </SheetContent>
                </Sheet>
                <div className="flex flex-wrap items-center justify-between gap-4 rounded-lg border border-border/45 p-3">
                  <div className="min-w-0">
                    <div className="text-sm font-semibold">{t('settings.colorScheme')}</div>
                    <div className="text-xs text-muted-foreground">{t('settings.colorSchemeDescription')}</div>
                  </div>
                  <Select value={appearance.colorScheme} onValueChange={(value) => saveAppearance({ ...appearance, colorScheme: value as ColorSchemePreference })}>
                    <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="system">{t('settings.schemeSystem')}</SelectItem>
                      <SelectItem value="light">{t('settings.schemeLight')}</SelectItem>
                      <SelectItem value="dark">{t('settings.schemeDark')}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                {currentTheme.visualQualityProfiles ? (
                  <div className="flex flex-wrap items-center justify-between gap-4 rounded-lg border border-border/45 p-3">
                    <div className="min-w-0">
                      <div className="text-sm font-semibold">{t('settings.visualQuality')}</div>
                      <div className="text-xs text-muted-foreground">{t('settings.visualQualityDescription')}</div>
                    </div>
                    <Select value={effectiveAppearance.visualQuality} onValueChange={(value) => saveAppearance(appearanceWithQuality(appearance, value as VisualQuality))}>
                      <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectItem value="full">{t('settings.visualQualityFull')}</SelectItem>
                        <SelectItem value="performance">{t('settings.visualQualityPerformance')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                ) : null}
              </div>
            </SettingsSection>

            <SettingsSection title={t('settings.typography')} divided>
              <div className="max-w-2xl divide-y divide-border/45 overflow-hidden rounded-lg border border-border/45">
                <TypographyDisclosure
                  title={t('settings.uiTypography')}
                  description={t('settings.uiTypographyDescription')}
                  open={typographyDisclosure.ui}
                  onOpenChange={(open) => setTypographySectionOpen('ui', open)}
                >
                  <TypographySizeSetting
                    label={t('settings.uiFontSize')}
                    description={t('settings.uiFontSizeDescription')}
                    value={uiFontSize}
                    min={desktopTypography.ui.min}
                    max={desktopTypography.ui.max}
                    onChange={(value) => previewTypographySize('ui', value)}
                    onCommit={(value) => chooseTypographySize('ui', value)}
                    onReset={() => resetTypographySize('ui')}
                  />
                  <FontPreferenceSetting
                    defaultOption={defaultFontOption}
                    defaultDisplayName={defaultUiFontDisplayName}
                    installedFontOptions={installedFontOptions}
                    selectedFonts={selectedUiFonts}
                    sample="Gold-Band / 优化 resume 会话 / 0123"
                    onChange={(families) => chooseFontStack('ui', families)}
                  />
                </TypographyDisclosure>
                <TypographyDisclosure
                  title={t('settings.editorTypography')}
                  description={t('settings.editorTypographyDescription')}
                  open={typographyDisclosure.editor}
                  onOpenChange={(open) => setTypographySectionOpen('editor', open)}
                >
                  <TypographySizeSetting
                    label={t('settings.editorFontSize')}
                    description={t('settings.editorFontSizeDescription')}
                    value={editorFontSize}
                    min={desktopTypography.editor.min}
                    max={desktopTypography.editor.max}
                    onChange={(value) => previewTypographySize('editor', value)}
                    onCommit={(value) => chooseTypographySize('editor', value)}
                    onReset={() => resetTypographySize('editor')}
                  />
                  <FontPreferenceSetting
                    defaultOption={defaultEditorFontOption}
                    defaultDisplayName={defaultEditorFontDisplayName}
                    installedFontOptions={installedFontOptions}
                    selectedFonts={selectedEditorFonts}
                    sample={'const workflow = "AI";'}
                    onChange={(families) => chooseFontStack('editor', families)}
                    monospace
                  />
                </TypographyDisclosure>
              </div>
            </SettingsSection>

            <SettingsSection title={t('settings.wallpaper.title')} divided>
              <WallpaperSettings
                preferences={preferences.wallpapers}
                personalization={personalization.wallpaper}
                activeColorScheme={effectiveAppearance.colorScheme}
                themeWallpapersByColorScheme={themeWallpapersByColorScheme}
                busy={busy}
                onImportWallpaper={onImportWallpaper}
                onSelectRecentWallpaper={onSelectRecentWallpaper}
                onSaveWallpaperOpacity={onSaveWallpaperOpacity}
                onRestoreThemeWallpaper={onRestoreThemeWallpaper}
              />
            </SettingsSection>

            <SettingsSection title={t('settings.avatar.title')} divided>
              <AvatarSettings
                preferences={preferences.avatars}
                personalization={personalization.avatars}
                busy={busy}
                onSaveAvatar={onSaveAvatar}
                onSelectRecentAvatar={onSelectRecentAvatar}
                onSaveAvatarShape={onSaveAvatarShape}
                onClearAvatar={onClearAvatar}
              />
            </SettingsSection>
          </AppCard>
        </TabsContent>

        <TabsContent value="advanced" className="m-0">
          <AppCard className="gap-0 overflow-hidden py-0">
            <SettingsSection title={t('settings.advanced')}>
              <div className="flex items-center gap-3 py-2">
                <div className="text-sm font-medium text-muted-foreground">{t('settings.verboseLogging.label')}</div>
                <SettingInfoTooltip content={t('settings.verboseLogging.description')} />
                <button
                  type="button"
                  role="switch"
                  aria-checked={verboseLogging}
                  className={cn(
                    'relative h-6 w-11 shrink-0 overflow-hidden rounded-full border p-0.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                    verboseLogging ? 'border-primary bg-primary' : 'border-border/70 bg-muted-foreground/20',
                  )}
                  onClick={() => {
                    const next = !verboseLogging;
                    setVerboseLogging(next);
                    onSave(appearance, personalization, language, useLocalClaude, next);
                  }}
                >
                  <span
                    className={cn(
                      'block size-5 rounded-full bg-background shadow-sm transition-transform',
                      verboseLogging && 'translate-x-5',
                    )}
                  />
                </button>
              </div>
            </SettingsSection>
            <SettingsSection title={<span className="inline-flex items-center gap-2">{t('settings.updater.title')}{showUpdatesSectionDot ? <UpdateDot /> : null}</span>}>
              <div className="max-w-4xl space-y-3">
                <div className="flex items-center gap-3">
                  <div className="w-28 shrink-0 text-sm font-medium text-muted-foreground">{t('settings.updater.currentUrl')}</div>
                  {editingUpdaterUrl ? (
                    <Input
                      value={updaterOverrideUrl}
                      placeholder={t('settings.updater.overridePlaceholder')}
                      className="h-9 min-w-0 flex-1 font-mono text-xs"
                      onChange={(event) => setUpdaterOverrideUrl(event.target.value)}
                    />
                  ) : (
                    <div className="min-w-0 break-all font-mono text-xs text-foreground">{updaterSettings.effectiveUrl}</div>
                  )}
                  <div className="ml-auto flex shrink-0 items-center gap-1">
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span>
                          <Button
                            size="icon"
                            variant="ghost"
                            className="size-8"
                            disabled={busy}
                            onClick={() => editingUpdaterUrl ? void saveUpdaterOverride() : setEditingUpdaterUrl(true)}
                          >
                            {editingUpdaterUrl ? <Save className="size-4" /> : <Pencil className="size-4" />}
                          </Button>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent side="top" className="max-w-64 whitespace-pre-wrap break-words text-xs">{editingUpdaterUrl ? t('settings.updater.saveOverride') : t('settings.updater.editUrl')}</TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span>
                          <Button
                            size="icon"
                            variant="ghost"
                            className="size-8"
                            disabled={busy}
                            onClick={() => void resetUpdaterOverride()}
                          >
                            <RotateCcw className="size-4" />
                          </Button>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent side="top" className="max-w-64 whitespace-pre-wrap break-words text-xs">{t('settings.updater.resetToBuiltIn')}</TooltipContent>
                    </Tooltip>
                  </div>
                </div>
                <div className="flex">
                  <div className="w-28 shrink-0" />
                  <UpdateStatusInline status={updateStatus} availableUpdate={availableUpdate} busy={busy} downloadProgress={downloadProgress} onCheckUpdate={onCheckUpdate} onInstallUpdate={onInstallUpdate} />
                </div>
              </div>
            </SettingsSection>
            {/* Metrics reporting section — always visible from desktop API */}
              <SettingsSection title={t('settings.metrics.title')} divided>
                <div className="max-w-4xl space-y-3">
                  <div className="flex items-center gap-3">
                    <p className="text-sm font-medium text-muted-foreground">{t('settings.metrics.enable')}</p>
                    <SettingInfoTooltip content={t('settings.metrics.enableDescription')} />
                    <button
                      type="button"
                      role="switch"
                      aria-checked={metricsEnabled}
                      disabled={metricsSettings?.toggleLocked}
                      className={cn(
                        'relative h-6 w-11 shrink-0 overflow-hidden rounded-full border p-0.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                        metricsEnabled ? 'border-primary bg-primary' : 'border-border/70 bg-muted-foreground/20',
                        metricsSettings?.toggleLocked && 'cursor-not-allowed opacity-60',
                      )}
                      onClick={() => setMetricsEnabled(!metricsEnabled)}
                    >
                      <span className={cn('block size-5 rounded-full bg-background shadow-sm transition-transform', metricsEnabled && 'translate-x-5')} />
                    </button>
                  </div>
                  {metricsEnabled && (
                    <>
                      <div className="space-y-1">
                        <div className="text-xs font-medium text-muted-foreground">{t('settings.metrics.baseUrl')}</div>
                        <Input value={metricsBaseUrl} placeholder="http://..." disabled={metricsSettings?.toggleLocked} className="h-9 min-w-0 font-mono text-xs" onChange={(event) => setMetricsBaseUrl(event.target.value)} />
                      </div>
                      <div className="space-y-1">
                        <div className="text-xs font-medium text-muted-foreground">{t('settings.metrics.apiKey')}</div>
                        <Input type="password" value={metricsApiKey} placeholder={metricsSettings?.apiKeySet ? t('settings.metrics.apiKeySet') : 'API Key'} disabled={metricsSettings?.toggleLocked} className="h-9 min-w-0 font-mono text-xs" onChange={(event) => setMetricsApiKey(event.target.value)} />
                      </div>
                      <div className="flex justify-end">
                        <Button size="sm" onClick={() => void handleSaveMetrics()} disabled={metricsSaving}>{metricsSaving ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}{t('settings.metrics.save')}</Button>
                      </div>
                    </>
                  )}
                </div>
              </SettingsSection>
          </AppCard>
        </TabsContent>
      </Tabs>

      {clientVersion ? <Badge variant="outline" className="font-mono text-muted-foreground"><span className="mr-2 size-2 rounded-full bg-gold-success" /> {t('settings.clientVersion', { version: clientVersion })}</Badge> : null}
      </div>
    </Page>
  );
}

function UpdateStatusInline({ status, availableUpdate, busy, downloadProgress, onCheckUpdate, onInstallUpdate }: { status: UpdateStatusVm; availableUpdate?: UpdateInfoVm | null; busy: boolean; downloadProgress: { downloaded: number; total: number | null } | null; onCheckUpdate: () => Promise<UpdateStatusVm | undefined>; onInstallUpdate: () => Promise<void> }) {
  const { t } = useTranslation();
  const resolvedUpdate = status.update ?? availableUpdate ?? null;
  const effectiveStatus = status.status === 'idle' && resolvedUpdate ? 'available' : status.status;
  const downloading = effectiveStatus === 'downloading';
  const statusClass = effectiveStatus === 'available' || effectiveStatus === 'downloading'
    ? 'text-gold-success'
    : effectiveStatus === 'error'
      ? 'text-destructive'
      : 'text-muted-foreground';
  const progressPct = downloadProgress && downloadProgress.total ? Math.min(100, Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)) : 0;
  const hasProgress = downloadProgress && downloadProgress.downloaded > 0;
  const hasTotal = downloadProgress && downloadProgress.total != null;
  const hasResultRow = resolvedUpdate !== null || status.status !== 'idle' || !!status.error;
  return (
    <div className="min-w-0 flex-1 space-y-1.5">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-sm">
        <Button size="sm" variant="secondary" onClick={() => void onCheckUpdate()} disabled={busy || status.status === 'checking'}>{status.status === 'checking' ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}{t('settings.updater.checkNow')}</Button>
        {status.checkedAt ? <span className="text-xs text-muted-foreground">{t('settings.updater.lastCheckedAt', { time: formatCheckedAt(status.checkedAt) })}</span> : null}
      </div>
      {hasResultRow ? (
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-sm">
          <span className={cn('font-medium', statusClass)}>{t(`settings.updater.status.${effectiveStatus}`)}</span>
          {resolvedUpdate ? <span className="font-mono text-xs text-muted-foreground">{resolvedUpdate.currentVersion} → <span className="text-destructive">{resolvedUpdate.version}</span></span> : null}
          {downloading ? (
            <Button size="sm" disabled><Loader2 className="mr-1.5 size-3.5 animate-spin" />{t('settings.updater.status.downloading')}</Button>
          ) : effectiveStatus === 'available' ? (
            <Button size="sm" onClick={() => void onInstallUpdate()} disabled={busy}>{t('settings.updater.install')}</Button>
          ) : null}
          {status.error ? <span className="text-xs text-destructive">{t(`errors.${status.error.code}`, status.error.params)}</span> : null}
        </div>
      ) : null}
      {downloading && hasProgress ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          {hasTotal ? (
            <>
              <div className="h-1.5 max-w-80 flex-1 rounded-full bg-secondary">
                <div className="h-full rounded-full bg-gold-success transition-all duration-300" style={{ width: `${progressPct}%` }} />
              </div>
              <span className="shrink-0 tabular-nums">{formatBytes(downloadProgress!.downloaded)} / {formatBytes(downloadProgress!.total!)}</span>
            </>
          ) : (
            <span className="tabular-nums">{formatBytes(downloadProgress!.downloaded)} downloaded</span>
          )}
        </div>
      ) : null}
    </div>
  );
}

function UpdateDot() {
  return <span className="size-2 rounded-full bg-destructive" aria-hidden="true" />;
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatCheckedAt(value: string) {
  return formatLocalDateTime(value, value);
}

function SettingsSection({ title, children, divided = false }: { title: ReactNode; children: ReactNode; divided?: boolean }) {
  const measuredSectionRef = useWebviewMeasuredContainer<HTMLElement>('settings-section');
  const measuredContentRef = useWebviewMeasuredContainer<HTMLDivElement>('settings-content');
  return (
    <section ref={measuredSectionRef} className={cn('@container/settings-section', divided && 'border-t border-border/45')}>
      <div className="grid gap-4 px-5 py-5 @3xl/settings-section:grid-cols-[160px_minmax(0,1fr)]">
        <h2 className="text-base font-semibold text-foreground">{title}</h2>
        <div ref={measuredContentRef} className="@container/settings-content min-w-0 space-y-4">{children}</div>
      </div>
    </section>
  );
}

function SettingInfoTooltip({ content }: { content: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" className="inline-flex size-4 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <CircleHelp className="size-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent align="start" side="top" sideOffset={8} className="max-w-64 whitespace-pre-wrap break-words text-xs leading-5">
        {content}
      </TooltipContent>
    </Tooltip>
  );
}

interface ThemePackageCardProps {
  summary: (typeof themePackageSummaries)[number];
  selected: boolean;
  scheme: 'light' | 'dark';
  onSelect: () => void;
}

function withTypographyFontStack(
  personalization: PersonalizationPreference,
  kind: TypographySection,
  families: readonly string[],
): PersonalizationPreference {
  const normalized = normalizeFontFamilies(families);
  return {
    ...personalization,
    schemaVersion: 4,
    typography: {
      ...personalization.typography,
      [kind]: {
        ...personalization.typography[kind],
        fontStack: normalized.length === 0
          ? { source: 'theme' as const }
          : { source: 'custom' as const, families: normalized },
      },
    },
  };
}

function CurrentThemeSummary({ summary, scheme }: {
  summary: (typeof themePackageSummaries)[number];
  scheme: 'light' | 'dark';
}) {
  const measuredSummaryRef = useWebviewMeasuredContainer<HTMLDivElement>('theme-summary');
  const { i18n, t } = useTranslation();
  const language = i18n.resolvedLanguage?.startsWith('zh') ? 'zh-CN' : 'en';
  return (
    <div ref={measuredSummaryRef} className="@container/theme-summary">
      <div className="grid gap-3 rounded-lg border border-border/35 bg-transparent p-3 @lg/theme-summary:grid-cols-[auto_minmax(0,1fr)] @lg/theme-summary:items-center @xl/theme-summary:grid-cols-[auto_minmax(0,1fr)_auto]">
        <TerminalPreview palette={summary.preview[scheme]} compact />
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-muted-foreground">{t('settings.currentTheme')}</span>
            <Badge variant="outline" className="px-1.5 py-0 text-ui-micro">{t('settings.activeTheme')}</Badge>
          </div>
          <div className="truncate text-base font-semibold text-foreground">{summary.name[language]}</div>
        </div>
        <SheetTrigger asChild>
          <Button variant="outline" className="w-full @lg/theme-summary:col-span-2 @xl/theme-summary:col-span-1 @xl/theme-summary:w-auto">
            {t('settings.chooseTheme')}
          </Button>
        </SheetTrigger>
      </div>
    </div>
  );
}

function ThemePackageCard({ summary, selected, scheme, onSelect }: ThemePackageCardProps) {
  const { i18n, t } = useTranslation();
  const language = i18n.resolvedLanguage?.startsWith('zh') ? 'zh-CN' : 'en';
  return (
    <button
      type="button"
      aria-pressed={selected}
      className={cn(
        'group flex min-w-0 flex-col gap-3 rounded-xl border border-border/45 bg-card p-3 text-left transition-[background-color,border-color,box-shadow] hover:border-primary/35 hover:bg-accent/20 hover:shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
        selected && 'border-primary/45 bg-primary/[0.045] ring-1 ring-inset ring-primary/15',
      )}
      onClick={onSelect}
    >
      <TerminalPreview palette={summary.preview[scheme]} compact />
      <div className="flex min-w-0 w-full items-center justify-between gap-2">
        <span className="truncate text-sm font-semibold text-foreground">{summary.name[language]}</span>
        {selected ? (
          <span className="inline-flex items-center gap-1 rounded-full bg-primary px-2 py-0.5 text-ui-micro font-medium text-primary-foreground">
            <Check className="size-3" aria-hidden="true" />
            {t('settings.activeTheme')}
          </span>
        ) : null}
      </div>
    </button>
  );
}

function FontPreviewSample({ sample, fontFamily }: { sample: string; fontFamily: string }) {
  const { t } = useTranslation();
  const [leading, trailing] = sample.split(' / ');
  return (
    <div className="mt-3 rounded-md border border-border/35 bg-background/60 px-3 py-2">
      <div className="text-ui-caption font-medium text-muted-foreground">{t('settings.fontPreview')}</div>
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

function TypographySizeSetting({
  label,
  description,
  value,
  min,
  max,
  onChange,
  onCommit,
  onReset,
}: {
  label: string;
  description: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
  onReset: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 flex-wrap items-center justify-between gap-4 py-3">
      <div className="min-w-0 space-y-1">
        <div className="text-sm font-medium text-foreground">{label}</div>
        <div className="text-xs text-muted-foreground">{description}</div>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          <Input
            aria-label={label}
            className="h-9 w-20 text-center tabular-nums"
            inputMode="numeric"
            max={max}
            min={min}
            step={1}
            type="number"
            value={value}
            onChange={(event) => {
              const next = event.currentTarget.valueAsNumber;
              if (Number.isFinite(next)) onChange(next);
            }}
            onBlur={(event) => {
              const next = event.currentTarget.valueAsNumber;
              if (Number.isFinite(next)) onCommit(next);
            }}
          />
          <span>px</span>
        </label>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" variant="ghost" size="icon-sm" aria-label={t('settings.resetFontSize')} onPointerDown={(event) => event.preventDefault()} onClick={onReset}>
              <RotateCcw className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('settings.resetFontSize')}</TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
}

function TypographyDisclosure({ title, description, open, onOpenChange, children }: {
  title: string;
  description: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: ReactNode;
}) {
  return (
    <Collapsible open={open} onOpenChange={onOpenChange}>
      <CollapsibleTrigger asChild>
        <button type="button" className="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
          <span className="min-w-0 flex-1 space-y-0.5">
            <span className="block text-sm font-semibold text-foreground">{title}</span>
            <span className="block text-xs text-muted-foreground">{description}</span>
          </span>
          <ChevronDown className={cn('size-4 shrink-0 text-muted-foreground transition-transform', open && 'rotate-180')} />
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent className="overflow-hidden border-t border-border/40 data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down">
        <div className="space-y-4 px-4 pb-4">
          {children}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function FontPreferenceSetting({ defaultOption, defaultDisplayName, installedFontOptions, selectedFonts, sample, onChange, monospace = false }: {
  defaultOption: DesktopFontOption;
  defaultDisplayName: string;
  installedFontOptions: string[];
  selectedFonts: string[];
  sample: string;
  onChange: (fonts: string[]) => void;
  monospace?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const usingDefault = selectedFonts.length === 0;
  const family = monospace ? editorFontFamilyForStack : fontFamilyForStack;
  const availableFonts = useMemo(
    () => normalizeFontCatalogFamilies([
      ...(monospace ? [] : ['Inter Variable', 'Gold Band MiSans']),
      ...installedFontOptions,
      ...selectedFonts,
    ]),
    [installedFontOptions, monospace, selectedFonts],
  );
  const toggleFont = (font: string) => {
    onChange(toggleFontFamily(selectedFonts, font));
  };
  const moveFont = (index: number, direction: -1 | 1) => {
    onChange(moveFontFamily(selectedFonts, index, direction));
  };
  return (
    <div className="space-y-3">
      <button
        type="button"
        aria-pressed={usingDefault}
        className={cn(
          'w-full rounded-lg border border-border/45 bg-transparent p-3 text-left transition hover:border-primary/60 hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
          usingDefault && 'border-primary/65 bg-primary/[0.07]',
        )}
        onClick={() => onChange([])}
      >
        <div className="text-sm font-semibold">{t(defaultOption.labelKey)} · {defaultDisplayName}</div>
        <FontPreviewSample sample={defaultOption.preview} fontFamily={monospace ? 'var(--gb-theme-editor-font-family)' : 'var(--gb-theme-ui-font-family)'} />
      </button>
      <div className={cn('rounded-lg border border-border/35 bg-transparent p-3', !usingDefault && 'border-primary/45 bg-primary/[0.04]')}>
        <div className="space-y-1">
          <div className="text-sm font-semibold">{t('settings.localFonts')}</div>
          <div className="text-xs text-muted-foreground">{t('settings.localFontsDescription', { count: installedFontOptions.length })}</div>
        </div>
        {selectedFonts.length > 0 ? (
          <div className="mt-3 space-y-1.5" aria-label={t('settings.selectedFontStack')}>
            {selectedFonts.map((font, index) => (
              <div key={font} className="flex min-w-0 items-center gap-2 rounded-md border border-border/40 bg-background px-2 py-1.5">
                <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-ui-caption tabular-nums text-muted-foreground">{index + 1}</span>
                <span className="min-w-0 flex-1 truncate text-sm">{font}</span>
                <FontStackAction label={t('settings.moveFontUp')} disabled={index === 0} onClick={() => moveFont(index, -1)}><ArrowUp /></FontStackAction>
                <FontStackAction label={t('settings.moveFontDown')} disabled={index === selectedFonts.length - 1} onClick={() => moveFont(index, 1)}><ArrowDown /></FontStackAction>
                <FontStackAction label={t('settings.removeFont')} onClick={() => toggleFont(font)}><X /></FontStackAction>
              </div>
            ))}
          </div>
        ) : null}
        <div className="mt-3">
          <Popover open={open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
              <Button variant="outline" className="w-full justify-between font-normal" disabled={availableFonts.length === 0}>
                {t('settings.chooseLocalFont')}
                <ChevronsUpDown className="size-4 text-muted-foreground" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
              <Command>
                <CommandInput placeholder={t('settings.searchFonts')} />
                <CommandList>
                  <CommandEmpty>{t('settings.noFontsFound')}</CommandEmpty>
                  <CommandGroup>
                    {availableFonts.map((font) => {
                      const selected = selectedFonts.includes(font);
                      return (
                        <CommandItem key={font} value={font} onSelect={() => toggleFont(font)}>
                          <Check className={cn('size-4', selected ? 'opacity-100' : 'opacity-0')} />
                          <span className="truncate">{font}</span>
                          {selected ? <span className="ml-auto text-ui-caption text-muted-foreground">{selectedFonts.indexOf(font) + 1}</span> : null}
                        </CommandItem>
                      );
                    })}
                  </CommandGroup>
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        </div>
        {!usingDefault ? <FontPreviewSample sample={sample} fontFamily={family(selectedFonts)} /> : null}
      </div>
    </div>
  );
}

function FontStackAction({ label, disabled = false, onClick, children }: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button type="button" variant="ghost" size="icon" className="size-7 shrink-0" aria-label={label} disabled={disabled} onClick={onClick}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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
