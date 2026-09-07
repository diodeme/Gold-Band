import { memo, useCallback, useEffect, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import {
  CheckCircle2,
  Loader2,
  MoreHorizontal,
  QrCode,
  RefreshCw,
  RotateCcw,
  Save,
  Trash2,
  UserRoundCog,
  X,
} from 'lucide-react';
import {
  cancelWeComScanAuthorization,
  completeWeComScanAuthorization,
  deleteImChannel,
  getImSettings,
  reconnectImChannel,
  resetImChannelBinding,
  saveImNotificationPreferences,
  setImChannelEnabled,
  startWeComScanAuthorization,
  subscribeImChannelStateUpdates,
} from '@/api';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Label } from '@/components/ui/label';
import { Separator } from '@/components/ui/separator';
import { Switch } from '@/components/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '@/i18n';
import {
  buildSaveImNotificationPreferencesInput,
  buildSetImChannelEnabledInput,
  imChannelDisplayModel,
  mergeImChannelSnapshot,
  mergeImSettings,
  notificationDraftFrom,
  notificationPreferencesEqual,
} from '@/lib/im-settings';
import type {
  ImChannelKind,
  ImChannelSettingsVm,
  ImNotificationPreferencesVm,
  ImSettingsVm,
} from '@/types';

const notificationGroups = [
  {
    key: 'needsAction',
    items: ['permission', 'elicitation', 'manualCheck'],
  },
  {
    key: 'results',
    items: ['runSuccess', 'runFailure', 'acpTurnFinished'],
  },
] as const satisfies readonly {
  key: string;
  items: readonly (keyof ImNotificationPreferencesVm)[];
}[];

function displayImError(t: TFunction, error: unknown) {
  if (error && typeof error === 'object' && typeof (error as { code?: unknown }).code === 'string') {
    const code = (error as { code: string }).code;
    return t(`settings.im.errors.${code}`, { defaultValue: t('settings.im.errors.IM_OPERATION_FAILED') });
  }
  return displayAppError(t, error);
}

