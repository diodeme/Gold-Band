import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { saveScheduledRuntimeSettings } from '@/api';
import { Switch } from '@/components/ui/switch';
import { useScheduledRuntimeSettings } from './useScheduledRuntimeSettings';
import type { ScheduledRuntimeSettingsVm } from '@/types';

type SaveInput = Pick<
  ScheduledRuntimeSettingsVm,
  'keepAwakeEnabled' | 'completionNotificationsEnabled'
>;
type SavePatch = Partial<SaveInput>;

export function ScheduledRuntimeSettings() {
  const { t } = useTranslation();
  const { settings, loadError, replace } = useScheduledRuntimeSettings();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const settingsRef = useRef(settings);
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const pendingSaveCountRef = useRef(0);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  const save = (patch: SavePatch) => {
    pendingSaveCountRef.current += 1;
    setSaving(true);
    setSaveError(false);
    const operation = saveQueueRef.current.then(async () => {
      const current = settingsRef.current;
      if (!current) return;
      const saved = await saveScheduledRuntimeSettings({
        keepAwakeEnabled: patch.keepAwakeEnabled ?? current.keepAwakeEnabled,
        completionNotificationsEnabled: patch.completionNotificationsEnabled ?? current.completionNotificationsEnabled,
      });
      settingsRef.current = saved;
      replace(saved);
    });
    saveQueueRef.current = operation.catch(() => undefined);
    return operation
      .catch(() => setSaveError(true))
      .finally(() => {
        pendingSaveCountRef.current -= 1;
        if (pendingSaveCountRef.current === 0) setSaving(false);
      });
  };

  if (!settings) {
    return (
      <div className="py-2 text-sm text-muted-foreground">
        {loadError ? t('scheduled.settings.loadFailed') : t('common.loading')}
      </div>
    );
  }

  const effectiveKey = settings.keepAwakeEnabled
    ? settings.keepAwakeEffective
      ? 'effective'
      : 'enabledInactive'
    : 'disabled';

  return (
    <div className="divide-y divide-border/45">
      <div className="flex items-start justify-between gap-5 py-3">
        <div className="min-w-0">
          <div className="text-sm font-medium">{t('scheduled.settings.keepAwake')}</div>
          <div className="mt-1 text-xs text-muted-foreground">{t(`scheduled.settings.keepAwakeState.${effectiveKey}`, { count: settings.enabledJobCount })}</div>
          {settings.powerErrorCode ? <div className="mt-1 text-xs text-destructive">{t(`scheduled.errors.${settings.powerErrorCode}`, { defaultValue: settings.powerErrorCode })}</div> : null}
        </div>
        <Switch
          checked={settings.keepAwakeEnabled}
          disabled={saving}
          aria-label={t('scheduled.settings.keepAwake')}
          onCheckedChange={(keepAwakeEnabled) => void save({ keepAwakeEnabled })}
        />
      </div>
      <div className="flex items-start justify-between gap-5 py-3">
        <div className="min-w-0">
          <div className="text-sm font-medium">{t('scheduled.settings.completionNotifications')}</div>
          <div className="mt-1 text-xs text-muted-foreground">{t('scheduled.settings.completionNotificationsDescription')}</div>
        </div>
        <Switch
          checked={settings.completionNotificationsEnabled}
          disabled={saving}
          aria-label={t('scheduled.settings.completionNotifications')}
          onCheckedChange={(completionNotificationsEnabled) => void save({ completionNotificationsEnabled })}
        />
      </div>
      {saveError ? <div className="py-2 text-xs text-destructive">{t('scheduled.settings.saveFailed')}</div> : null}
    </div>
  );
}
