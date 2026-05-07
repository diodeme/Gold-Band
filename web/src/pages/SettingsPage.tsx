import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DesktopLanguage, DesktopThemePreference, PreferencesVm } from '../types';
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

      <AppCard>
        <CardHeader>
          <CardTitle>{t('settings.appearance')}</CardTitle>
        </CardHeader>
        <CardContent className="flex gap-2">
          {(['light', 'dark', 'system'] as DesktopThemePreference[]).map((value) => (
            <Button className={cn('min-w-28 capitalize', theme === value && 'border-primary bg-primary/15 text-primary')} variant="outline" key={value} onClick={() => chooseTheme(value)}>
              {t(`settings.${value}`)}
            </Button>
          ))}
        </CardContent>
      </AppCard>

      <AppCard>
        <CardHeader>
          <CardTitle>{t('settings.language')}</CardTitle>
        </CardHeader>
        <CardContent>
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
