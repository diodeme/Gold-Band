import { memo, useCallback, useEffect, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import {
  Bot,
  Copy,
  CircleCheck,
  ChevronDown,
  Info,
  Loader2,
  QrCode,
  Repeat2,
  RefreshCw,
  RotateCcw,
  Save,
  Trash2,
  UserRoundCog,
  X,
} from 'lucide-react';
import {
  cancelFeishuScanAuthorization,
  cancelWeComScanAuthorization,
  completeFeishuScanAuthorization,
  completeWeComScanAuthorization,
  deleteImChannel,
  reconnectImChannel,
  resetImChannelBinding,
  saveImNotificationPreferences,
  setImChannelEnabled,
  startFeishuScanAuthorization,
  startWeComScanAuthorization,
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
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
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
  notificationDraftFrom,
  notificationPreferencesEqual,
} from '@/lib/im-settings';
import { useImSettings } from './useImSettings';
import type {
  FeishuScanMode,
  ImChannelKind,
  ImChannelSettingsVm,
  ImNotificationPreferencesVm,
  ImScanAuthorizationVm,
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

const channelOrder = ['weCom', 'feishu'] as const satisfies readonly ImChannelKind[];

function displayImError(t: TFunction, error: unknown) {
  if (error && typeof error === 'object' && typeof (error as { code?: unknown }).code === 'string') {
    const code = (error as { code: string }).code;
    return t(`settings.im.errors.${code}`, { defaultValue: t('settings.im.errors.IM_OPERATION_FAILED') });
  }
  return displayAppError(t, error);
}

export function ImIntegrationSettings() {
  const { t } = useTranslation();
  const { settings, loadError, replace: mergeSettings } = useImSettings();

  if (!settings) {
    return (
      <div className="py-3 text-sm text-muted-foreground" role={loadError ? 'alert' : undefined}>
        {loadError ? displayImError(t, loadError) : t('common.loading')}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {channelOrder.flatMap((kind) => {
        const channel = settings.channels.find((candidate) => candidate.kind === kind);
        return channel ? [channel] : [];
      }).map((channel) => (
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
  const [scanRequest, setScanRequest] = useState<ImScanRequest>(null);
  const [resetOpen, setResetOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [open, setOpen] = useState(false);
  const [configurationMenuOpen, setConfigurationMenuOpen] = useState(false);
  const display = imChannelDisplayModel(channel);
  const channelTitle = t(`settings.im.channels.${channel.kind}.title`);
  const resetCopy = channel.kind === 'feishu' ? 'settings.im.channels.feishu.resetBinding' : 'settings.im.resetBinding';
  const titleId = `im-${channel.kind}-title`;
  const enabledId = `im-${channel.kind}-enabled`;
  const contentId = `im-${channel.kind}-content`;
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
    <Collapsible
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (!nextOpen) setConfigurationMenuOpen(false);
      }}
      asChild
    >
      <section
        className={`overflow-hidden rounded-xl border bg-card transition-[border-color,box-shadow] ${open ? 'border-border shadow-sm' : 'border-border/55'}`}
        aria-labelledby={titleId}
        data-im-channel={channel.kind}
      >
        <div className={`flex flex-wrap items-center justify-between gap-3 px-4 py-3 transition-colors ${open ? 'bg-muted/25' : ''}`}>
          <div className="flex min-w-0 items-center gap-3">
            <span className={`flex size-9 shrink-0 items-center justify-center rounded-lg ${channel.kind === 'weCom' ? 'bg-gold-success/10 text-gold-success' : 'bg-gold-running/10 text-gold-running'}`} aria-hidden>
              <Bot className="size-4" />
            </span>
            <div className="min-w-0">
              <h3 id={titleId} className="truncate text-sm font-semibold">
                {channelTitle}
              </h3>
              <p className="truncate text-xs text-muted-foreground">{t('settings.im.channelSubtitle')}</p>
            </div>
          </div>

          <div className="flex min-h-9 shrink-0 items-center gap-2">
            <Badge
              variant={badgeVariant}
              className={display.status === 'ready' ? 'border-gold-success/30 bg-gold-success/10 text-gold-success' : undefined}
              data-im-status={display.status}
            >
              {display.status === 'connecting' || display.status === 'reconnecting'
                ? <Loader2 className="animate-spin" />
                : null}
              {t(`settings.im.status.${display.status}`)}
            </Badge>
            {channel.credentialConfigured ? (
              <div className="flex items-center gap-2">
                {pendingEnabled !== null ? <Loader2 className="size-4 animate-spin text-muted-foreground" aria-hidden /> : null}
                <Label htmlFor={enabledId} className="text-xs text-muted-foreground">{t('settings.im.enabled')}</Label>
                <Switch
                  id={enabledId}
                  checked={pendingEnabled ?? channel.enabled}
                  disabled={busy}
                  aria-label={t('settings.im.channelEnabled', { channel: channelTitle })}
                  aria-busy={pendingEnabled !== null}
                  onCheckedChange={(enabled) => void toggleEnabled(enabled)}
                />
              </div>
            ) : null}
            <Tooltip>
              <TooltipTrigger asChild>
                <CollapsibleTrigger asChild>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    className="size-9 shrink-0"
                    aria-controls={contentId}
                    aria-label={t(open ? 'settings.im.collapseChannel' : 'settings.im.expandChannel', { channel: channelTitle })}
                  >
                    <ChevronDown className={`size-4 transition-transform ${open ? '' : '-rotate-90'}`} aria-hidden />
                  </Button>
                </CollapsibleTrigger>
              </TooltipTrigger>
              <TooltipContent>
                {t(open ? 'settings.im.collapseChannel' : 'settings.im.expandChannel', { channel: channelTitle })}
              </TooltipContent>
            </Tooltip>
          </div>
        </div>

        <CollapsibleContent id={contentId} forceMount hidden={!open}>
          <div className="space-y-4 border-t border-border/45 px-4 py-3.5">
            {channel.credentialConfigured ? (
              <ConnectionGuidance
                channel={channel}
                channelTitle={channelTitle}
                recoveryBusy={recoveryBusy}
                onReconnect={() => void reconnect()}
                onReauthorize={() => setScanRequest({ mode: 'reauthorizeCurrent', replacesConfiguredApp: false })}
              />
            ) : null}

            {!channel.credentialConfigured ? (
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <p className="text-sm font-medium">{t(`settings.im.guidance.${display.status}.title`, { channel: channelTitle })}</p>
                  <p className="text-xs text-muted-foreground">{t('settings.im.scan.notConfigured', { channel: channelTitle })}</p>
                </div>
                <Button type="button" size="sm" onClick={() => setScanRequest({ mode: 'selectOrCreate', replacesConfiguredApp: false })}>
                  <QrCode className="size-4" />
                  {t('settings.im.scan.connect')}
                </Button>
              </div>
            ) : (
              <>
                <Separator />
                <div className="max-w-4xl space-y-3" data-im-configuration-section>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <p className="text-sm font-medium">{t('settings.im.configurationHint')}</p>
                    <Tooltip>
                      <DropdownMenu open={configurationMenuOpen} onOpenChange={setConfigurationMenuOpen}>
                        <TooltipTrigger asChild>
                          <DropdownMenuTrigger asChild>
                            <Button
                              type="button"
                              size="sm"
                              variant="secondary"
                              className="h-8 gap-1.5 border border-border/70 px-3 text-xs font-medium shadow-xs"
                              disabled={busy}
                              data-im-configuration-menu
                            >
                              {t('settings.im.configurationMenu')}
                              <ChevronDown
                                className={`size-3.5 transition-transform ${configurationMenuOpen ? 'rotate-180' : ''}`}
                                aria-hidden
                              />
                            </Button>
                          </DropdownMenuTrigger>
                        </TooltipTrigger>
                        <DropdownMenuContent align="end">
                          {channel.binding ? (
                            <DropdownMenuItem onSelect={() => setResetOpen(true)}>
                              <UserRoundCog />
                              {t(`${resetCopy}.action`)}
                            </DropdownMenuItem>
                          ) : null}
                          {channel.kind === 'feishu' ? (
                            <DropdownMenuItem onSelect={() => setScanRequest({ mode: 'selectOrCreate', replacesConfiguredApp: true })}>
                              <Repeat2 />
                              {t('settings.im.scan.changeApp')}
                            </DropdownMenuItem>
                          ) : null}
                          <DropdownMenuItem onSelect={() => setScanRequest({ mode: 'reauthorizeCurrent', replacesConfiguredApp: false })}>
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
                  <dl className="grid max-w-3xl gap-x-8 gap-y-3 text-xs sm:grid-cols-2">
                    {channel.binding ? (
                      <div className="min-w-0 space-y-1">
                        <dt className="text-muted-foreground">{t('settings.im.receiverAccount')}</dt>
                        <dd className="truncate text-foreground">{channel.binding.displayName ?? t('settings.im.privateBindingUnnamed')}</dd>
                      </div>
                    ) : null}
                    {channel.publicIdentity ? (
                      <div className="min-w-0 space-y-1">
                        <dt className="text-muted-foreground">{t(`settings.im.channels.${channel.kind}.identityLabel`)}</dt>
                        <dd className="flex min-w-0 items-center gap-1">
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <span className="truncate text-foreground">{channel.publicIdentity}</span>
                            </TooltipTrigger>
                            <TooltipContent className="max-w-sm break-all">{channel.publicIdentity}</TooltipContent>
                          </Tooltip>
                          <CopyIdentityButton
                            value={channel.publicIdentity}
                            label={t(`settings.im.channels.${channel.kind}.identityLabel`)}
                          />
                        </dd>
                      </div>
                    ) : null}
                  </dl>
                  {channel.kind === 'feishu' && channel.connection?.identity?.displayName ? (
                    <div className="space-y-1 text-xs">
                      <p className="text-muted-foreground">{t('settings.im.robotName')}</p>
                      <p className="break-words text-foreground">{channel.connection.identity.displayName}</p>
                    </div>
                  ) : null}
                  {channel.kind === 'feishu' ? (
                    <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
                      <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                      <span>{t('settings.im.channels.feishu.singleDeviceHint')}</span>
                    </p>
                  ) : null}
                </div>
                <Separator />
                <NotificationPreferencesForm channel={channel} onSettings={onSettings} />
              </>
            )}

            {error ? <p className="text-xs text-destructive" role="alert">{displayImError(t, error)}</p> : null}
          </div>
        </CollapsibleContent>

        <ImScanDialog
        channel={channel}
        channelTitle={channelTitle}
        mode={scanRequest?.mode ?? 'selectOrCreate'}
        replacesConfiguredApp={scanRequest?.replacesConfiguredApp ?? false}
        open={scanRequest !== null}
        onOpenChange={(open) => {
          if (!open) setScanRequest(null);
        }}
        onSettings={onSettings}
        />
        <AlertDialog open={resetOpen} onOpenChange={setResetOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t(`${resetCopy}.title`)}</AlertDialogTitle>
            <AlertDialogDescription>{t(`${resetCopy}.description`)}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={destructiveBusy}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={destructiveBusy} onClick={(event) => {
              event.preventDefault();
              void resetBinding();
            }}>
              {destructiveBusy ? <Loader2 className="size-4 animate-spin" /> : <UserRoundCog className="size-4" />}
              {t(`${resetCopy}.confirm`)}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
        </AlertDialog>
        <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.im.deleteTitle', { channel: channelTitle })}</AlertDialogTitle>
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
    </Collapsible>
  );
});

function CopyIdentityButton({ value, label }: { value: string; label: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1_500);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="size-6 shrink-0"
          aria-label={t('common.copy')}
          onClick={() => {
            void navigator.clipboard?.writeText(value)
              .then(() => setCopied(true))
              .catch(() => undefined);
          }}
        >
          {copied
            ? <CircleCheck className="size-3 text-gold-success" aria-hidden />
            : <Copy className="size-3" aria-hidden />}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{copied ? t('common.copied') : `${t('common.copy')} ${label}`}</TooltipContent>
    </Tooltip>
  );
}

function ConnectionGuidance({
  channel,
  channelTitle,
  recoveryBusy,
  onReconnect,
  onReauthorize,
}: {
  channel: ImChannelSettingsVm;
  channelTitle: string;
  recoveryBusy: boolean;
  onReconnect: () => void;
  onReauthorize: () => void;
}) {
  const { t } = useTranslation();
  const display = imChannelDisplayModel(channel);
  const errorCode = channel.connection?.lastErrorCode;
  return (
    <div
      className={`flex max-w-4xl flex-wrap items-start justify-between gap-3 rounded-lg px-3 py-2.5 ${display.status === 'ready' && !errorCode ? 'bg-gold-success/10' : 'bg-muted/35'}`}
      data-im-guidance={display.status}
    >
      <div className="flex min-w-0 items-start gap-2">
        {display.status === 'ready' && !errorCode ? <CircleCheck className="mt-0.5 size-4 shrink-0 text-gold-success" aria-hidden /> : null}
        <div className="min-w-0 space-y-0.5">
        <p className="text-sm font-medium">{t(`settings.im.guidance.${display.status}.title`, { channel: channelTitle })}</p>
        <p className="text-xs text-muted-foreground">{t(`settings.im.guidance.${display.status}.description`, { channel: channelTitle })}</p>
        {errorCode ? (
          <p className="text-xs text-destructive" role="alert">
            {t(`settings.im.errors.${errorCode}`, { defaultValue: t('settings.im.errors.IM_OPERATION_FAILED') })}
          </p>
        ) : null}
        </div>
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
    <fieldset className="max-w-4xl space-y-3" aria-busy={saving}>
      <legend className="text-sm font-medium">{t('settings.im.notificationKinds')}</legend>
      <div className="space-y-3">
        {notificationGroups.map((group) => (
          <div key={group.key} className="min-w-0 space-y-1.5">
            <p className="text-xs font-medium text-muted-foreground">
              {t(`settings.im.notificationGroups.${group.key}`)}
            </p>
            <div className="flex flex-wrap gap-x-2 gap-y-1">
              {group.items.map((key) => (
                <Label key={key} className="flex min-h-8 min-w-36 cursor-pointer items-center gap-2 rounded-md px-2 text-sm font-normal transition-colors hover:bg-muted/50">
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

type ImScanRequest = { mode: FeishuScanMode; replacesConfiguredApp: boolean } | null;

type ImScanState =
  | { kind: 'idle' | 'starting' }
  | { kind: 'waitingQr'; qrUrl: string; expiresAtMs: number }
  | { kind: 'waitingBinding'; generation: number }
  | { kind: 'error'; error: unknown };

function ImQrCanvas({ value, label, onError }: { value: string; label: string; onError: (error: unknown) => void }) {
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

const IM_SCAN_AUTHORIZATION: Record<ImChannelKind, {
  start: (sessionId: string, mode: FeishuScanMode) => Promise<ImScanAuthorizationVm>;
  complete: (sessionId: string) => Promise<ImSettingsVm>;
  cancel: (sessionId: string) => Promise<void>;
}> = {
  weCom: {
    start: (sessionId) => startWeComScanAuthorization(sessionId),
    complete: completeWeComScanAuthorization,
    cancel: cancelWeComScanAuthorization,
  },
  feishu: {
    start: startFeishuScanAuthorization,
    complete: completeFeishuScanAuthorization,
    cancel: cancelFeishuScanAuthorization,
  },
};

function ImScanDialog({
  channel,
  channelTitle,
  mode,
  replacesConfiguredApp,
  open,
  onOpenChange,
  onSettings,
}: {
  channel: ImChannelSettingsVm;
  channelTitle: string;
  mode: FeishuScanMode;
  replacesConfiguredApp: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSettings: (settings: ImSettingsVm) => void;
}) {
  const { t } = useTranslation();
  const [state, setState] = useState<ImScanState>({ kind: 'idle' });
  const [now, setNow] = useState(() => Date.now());
  const activeSessionRef = useRef<string | null>(null);
  const scan = IM_SCAN_AUTHORIZATION[channel.kind];

  const cancelActive = useCallback(() => {
    const sessionId = activeSessionRef.current;
    activeSessionRef.current = null;
    if (sessionId) void scan.cancel(sessionId).catch(() => undefined);
  }, [scan]);

  const close = useCallback(() => {
    cancelActive();
    setState({ kind: 'idle' });
    onOpenChange(false);
  }, [cancelActive, onOpenChange]);

  const start = useCallback(async () => {
    cancelActive();
    const sessionId = crypto.randomUUID();
    activeSessionRef.current = sessionId;
    setState({ kind: 'starting' });
    try {
      const authorization = await scan.start(sessionId, mode);
      if (activeSessionRef.current !== sessionId) return;
      setNow(Date.now());
      setState({ kind: 'waitingQr', qrUrl: authorization.qrUrl, expiresAtMs: authorization.expiresAtMs });
      const settings = await scan.complete(sessionId);
      if (activeSessionRef.current !== sessionId) return;
      activeSessionRef.current = null;
      onSettings(settings);
      const savedChannel = settings.channels.find((candidate) => candidate.kind === channel.kind);
      if (savedChannel && imChannelDisplayModel(savedChannel).status === 'ready') {
        close();
      } else {
        setState({ kind: 'waitingBinding', generation: savedChannel?.connection?.generation ?? 0 });
      }
    } catch (error) {
      if (activeSessionRef.current !== sessionId) return;
      activeSessionRef.current = null;
      setState({ kind: 'error', error });
    }
  }, [cancelActive, channel.kind, close, mode, onSettings, scan]);

  useEffect(() => {
    if (!open || state.kind !== 'idle' || channel.kind === 'feishu') return;
    void start();
  }, [channel.kind, open, start, state.kind]);

  useEffect(() => {
    if (state.kind !== 'waitingQr') return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [state.kind]);

  useEffect(() => {
    if (open && state.kind === 'waitingBinding'
        && (channel.connection?.generation ?? -1) >= state.generation
        && imChannelDisplayModel(channel).status === 'ready') {
      close();
    }
  }, [channel, close, open, state]);

  useEffect(() => () => cancelActive(), [cancelActive]);

  const qrError = useCallback((error: unknown) => setState({ kind: 'error', error }), []);
  const remainingSeconds = state.kind === 'waitingQr'
    ? Math.max(0, Math.ceil((state.expiresAtMs - now) / 1_000))
    : 0;
  const countdown = `${Math.floor(remainingSeconds / 60)}:${String(remainingSeconds % 60).padStart(2, '0')}`;

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) close(); }}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>
            {channel.kind === 'feishu' && replacesConfiguredApp && mode === 'selectOrCreate'
              ? t('settings.im.scan.changeApp')
              : t('settings.im.scan.title', { channel: channelTitle })}
          </DialogTitle>
          <DialogDescription>{t('settings.im.scan.description', { channel: channelTitle })}</DialogDescription>
        </DialogHeader>
        <div className="flex min-h-64 flex-col items-center justify-center gap-4 py-2" data-im-scan-state={state.kind}>
          {state.kind === 'idle' && channel.kind === 'feishu' ? (
            <div className="w-full space-y-4" data-im-scan-consent>
              <p className="text-sm font-medium">{t('settings.im.channels.feishu.permissions.title')}</p>
              <ul className="list-disc space-y-2 pl-5 text-sm text-muted-foreground">
                {(['send', 'receive', 'update', 'profile'] as const).map((permission) => (
                  <li key={permission}>{t(`settings.im.channels.feishu.permissions.${permission}`)}</li>
                ))}
              </ul>
              {replacesConfiguredApp && mode === 'selectOrCreate' ? (
                <p className="text-xs text-muted-foreground" role="status">
                  {t('settings.im.scan.changeAppWarning')}
                </p>
              ) : null}
              <Button type="button" className="w-full" onClick={() => void start()}>
                <QrCode className="size-4" />
                {t('settings.im.channels.feishu.permissions.confirm')}
              </Button>
            </div>
          ) : null}
          {state.kind === 'starting' ? (
            <>
              <Loader2 className="size-6 animate-spin text-muted-foreground" />
              <p className="text-sm text-muted-foreground">{t('settings.im.scan.generating')}</p>
            </>
          ) : null}
          {state.kind === 'waitingQr' ? (
            <>
              <ImQrCanvas value={state.qrUrl} label={t('settings.im.scan.qrCodeLabel', { channel: channelTitle })} onError={qrError} />
              <p className="text-center text-sm font-medium">{t('settings.im.scan.waiting', { channel: channelTitle })}</p>
              <p className="text-xs text-muted-foreground">{t('settings.im.scan.expiresIn', { countdown })}</p>
            </>
          ) : null}
          {state.kind === 'waitingBinding' ? (
            <>
              <UserRoundCog className="size-10 text-foreground" />
              <p className="text-center text-sm font-medium">{t('settings.im.scan.waitingBinding')}</p>
              <p className="max-w-xs text-center text-xs text-muted-foreground">{t('settings.im.scan.nextStep', { channel: channelTitle })}</p>
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
          <Button type="button" variant="outline" onClick={close}>
            {t('common.close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
