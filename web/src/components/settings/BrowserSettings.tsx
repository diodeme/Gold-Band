import { useTranslation } from 'react-i18next';
import type { BrowserPreferences, BrowserSearchEngine } from '@/types';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';

export function BrowserSettings({
  preferences,
  onChange,
}: {
  preferences: BrowserPreferences;
  onChange: (preferences: BrowserPreferences) => void;
}) {
  const { t } = useTranslation();
  const patch = (next: Partial<BrowserPreferences>) => onChange({ ...preferences, ...next });
  return (
    <div className="divide-y divide-border/40">
      <div className="flex flex-wrap items-center justify-between gap-4 py-3 first:pt-0">
        <div className="min-w-0 space-y-1">
          <div className="text-sm font-medium text-foreground">{t('settings.browser.searchEngine')}</div>
          <div className="text-xs text-muted-foreground">{t('settings.browser.searchEngineDescription')}</div>
        </div>
        <Select value={preferences.searchEngine} onValueChange={(value) => patch({ searchEngine: value as BrowserSearchEngine })}>
          <SelectTrigger className="w-44" aria-label={t('settings.browser.searchEngine')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="baidu">Baidu</SelectItem>
            <SelectItem value="google">Google</SelectItem>
            <SelectItem value="bing">Bing</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <BrowserToggle
        id="browser-open-local-links"
        label={t('settings.browser.openLocalLinks')}
        description={t('settings.browser.openLocalLinksDescription')}
        checked={preferences.openLocalLinksInBrowser}
        onCheckedChange={(checked) => patch({ openLocalLinksInBrowser: checked })}
      />
      <BrowserToggle
        id="browser-open-web-links"
        label={t('settings.browser.openWebLinks')}
        description={t('settings.browser.openWebLinksDescription')}
        checked={preferences.openWebLinksInBrowser}
        onCheckedChange={(checked) => patch({ openWebLinksInBrowser: checked })}
      />
    </div>
  );
}

function BrowserToggle({ id, label, description, checked, onCheckedChange }: {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <label htmlFor={id} className="min-w-0 cursor-pointer space-y-1">
        <span className="block text-sm font-medium text-foreground">{label}</span>
        <span className="block text-xs text-muted-foreground">{description}</span>
      </label>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} aria-label={label} />
    </div>
  );
}
