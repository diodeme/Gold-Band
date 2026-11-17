import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { ExternalLink, FileKey, MoreHorizontal, Pencil, Plug, Plus, RefreshCw, Router, Trash2, X } from 'lucide-react';
import {
  createServerConnection,
  deleteServerConnection,
  listRecentConnections,
  listServerConnections,
  openDetachedWindow,
  updateServerConnection,
} from '@/api';
import { parseDetachedRoute } from '@/lib/detached';
import { useFeatureAvailability } from '@/lib/feature-availability';
import { Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { AppErrorVm, ConnectionAuthMethod, ConnectionEnvironment, RecentConnectionVm, ServerConnectionInput, ServerConnectionVm } from '@/types';

function isAppErrorVm(value: unknown): value is AppErrorVm {
  return Boolean(value)
    && typeof value === 'object'
    && typeof (value as Partial<AppErrorVm>).code === 'string'
    && typeof (value as Partial<AppErrorVm>).params === 'object'
    && (value as Partial<AppErrorVm>).params !== null;
}

/** Structured backend errors carry only a code + params, never user copy — the
 *  frontend owns the message via `errors.<code>` (dots become nested i18n keys). */
function actionErrorMessage(t: TFunction, error: unknown): string {
  if (isAppErrorVm(error)) return t(`errors.${error.code}`, error.params as Record<string, unknown>);
  return t('connections.management.actionFailed');
}

function formatTimestamp(value?: string | null) {
  if (!value) return '--';
  try {
    return new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' }).format(new Date(value));
  } catch {
    return value;
  }
}

export function connectionEnvironmentLabel(t: TFunction, env: ConnectionEnvironment) {
  return t(env === 'test' ? 'connections.dialog.envTest' : env === 'stage' ? 'connections.dialog.envStage' : 'connections.dialog.envProd');
}

function environmentBadgeClass(env: ConnectionEnvironment) {
  if (env === 'prod') return 'bg-destructive text-destructive-foreground';
  if (env === 'stage') return 'border-amber-500/50 bg-amber-500/10 text-amber-700 dark:text-amber-400';
  return 'border-border bg-muted text-muted-foreground';
}

function authLabel(t: TFunction, method: ConnectionAuthMethod) {
  return t(method === 'publicKey' ? 'connections.management.keyAuth' : 'connections.management.passwordAuth');
}

const NO_JUMP_VALUE = 'none';

function ConnectionDialog({
  t,
  editing,
  connections,
  saving,
  errorMessage,
  onDismissError,
  onSave,
  onClose,
}: {
  t: TFunction;
  editing: ServerConnectionVm | null;
  connections: ServerConnectionVm[];
  saving: boolean;
  errorMessage: string | null;
  onDismissError: () => void;
  onSave: (input: ServerConnectionInput) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState(editing?.name ?? '');
  const [host, setHost] = useState(editing?.host ?? '');
  const [port, setPort] = useState(String(editing?.port ?? 22));
  const [username, setUsername] = useState(editing?.username ?? '');
  const [authMethod, setAuthMethod] = useState<ConnectionAuthMethod>(editing?.authMethod ?? 'password');
  const [keyPath, setKeyPath] = useState<string>(editing?.keyPath ?? '');
  const [jumpRef, setJumpRef] = useState<string>(editing?.jumpRef ?? '');
  const [env, setEnv] = useState<ConnectionEnvironment>(editing?.env ?? 'test');
  const [secret, setSecret] = useState('');
  const [saveSecret, setSaveSecret] = useState(false);
  const [clearSecret, setClearSecret] = useState(false);

  const jumpOptions = useMemo(() => {
    const options = connections.filter((connection) => connection.id !== editing?.id);
    if (editing?.jumpRef && editing.jumpMissing && !options.some((option) => option.id === editing.jumpRef)) {
      return [{ id: editing.jumpRef, name: t('connections.management.jumpMissing') }, ...options];
    }
    return options;
  }, [connections, editing, t]);

  const portNumber = Number.parseInt(port, 10);
  const validated =
    name.trim().length > 0 &&
    host.trim().length > 0 &&
    username.trim().length > 0 &&
    Number.isFinite(portNumber) &&
    portNumber > 0 &&
    (authMethod === 'password' || keyPath.trim().length > 0);

  const submit = () => {
    if (!validated || saving) return;
    onSave({
      name: name.trim(),
      host: host.trim(),
      port: portNumber,
      username: username.trim(),
      authMethod,
      keyPath: authMethod === 'publicKey' ? keyPath.trim() : null,
      jumpRef: jumpRef && jumpRef !== NO_JUMP_VALUE ? jumpRef : null,
      env,
      secret: saveSecret && secret ? secret : null,
      clearSecret: Boolean(editing) && clearSecret,
    });
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open && !saving) onClose(); }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{editing ? t('connections.dialog.editTitle') : t('connections.dialog.createTitle')}</DialogTitle>
          <DialogDescription className="sr-only">{editing ? t('connections.dialog.editTitle') : t('connections.dialog.createTitle')}</DialogDescription>
        </DialogHeader>
        {errorMessage ? (
          <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            <span className="min-w-0 flex-1">{errorMessage}</span>
            <button type="button" onClick={onDismissError} aria-label={t('connections.dialog.dismissError')} className="shrink-0 rounded p-0.5 hover:bg-destructive/10">
              <X className="size-3.5" />
            </button>
          </div>
        ) : null}
        <div className="grid gap-4 py-2">
          <div className="grid gap-2">
            <Label htmlFor="connection-name">{t('connections.dialog.name')}</Label>
            <Input id="connection-name" value={name} onChange={(event) => setName(event.target.value)} placeholder={t('connections.dialog.namePlaceholder')} autoFocus />
          </div>
          <div className="grid grid-cols-[minmax(0,1fr)_5.5rem] gap-3">
            <div className="grid gap-2">
              <Label htmlFor="connection-host">{t('connections.dialog.host')}</Label>
              <Input id="connection-host" value={host} onChange={(event) => setHost(event.target.value)} placeholder={t('connections.dialog.hostPlaceholder')} />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="connection-port">{t('connections.dialog.port')}</Label>
              <Input id="connection-port" type="number" min={1} max={65535} value={port} onChange={(event) => setPort(event.target.value)} />
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="connection-username">{t('connections.dialog.username')}</Label>
            <Input id="connection-username" value={username} onChange={(event) => setUsername(event.target.value)} />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="grid gap-2">
              <Label>{t('connections.dialog.authMethod')}</Label>
              <Select value={authMethod} onValueChange={(value) => setAuthMethod(value as ConnectionAuthMethod)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="password">{t('connections.dialog.authPassword')}</SelectItem>
                  <SelectItem value="publicKey">{t('connections.dialog.authPublicKey')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>{t('connections.dialog.env')}</Label>
              <Select value={env} onValueChange={(value) => setEnv(value as ConnectionEnvironment)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="test">{t('connections.dialog.envTest')}</SelectItem>
                  <SelectItem value="stage">{t('connections.dialog.envStage')}</SelectItem>
                  <SelectItem value="prod">{t('connections.dialog.envProd')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {authMethod === 'publicKey' ? (
            <div className="grid gap-2">
              <Label htmlFor="connection-key-path">{t('connections.dialog.keyPath')}</Label>
              <Input id="connection-key-path" value={keyPath} onChange={(event) => setKeyPath(event.target.value)} placeholder={t('connections.dialog.keyPathPlaceholder')} />
            </div>
          ) : null}
          <div className="grid gap-2">
            <Label>{authMethod === 'publicKey' ? t('connections.dialog.keyPassphrase') : t('connections.dialog.secret')}</Label>
            <div className="flex items-center gap-3">
              <Input type="password" value={secret} onChange={(event) => setSecret(event.target.value)} />
              <label className="flex shrink-0 items-center gap-2 text-sm text-muted-foreground">
                <Checkbox checked={saveSecret} onCheckedChange={(checked) => setSaveSecret(checked === true)} />
                {t('connections.dialog.saveSecret')}
              </label>
            </div>
            <p className="text-xs text-muted-foreground">{t('connections.dialog.saveSecretHint')}</p>
            {authMethod === 'password' ? (
              <p className="text-xs text-muted-foreground">{t('connections.dialog.dynamicPasswordHint')}</p>
            ) : null}
          </div>
          {editing?.credentialRef ? (
            <label className="flex items-center gap-2 text-sm text-muted-foreground">
              <Checkbox checked={clearSecret} onCheckedChange={(checked) => setClearSecret(checked === true)} />
              {t('connections.dialog.clearSecret')}
            </label>
          ) : null}
          <div className="grid gap-2">
            <Label>{t('connections.dialog.jump')}</Label>
            <Select value={jumpRef || NO_JUMP_VALUE} onValueChange={(value) => setJumpRef(value === NO_JUMP_VALUE ? '' : value)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_JUMP_VALUE}>{t('connections.dialog.jumpNone')}</SelectItem>
                {jumpOptions.map((option) => (
                  <SelectItem key={option.id} value={option.id}>
                    {option.name}{option.id === editing?.jumpRef && editing.jumpMissing ? ` (${t('connections.management.jumpMissing')})` : ''}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {env === 'prod' ? <p className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">{t('connections.dialog.prodWarning')}</p> : null}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={saving}>{t('connections.dialog.cancel')}</Button>
          <Button onClick={submit} disabled={!validated || saving}>{t('connections.dialog.save')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ConnectionsTerminalPage({ workspaceRoot }: { workspaceRoot?: string | null }) {
  const { t } = useTranslation();
  const { remoteConnectionsEnabled } = useFeatureAvailability();
  const [connections, setConnections] = useState<ServerConnectionVm[]>([]);
  const [recent, setRecent] = useState<RecentConnectionVm[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  // Save/delete errors render inside the still-open dialog (not the page body,
  // which is hidden behind it) and persist until the user dismisses them — they
  // must never auto-clear or be silently replaced ("flashes and disappears").
  const [dialogError, setDialogError] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState<ServerConnectionVm | null>(null);
  const [dialog, setDialog] = useState<{ editing: ServerConnectionVm | null } | null>(null);
  const requestIdRef = useRef(0);

  const load = useCallback(() => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    setLoadError(false);
    return Promise.all([listServerConnections(), listRecentConnections()])
      .then(([nextConnections, nextRecent]) => {
        if (requestId !== requestIdRef.current) return;
        setConnections(nextConnections);
        setRecent(nextRecent);
      })
      .catch(() => {
        if (requestId === requestIdRef.current) setLoadError(true);
      })
      .finally(() => {
        if (requestId === requestIdRef.current) setLoading(false);
      });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const saveConnection = async (input: ServerConnectionInput) => {
    setSaving(true);
    setDialogError(null);
    try {
      if (dialog?.editing) await updateServerConnection(dialog.editing.id, input);
      else await createServerConnection(input);
      setDialog(null);
      await load();
    } catch (error) {
      setDialogError(actionErrorMessage(t, error));
    } finally {
      setSaving(false);
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    setSaving(true);
    setDeleteError(null);
    try {
      await deleteServerConnection(deleting.id);
      setDeleting(null);
      await load();
    } catch (error) {
      setDeleteError(actionErrorMessage(t, error));
    } finally {
      setSaving(false);
    }
  };

  // Remote connections are intranet-only. On external/public channels the
  // sidebar entry is hidden, but guard the page too in case it is reached by URL.
  if (!remoteConnectionsEnabled) {
    return (
      <Page flush>
        <PageHeader
          variant="integrated"
          icon={<Plug />}
          title={<span className="text-title">{t('terminal.panelTitle')}</span>}
        />
        <div className="flex min-h-0 flex-1 items-center justify-center px-6">
          <p className="text-sm text-muted-foreground">{t('connections.management.disableNotice')}</p>
        </div>
      </Page>
    );
  }

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        variant="integrated"
        icon={<Plug />}
        title={<span className="text-title">{t('connections.management.title')}</span>}
        badges={<span className="text-sm font-normal text-muted-foreground">{connections.length}</span>}
        actions={(
          <>
            {!parseDetachedRoute().isDetached ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button size="sm" variant="ghost" className="h-8 gap-1.5" onClick={() => void openDetachedWindow('connections')} aria-label={t('terminal.openInNewWindow')}>
                    <ExternalLink className="size-3.5" />{t('terminal.openInNewWindow')}
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{t('terminal.openInNewWindow')}</TooltipContent>
              </Tooltip>
            ) : null}
            <Button size="sm" className="h-8 gap-1.5" onClick={() => setDialog({ editing: null })}>
              <Plus className="size-3.5" />{t('connections.management.create')}
            </Button>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="size-8" onClick={() => void load()} disabled={loading} aria-label={t('connections.management.refresh')}>
                  <RefreshCw className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('connections.management.refresh')}</TooltipContent>
            </Tooltip>
          </>
        )}
      />

      <div className="flex min-h-0 flex-1 flex-col">
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-6 pt-4">
        {loadError ? <p className="mb-3 text-sm text-destructive">{t('connections.management.loadFailed')}</p> : null}
        {loading && connections.length === 0 ? (
          <div className="border-y border-border/60 py-12 text-center text-sm text-muted-foreground">{t('connections.management.loading')}</div>
        ) : null}
        {!loading && !loadError && connections.length === 0 ? (
          <div className="border-y border-border/60 py-14 text-center">
            <p className="text-sm text-muted-foreground">{t('connections.management.empty')}</p>
            <Button size="sm" className="mt-4 h-8 gap-1.5" onClick={() => setDialog({ editing: null })}>
              <Plus className="size-3.5" />{t('connections.management.create')}
            </Button>
          </div>
        ) : null}
        {connections.length > 0 ? (
          <section className="w-full min-w-0">
            <div className="hidden grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,0.8fr)_auto_2rem] items-center gap-4 border-b border-border/60 px-3 pb-3 text-xs text-muted-foreground md:grid">
              <span>{t('connections.management.columns.name')}</span>
              <span>{t('connections.management.columns.host')}</span>
              <span>{t('connections.management.columns.username')}</span>
              <span>{t('connections.management.columns.env')}</span>
              <span>{t('connections.management.columns.credential')}</span>
              <span />
            </div>
            <div className="divide-y divide-border/60">
              {connections.map((connection) => (
                <div key={connection.id} className="grid grid-cols-[minmax(0,1fr)_2rem] items-center gap-3 px-3 py-4 md:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,0.9fr)_minmax(0,0.8fr)_auto_2rem] md:gap-4">
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-foreground/10 text-foreground">
                      {connection.authMethod === 'publicKey' ? <FileKey className="size-4" /> : <Router className="size-4" />}
                    </span>
                    <div className="min-w-0">
                      <div className="flex min-w-0 items-center gap-2">
                        <span className="truncate text-sm font-medium">{connection.name}</span>
                        {connection.jumpRef ? (
                          <Badge variant="outline" className="shrink-0 gap-1">
                            <Router className="size-3" />
                            {connection.jumpMissing ? t('connections.management.jumpMissing') : connection.jumpLabel ?? t('connections.management.jumpMarker')}
                          </Badge>
                        ) : null}
                      </div>
                      <div className="mt-1 truncate text-xs text-muted-foreground">{authLabel(t, connection.authMethod)}<span className="md:hidden"> · {connection.username}@{connection.host}:{connection.port}</span></div>
                    </div>
                  </div>
                  <div className="hidden min-w-0 text-xs text-muted-foreground md:block">{connection.host}:{connection.port}</div>
                  <div className="hidden min-w-0 truncate text-xs text-foreground md:block">{connection.username}</div>
                  <div className="hidden md:block">
                    <Badge className={environmentBadgeClass(connection.env)}>{connectionEnvironmentLabel(t, connection.env)}</Badge>
                  </div>
                  <div className="hidden text-xs md:block">
                    {connection.credentialRef
                      ? <span className="text-foreground">{t('connections.management.credentialSaved')}</span>
                      : <span className="text-muted-foreground">{t('connections.management.credentialMissing')}</span>}
                  </div>
                  <div>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" className="size-8" aria-label={t('connections.management.more')}><MoreHorizontal className="size-4" /></Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onClick={() => setDialog({ editing: connection })}><Pencil className="size-4" />{t('connections.management.edit')}</DropdownMenuItem>
                        <DropdownMenuItem className="text-destructive focus:text-destructive" onClick={() => setDeleting(connection)}><Trash2 className="size-4" />{t('connections.management.delete')}</DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </div>
              ))}
            </div>
          </section>
        ) : null}

        <section className="mt-6 w-full min-w-0">
          <div className="flex items-center justify-between border-b border-border/60 pb-2">
            <h2 className="text-sm font-medium text-foreground">{t('connections.recent.title')}</h2>
          </div>
          {recent.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">{t('connections.recent.empty')}</p>
          ) : (
            <div className="divide-y divide-border/60">
              {recent.map((entry, index) => (
                <div key={`${entry.host}-${entry.username}-${index}`} className="flex items-center justify-between gap-3 px-1 py-3 text-sm">
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="size-2 shrink-0 rounded-full bg-primary/70" />
                    <div className="min-w-0">
                      <div className="truncate text-foreground">{entry.host}</div>
                      <div className="mt-0.5 truncate text-xs text-muted-foreground">{entry.username} · <Badge className={environmentBadgeClass(entry.env)}>{connectionEnvironmentLabel(t, entry.env)}</Badge></div>
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-3">
                    <span className="text-xs text-muted-foreground">{formatTimestamp(entry.lastConnectedAt)}</span>
                    {entry.reconnectable
                      ? <span className="text-xs text-foreground">{t('connections.recent.reconnect')}</span>
                      : <span className="text-xs text-muted-foreground">{t('connections.recent.notReconnectable')}</span>}
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
        </div>
      </div>

      {dialog ? (
        <ConnectionDialog
          t={t}
          editing={dialog.editing}
          connections={connections}
          saving={saving}
          errorMessage={dialogError}
          onDismissError={() => setDialogError(null)}
          onSave={(input) => void saveConnection(input)}
          onClose={() => { setDialog(null); setDialogError(null); }}
        />
      ) : null}

      <AlertDialog open={Boolean(deleting)} onOpenChange={(open) => { if (!open && !saving) { setDeleting(null); setDeleteError(null); } }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('connections.management.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('connections.management.deleteDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          {deleteError ? (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              <span className="min-w-0 flex-1">{deleteError}</span>
              <button type="button" onClick={() => setDeleteError(null)} aria-label={t('connections.dialog.dismissError')} className="shrink-0 rounded p-0.5 hover:bg-destructive/10">
                <X className="size-3.5" />
              </button>
            </div>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={saving}>{t('connections.management.cancel')}</AlertDialogCancel>
            <AlertDialogAction disabled={saving} className="bg-destructive text-destructive-foreground hover:bg-destructive/90" onClick={(event) => {
              event.preventDefault();
              void confirmDelete();
            }}>{t('connections.management.confirmDelete')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}