export function ImIntegrationSettings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<ImSettingsVm | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void getImSettings()
      .then((value) => {
        if (active) setSettings(value);
      })
      .catch((error) => {
        if (active) setLoadError(error);
      });
    void subscribeImChannelStateUpdates((snapshot) => {
      if (active) setSettings((current) => current ? mergeImChannelSnapshot(current, snapshot) : current);
    }).then((cleanup) => {
      if (active) unlisten = cleanup;
      else cleanup();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const mergeSettings = useCallback((incoming: ImSettingsVm) => {
    setSettings((current) => current ? mergeImSettings(current, incoming) : incoming);
  }, []);

  if (!settings) {
    return (
      <div className="py-3 text-sm text-muted-foreground" role={loadError ? 'alert' : undefined}>
        {loadError ? displayImError(t, loadError) : t('common.loading')}
      </div>
    );
  }

  return (
    <div className="divide-y divide-border/45">
      {settings.channels.map((channel) => (
        <ImChannelEditor key={channel.kind} channel={channel} onSettings={mergeSettings} />
      ))}
    </div>
  );
}

const ImChannelEditor = memo(function ImChannelEditor({
  channel,
  onSettings,
}: {
  channel: ImChannelSettingsVm;
  onSettings: (settings: ImSettingsVm) => void;
}) {
  const { t } = useTranslation();
  const [pendingEnabled, setPendingEnabled] = useState<boolean | null>(null);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [destructiveBusy, setDestructiveBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [scanOpen, setScanOpen] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const display = imChannelDisplayModel(channel);
  const generation = channel.connection?.generation ?? 0;
  const busy = pendingEnabled !== null || recoveryBusy || destructiveBusy;

  const toggleEnabled = async (enabled: boolean) => {
    if (pendingEnabled !== null) return;
    setPendingEnabled(enabled);
    setError(null);
    try {
      onSettings(await setImChannelEnabled(buildSetImChannelEnabledInput(channel.kind, enabled)));
    } catch (caught) {
      setError(caught);
    } finally {
      setPendingEnabled(null);
    }
  };

  const reconnect = async () => {
    if (recoveryBusy) return;
    setRecoveryBusy(true);
    setError(null);
    try {
      const snapshot = await reconnectImChannel({ kind: channel.kind, expectedGeneration: generation });
      onSettings(mergeImChannelSnapshot({ channels: [channel] }, snapshot));
    } catch (caught) {
      setError(caught);
    } finally {
      setRecoveryBusy(false);
    }
  };

  const resetBinding = async () => {
    setDestructiveBusy(true);
    setError(null);
    try {
      onSettings(await resetImChannelBinding({ kind: channel.kind, expectedGeneration: generation }));
      setResetOpen(false);
    } catch (caught) {
      setError(caught);
    } finally {
      setDestructiveBusy(false);
    }
  };

  const deleteChannel = async () => {
    setDestructiveBusy(true);
    setError(null);
    try {
      const result = await deleteImChannel(channel.kind);
      onSettings(result.settings);
      if (result.cleanupStatus === 'pending') {
        setError({ code: 'IM_CHANNEL_CLEANUP_PENDING', params: { operationId: result.operationId } });
      }
      setDeleteOpen(false);
    } catch (caught) {
      setError(caught);
    } finally {
      setDestructiveBusy(false);
    }
  };

  const badgeVariant = display.tone === 'default'
    ? 'secondary'
    : display.tone === 'destructive' ? 'destructive' : 'outline';

  return (
    <section className="space-y-5 py-5 first:pt-3 last:pb-3" aria-labelledby="im-wecom-title">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1.5">
          <div className="flex flex-wrap items-center gap-2">
            <h3 id="im-wecom-title" className="text-sm font-semibold">
              {t('settings.im.channels.wecom.title')}
            </h3>
            <Badge variant={badgeVariant} data-im-status={display.status}>
              {display.status === 'connecting' || display.status === 'reconnecting'
                ? <Loader2 className="animate-spin" />
                : null}
              {t(`settings.im.status.${display.status}`)}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground">{t('settings.im.channels.wecom.description')}</p>
          {channel.publicIdentity ? (
            <p className="break-all text-xs text-muted-foreground">
              {t('settings.im.scan.botId', { id: channel.publicIdentity })}
            </p>
          ) : null}
          {channel.binding ? (
            <p className="break-words text-xs text-muted-foreground">
              {t('settings.im.privateBinding', { name: channel.binding.displayName })}
            </p>
          ) : null}
        </div>

        {channel.credentialConfigured ? (
          <div className="flex min-h-9 items-center gap-2">
            {pendingEnabled !== null ? <Loader2 className="size-4 animate-spin text-muted-foreground" aria-hidden /> : null}
            <Label htmlFor="im-wecom-enabled" className="text-xs">{t('settings.im.enabled')}</Label>
            <Switch
              id="im-wecom-enabled"
              checked={pendingEnabled ?? channel.enabled}
              disabled={busy}
              aria-busy={pendingEnabled !== null}
              onCheckedChange={(enabled) => void toggleEnabled(enabled)}
            />
          </div>
        ) : null}
      </div>

      {!channel.credentialConfigured ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="min-w-0 text-sm text-muted-foreground">{t('settings.im.scan.notConfigured')}</p>
          <Button type="button" size="sm" onClick={() => setScanOpen(true)}>
            <QrCode className="size-4" />
            {t('settings.im.scan.connect')}
          </Button>
        </div>
      ) : (
        <>
          <ConnectionGuidance
            channel={channel}
            recoveryBusy={recoveryBusy}
            onReconnect={() => void reconnect()}
            onReauthorize={() => setScanOpen(true)}
          />
          <Separator />
          <NotificationPreferencesForm channel={channel} onSettings={onSettings} />
          <Separator />
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-xs text-muted-foreground">{t('settings.im.configurationHint')}</p>
            <Tooltip>
              <DropdownMenu>
                <TooltipTrigger asChild>
                  <DropdownMenuTrigger asChild>
                    <Button type="button" size="icon" variant="ghost" disabled={busy} aria-label={t('settings.im.configurationMenu')}>
                      <MoreHorizontal className="size-4" />
                    </Button>
                  </DropdownMenuTrigger>
                </TooltipTrigger>
                <DropdownMenuContent align="end">
                  {channel.binding ? (
                    <DropdownMenuItem onSelect={() => setResetOpen(true)}>
                      <UserRoundCog />
                      {t('settings.im.resetBinding.action')}
                    </DropdownMenuItem>
                  ) : null}
                  <DropdownMenuItem onSelect={() => setScanOpen(true)}>
                    <QrCode />
                    {t('settings.im.scan.reauthorize')}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem variant="destructive" onSelect={() => setDeleteOpen(true)}>
                    <Trash2 />
                    {t('settings.im.delete')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <TooltipContent>{t('settings.im.configurationMenu')}</TooltipContent>
            </Tooltip>
          </div>
        </>
      )}

      {error ? <p className="text-xs text-destructive" role="alert">{displayImError(t, error)}</p> : null}

      <WeComScanDialog
        channel={channel}
        open={scanOpen}
        onOpenChange={setScanOpen}
        onSettings={onSettings}
      />
      <AlertDialog open={resetOpen} onOpenChange={setResetOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.im.resetBinding.title')}</AlertDialogTitle>
            <AlertDialogDescription>{t('settings.im.resetBinding.description')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={destructiveBusy}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={destructiveBusy} onClick={(event) => {
              event.preventDefault();
              void resetBinding();
            }}>
              {destructiveBusy ? <Loader2 className="size-4 animate-spin" /> : <UserRoundCog className="size-4" />}
              {t('settings.im.resetBinding.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.im.deleteTitle', { channel: t('settings.im.channels.wecom.title') })}</AlertDialogTitle>
            <AlertDialogDescription>{t('settings.im.deleteDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={destructiveBusy}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" disabled={destructiveBusy} onClick={(event) => {
              event.preventDefault();
              void deleteChannel();
            }}>
              {destructiveBusy ? <Loader2 className="size-4 animate-spin" /> : <Trash2 className="size-4" />}
              {t('settings.im.confirmDelete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
});

function ConnectionGuidance({
  channel,
  recoveryBusy,
  onReconnect,
  onReauthorize,
}: {
  channel: ImChannelSettingsVm;
  recoveryBusy: boolean;
  onReconnect: () => void;
  onReauthorize: () => void;
}) {
  const { t } = useTranslation();
  const display = imChannelDisplayModel(channel);
  const errorCode = channel.connection?.lastErrorCode;
  const showGuidance = display.status !== 'ready' || Boolean(errorCode);
  if (!showGuidance) return null;
  return (
    <div className="flex flex-wrap items-start justify-between gap-3" data-im-guidance={display.status}>
      <div className="min-w-0 space-y-1">
        <p className="text-sm font-medium">{t(`settings.im.guidance.${display.status}.title`)}</p>
        <p className="text-xs text-muted-foreground">{t(`settings.im.guidance.${display.status}.description`)}</p>
        {errorCode ? (
          <p className="text-xs text-destructive" role="alert">
            {t(`settings.im.errors.${errorCode}`, { defaultValue: t('settings.im.errors.IM_OPERATION_FAILED') })}
          </p>
        ) : null}
      </div>
      {display.recoveryAction === 'reauthorize' ? (
        <Button type="button" size="sm" variant="outline" disabled={recoveryBusy} onClick={onReauthorize}>
          <QrCode className="size-4" />
          {t('settings.im.scan.reauthorize')}
        </Button>
      ) : null}
      {display.recoveryAction === 'reconnect' ? (
        <Button type="button" size="sm" variant="outline" disabled={recoveryBusy} onClick={onReconnect}>
          {recoveryBusy ? <Loader2 className="size-4 animate-spin" /> : <RotateCcw className="size-4" />}
          {t('settings.im.reconnect')}
        </Button>
      ) : null}
    </div>
  );
}

function NotificationPreferencesForm({
  channel,
  onSettings,
}: {
  channel: ImChannelSettingsVm;
  onSettings: (settings: ImSettingsVm) => void;
}) {
  const { t } = useTranslation();
  const [baseline, setBaseline] = useState(() => notificationDraftFrom(channel).notifications);
  const [draft, setDraft] = useState(() => notificationDraftFrom(channel).notifications);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const dirty = !notificationPreferencesEqual(draft, baseline);

  useEffect(() => {
    if (notificationPreferencesEqual(channel.notifications, baseline)) return;
    setBaseline({ ...channel.notifications });
    if (!dirty) setDraft({ ...channel.notifications });
  }, [baseline, channel.notifications, dirty]);

  const save = async () => {
    if (saving || !dirty) return;
    setSaving(true);
    setError(null);
    try {
      const settings = await saveImNotificationPreferences(
        buildSaveImNotificationPreferencesInput(channel.kind, draft),
      );
      const saved = settings.channels.find((candidate) => candidate.kind === channel.kind);
      if (saved) {
        setBaseline({ ...saved.notifications });
        setDraft({ ...saved.notifications });
      }
      onSettings(settings);
    } catch (caught) {
      setError(caught);
    } finally {
      setSaving(false);
    }
  };

  return (
    <fieldset className="space-y-4" aria-busy={saving}>
      <legend className="text-sm font-medium">{t('settings.im.notificationKinds')}</legend>
      <div className="grid gap-5 md:grid-cols-2">
        {notificationGroups.map((group) => (
          <div key={group.key} className="min-w-0 space-y-2">
            <p className="text-xs font-medium text-muted-foreground">
              {t(`settings.im.notificationGroups.${group.key}`)}
            </p>
            <div className="space-y-1">
              {group.items.map((key) => (
                <Label key={key} className="flex min-h-9 cursor-pointer items-center gap-2 text-sm font-normal">
                  <Checkbox
                    checked={draft[key]}
                    disabled={saving}
                    onCheckedChange={(checked) => setDraft((current) => ({
                      ...current,
                      [key]: checked === true,
                    }))}
                  />
                  <span className="break-words">{t(`settings.im.notifications.${key}`)}</span>
                </Label>
              ))}
            </div>
          </div>
        ))}
      </div>
      {error ? <p className="text-xs text-destructive" role="alert">{displayImError(t, error)}</p> : null}
      {dirty ? (
        <div className="flex flex-wrap justify-end gap-2" data-im-notification-actions>
          <Button type="button" size="sm" variant="ghost" disabled={saving} onClick={() => setDraft({ ...baseline })}>
            <X className="size-4" />
            {t('settings.im.notificationsDiscard')}
          </Button>
          <Button type="button" size="sm" disabled={saving} onClick={() => void save()}>
            {saving ? <Loader2 className="size-4 animate-spin" /> : <Save className="size-4" />}
            {t('settings.im.notificationsSave')}
          </Button>
        </div>
      ) : null}
    </fieldset>
  );
}

type WeComScanState =
  | { kind: 'idle' | 'starting' }
  | { kind: 'waitingQr'; authUrl: string; expiresAtMs: number }
  | { kind: 'waitingBinding' }
  | { kind: 'complete' }
  | { kind: 'error'; error: unknown };

function WeComQrCanvas({ value, label, onError }: { value: string; label: string; onError: (error: unknown) => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let active = true;
    if (!canvasRef.current) return;
    void import('qrcode')
      .then(({ default: QRCode }) => QRCode.toCanvas(canvasRef.current, value, {
        width: 208,
        margin: 1,
        color: { dark: '#000000', light: '#ffffff' },
      }))
      .catch((error) => { if (active) onError(error); });
    return () => { active = false; };
  }, [onError, value]);
  return <canvas ref={canvasRef} width={208} height={208} className="size-52 rounded-md bg-white" aria-label={label} />;
}

function WeComScanDialog({
  channel,
  open,
  onOpenChange,
  onSettings,
}: {
  channel: ImChannelSettingsVm;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSettings: (settings: ImSettingsVm) => void;
}) {
  const { t } = useTranslation();
  const [state, setState] = useState<WeComScanState>({ kind: 'idle' });
  const [now, setNow] = useState(() => Date.now());
  const activeSessionRef = useRef<string | null>(null);

  const cancelActive = useCallback(() => {
    const sessionId = activeSessionRef.current;
    activeSessionRef.current = null;
    if (sessionId) void cancelWeComScanAuthorization(sessionId).catch(() => undefined);
  }, []);

  const start = useCallback(async () => {
    cancelActive();
    const sessionId = crypto.randomUUID();
    activeSessionRef.current = sessionId;
    setState({ kind: 'starting' });
    try {
      const authorization = await startWeComScanAuthorization(sessionId);
      if (activeSessionRef.current !== sessionId) return;
      setNow(Date.now());
      setState({ kind: 'waitingQr', authUrl: authorization.authUrl, expiresAtMs: authorization.expiresAtMs });
      const settings = await completeWeComScanAuthorization(sessionId);
      if (activeSessionRef.current !== sessionId) return;
      activeSessionRef.current = null;
      onSettings(settings);
      const savedChannel = settings.channels.find((candidate) => candidate.kind === channel.kind);
      setState(savedChannel && imChannelDisplayModel(savedChannel).status === 'ready'
        ? { kind: 'complete' }
        : { kind: 'waitingBinding' });
    } catch (error) {
      if (activeSessionRef.current !== sessionId) return;
      activeSessionRef.current = null;
      setState({ kind: 'error', error });
    }
  }, [cancelActive, channel.kind, onSettings]);

  useEffect(() => {
    if (!open || state.kind !== 'idle') return;
    void start();
  }, [open, start, state.kind]);

  useEffect(() => {
    if (state.kind !== 'waitingQr') return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [state.kind]);

  useEffect(() => {
    if (state.kind === 'waitingBinding' && imChannelDisplayModel(channel).status === 'ready') {
      setState({ kind: 'complete' });
    }
  }, [channel, state.kind]);

  useEffect(() => () => cancelActive(), [cancelActive]);

  const close = useCallback(() => {
    cancelActive();
    setState({ kind: 'idle' });
    onOpenChange(false);
  }, [cancelActive, onOpenChange]);
  const qrError = useCallback((error: unknown) => setState({ kind: 'error', error }), []);
  const remainingSeconds = state.kind === 'waitingQr'
    ? Math.max(0, Math.ceil((state.expiresAtMs - now) / 1_000))
    : 0;
  const countdown = `${Math.floor(remainingSeconds / 60)}:${String(remainingSeconds % 60).padStart(2, '0')}`;

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) close(); }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>{t('settings.im.scan.title')}</DialogTitle>
          <DialogDescription>{t('settings.im.scan.description')}</DialogDescription>
        </DialogHeader>
        <div className="flex min-h-64 flex-col items-center justify-center gap-4 py-2" data-im-scan-state={state.kind}>
          {state.kind === 'starting' ? (
            <>
              <Loader2 className="size-6 animate-spin text-muted-foreground" />
              <p className="text-sm text-muted-foreground">{t('settings.im.scan.generating')}</p>
            </>
          ) : null}
          {state.kind === 'waitingQr' ? (
            <>
              <WeComQrCanvas value={state.authUrl} label={t('settings.im.scan.qrCodeLabel')} onError={qrError} />
              <p className="text-center text-sm font-medium">{t('settings.im.scan.waiting')}</p>
              <p className="text-xs text-muted-foreground">{t('settings.im.scan.expiresIn', { countdown })}</p>
            </>
          ) : null}
          {state.kind === 'waitingBinding' ? (
            <>
              <UserRoundCog className="size-10 text-foreground" />
              <p className="text-center text-sm font-medium">{t('settings.im.scan.waitingBinding')}</p>
              <p className="max-w-xs text-center text-xs text-muted-foreground">{t('settings.im.scan.nextStep')}</p>
            </>
          ) : null}
          {state.kind === 'complete' ? (
            <>
              <CheckCircle2 className="size-10 text-foreground" />
              <p className="text-sm font-medium">{t('settings.im.scan.complete')}</p>
            </>
          ) : null}
          {state.kind === 'error' ? (
            <>
              <p className="text-center text-sm text-destructive" role="alert">{displayImError(t, state.error)}</p>
              <Button type="button" variant="outline" size="sm" onClick={() => void start()}>
                <RefreshCw className="size-4" />
                {t('settings.im.scan.retry')}
              </Button>
            </>
          ) : null}
        </div>
        <DialogFooter>
          <Button type="button" variant={state.kind === 'complete' ? 'default' : 'outline'} onClick={close}>
            {t(state.kind === 'complete' ? 'common.done' : 'common.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
