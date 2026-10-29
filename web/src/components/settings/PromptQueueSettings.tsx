import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Switch } from '@/components/ui/switch';
import { displayAppError } from '@/i18n';
import { savePromptQueueSettings, usePromptQueueSettings } from '@/lib/prompt-queue-settings-store';

export function PromptQueueSettings() {
  const { t } = useTranslation();
  const { confirmInterrupt } = usePromptQueueSettings();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = async (next: boolean) => {
    setSaving(true);
    setError(null);
    try {
      await savePromptQueueSettings({ confirmInterrupt: next });
    } catch (saveError) {
      setError(displayAppError(t, saveError));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-1 py-2">
      <label className="flex items-center gap-3">
        <span className="text-sm font-medium text-muted-foreground">{t('settings.promptQueue.confirmInterrupt')}</span>
        <Switch
          checked={confirmInterrupt}
          disabled={saving}
          onCheckedChange={(checked) => { void toggle(checked); }}
          data-prompt-queue-confirm-interrupt="true"
        />
      </label>
      {error ? <p className="text-ui-caption text-destructive" role="alert">{error}</p> : null}
    </div>
  );
}
