import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { saveScheduledRuntimeSettings } from '@/api';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { readScheduledRuntimeSettingsCache, useScheduledRuntimeSettings } from './useScheduledRuntimeSettings';
import type { ScheduledRuntimeSettingsVm } from '@/types';

type SaveInput = Pick<
  ScheduledRuntimeSettingsVm,
  'keepAwakeEnabled' | 'completionNotificationsEnabled' | 'occurrenceRetentionDays'
>;
type SavePatch = Partial<SaveInput>;

export function ScheduledRuntimeSettings() {
  const { t } = useTranslation();
  const { settings, loadError, replace } = useScheduledRuntimeSettings();
  const [retention, setRetention] = useState<string>(() => {
    const cached = readScheduledRuntimeSettingsCache();
    return cached ? String(cached.occurrenceRetentionDays) : '';
  });
  const [retentionDirty, setRetentionDirty] = useState(false);
  const [retentionError, setRetentionError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const settingsRef = useRef(settings);
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const pendingSaveCountRef = useRef(0);

  // settings 首次到位后初始化保留天数输入框；后续后台刷新不覆盖用户编辑。
  useEffect(() => {
    settingsRef.current = settings;
    if (settings && !retentionDirty) {
      setRetention(String(settings.occurrenceRetentionDays));
    }
  }, [retentionDirty, settings]);

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
        occurrenceRetentionDays: patch.occurrenceRetentionDays ?? current.occurrenceRetentionDays,
      });
      settingsRef.current = saved;
      replace(saved);
      if (patch.occurrenceRetentionDays !== undefined) {
        setRetentionDirty(false);
        setRetentionError(false);
      }
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
      <div className="flex items-center justify-between gap-5 py-3">
        <label htmlFor="scheduled-retention-days" className="min-w-0">
          <span className="block text-sm font-medium">{t('scheduled.settings.retention')}</span>
          <span className="mt-1 block text-xs text-muted-foreground">{t('scheduled.settings.retentionDescription')}</span>
        </label>
        <div className="flex flex-col items-end gap-1">
          <Input
            id="scheduled-retention-days"
            type="number"
            min={1}
            max={3650}
            aria-invalid={retentionError}
            aria-describedby={retentionError ? 'scheduled-retention-error' : undefined}
            className="w-24"
            value={retention}
            disabled={saving}
            onChange={(event) => {
              setRetention(event.target.value);
              setRetentionDirty(true);
              setRetentionError(false);
            }}
            onBlur={() => {
              const occurrenceRetentionDays = Number(retention);
              if (!Number.isInteger(occurrenceRetentionDays) || occurrenceRetentionDays < 1 || occurrenceRetentionDays > 3650) {
                setRetentionError(true);
                return;
              }
              if (occurrenceRetentionDays === settings.occurrenceRetentionDays) {
                setRetentionDirty(false);
                return;
              }
              void save({ occurrenceRetentionDays });
            }}
          />
          {retentionError ? <span id="scheduled-retention-error" className="text-xs text-destructive">{t('scheduled.settings.retentionInvalid')}</span> : null}
        </div>
      </div>
      {saveError ? <div className="py-2 text-xs text-destructive">{t('scheduled.settings.saveFailed')}</div> : null}
    </div>
  );
}
