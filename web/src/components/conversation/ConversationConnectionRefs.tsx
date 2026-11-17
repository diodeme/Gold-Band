import { Cable, Loader2, Plus, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { ConnectionRefVm, ServerConnectionVm } from '../../types';
import { listServerConnections } from '@/api';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ConversationConnectionRefsProps {
  /** Non-secret connections bound to this conversation (never a key path/secret). */
  connections: ConnectionRefVm[];
  /** Unbinds a connection: it leaves the agent's hidden context and tool scope
   *  on the next turn. */
  onRemove: (connectionId: string) => Promise<void>;
  /** Binds a connection to this conversation (the agent's context and tool scope
   *  pick it up on the next turn). Omitting it hides the attach entry. */
  onAttach?: (connectionId: string) => Promise<void>;
  /** Connections available to attach (already-bound ones are filtered out). */
  availableConnections?: ServerConnectionVm[];
}

/**
 * The conversation's persistent "connection reference" card: a compact row of
 * bound connections at the top of the conversation. Hover/click reveals the full
 * spec; each chip can be removed (removing = dropping it from the agent's
 * context, per the connection→conversation design). The trailing "+ attach
 * connection" entry binds an available connection to the conversation.
 *
 * Each chip / the attach menu owns its own state so opening one chip or
 * removing/adding a connection never re-renders the conversation body or the
 * other chips.
 */
export function ConversationConnectionRefs({
  connections,
  onRemove,
  onAttach,
  availableConnections,
}: ConversationConnectionRefsProps) {
  const { t } = useTranslation();
  const boundIds = new Set(connections.map((connection) => connection.id));
  const attachable = (availableConnections ?? []).filter(
    (connection) => !boundIds.has(connection.id),
  );
  const showAttach = Boolean(onAttach) && attachable.length > 0;
  if (connections.length === 0 && !showAttach) {
    return null;
  }
  return (
    <div
      data-conversation-connection-refs="true"
      className="flex min-w-0 flex-wrap items-center gap-1.5"
      aria-label={t('connections.reference.groupLabel')}
    >
      {connections.map((connection) => (
        <ConnectionRefChip key={connection.id} connection={connection} onRemove={onRemove} />
      ))}
      {showAttach && onAttach ? (
        <ConnectionAttachMenu connections={attachable} onAttach={onAttach} />
      ) : null}
    </div>
  );
}

/**
 * Page-level wrapper: owns the available-connections list (the only consumer
 * boundary) and renders the refs row whenever attach/remove handlers are
 * provided. Rendered directly by the run page so Direct conversations — which
 * replace the run header with their own title — keep the entry too.
 */
export function ConversationConnectionRefsRow({
  connections,
  onRemove,
  onAttach,
}: {
  connections: ConnectionRefVm[];
  onRemove?: (connectionId: string) => Promise<void>;
  onAttach?: (connectionId: string) => Promise<void>;
}) {
  const [availableConnections, setAvailableConnections] = useState<ServerConnectionVm[]>([]);
  const loadConnections = onAttach !== undefined;
  useEffect(() => {
    if (!loadConnections) return;
    let cancelled = false;
    listServerConnections()
      .then((connections) => {
        if (!cancelled) setAvailableConnections(connections);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [loadConnections]);

  if (!onRemove && !onAttach) {
    return null;
  }
  return (
    <div className="shrink-0 border-b border-border/60 bg-content-header px-5 py-1">
      <ConversationConnectionRefs
        connections={connections}
        onRemove={onRemove ?? (() => Promise.resolve())}
        onAttach={onAttach}
        availableConnections={availableConnections}
      />
    </div>
  );
}

/**
 * Home-composer attach picker (before the conversation's first turn exists, so
 * there is no run to bind to yet). The selection is draft state owned by the
 * parent; after the run is created the parent binds it and clears the
 * selection. Self-contained: loads the connection list itself and renders
 * nothing when there is nothing to attach.
 */
export function ConversationHomeAttach({
  selectedId,
  onSelect,
  disabled,
}: {
  selectedId: string | null;
  onSelect: (connectionId: string | null) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [connections, setConnections] = useState<ServerConnectionVm[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    listServerConnections()
      .then((connections) => {
        if (!cancelled) setConnections(connections);
      })
      .catch(() => {
        if (!cancelled) setConnections([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!connections || connections.length === 0) {
    return null;
  }
  const selected = connections.find((connection) => connection.id === selectedId) ?? null;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1.5 rounded-full px-2 text-xs font-normal text-muted-foreground"
          aria-label={t('connections.reference.attach')}
          disabled={disabled}
        >
          <Cable className="size-3.5" aria-hidden="true" />
          {selected ? (
            <span className="max-w-28 truncate text-foreground">{selected.name}</span>
          ) : (
            <span className="hidden sm:inline">{t('connections.reference.attach')}</span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        {connections.map((connection) => (
          <DropdownMenuItem
            key={connection.id}
            className="gap-2"
            onSelect={() => onSelect(selectedId === connection.id ? null : connection.id)}
          >
            <Cable className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">{connection.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {connection.host}:{connection.port}
            </span>
            {selectedId === connection.id ? (
              <span className="shrink-0 text-xs text-primary" aria-hidden="true">✓</span>
            ) : null}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function ConnectionAttachMenu({
  connections,
  onAttach,
}: {
  connections: ServerConnectionVm[];
  onAttach: (connectionId: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [attachingId, setAttachingId] = useState<string | null>(null);
  const [attachFailed, setAttachFailed] = useState(false);

  const handleAttach = async (connectionId: string) => {
    if (attachingId) return;
    setAttachingId(connectionId);
    setAttachFailed(false);
    try {
      await onAttach(connectionId);
    } catch {
      setAttachFailed(true);
    } finally {
      setAttachingId(null);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 gap-1 rounded-md border border-dashed border-border/60 px-2 text-xs font-normal text-muted-foreground"
          aria-label={t('connections.reference.attach')}
          disabled={attachingId !== null}
        >
          {attachingId ? (
            <Loader2 className="size-3 animate-spin" aria-hidden="true" />
          ) : (
            <Plus className="size-3" aria-hidden="true" />
          )}
          {t('connections.reference.attach')}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        {connections.map((connection) => (
          <DropdownMenuItem
            key={connection.id}
            className="gap-2"
            disabled={attachingId !== null}
            onSelect={() => void handleAttach(connection.id)}
          >
            <Cable className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">{connection.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {connection.host}:{connection.port}
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
      {attachFailed ? (
        <p className="text-xs text-destructive" role="alert">
          {t('connections.reference.attachFailed')}
        </p>
      ) : null}
    </DropdownMenu>
  );
}

function ConnectionRefChip({
  connection,
  onRemove,
}: {
  connection: ConnectionRefVm;
  onRemove: (connectionId: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [removeFailed, setRemoveFailed] = useState(false);

  const envLabel = t(
    connection.env === 'test'
      ? 'connections.dialog.envTest'
      : connection.env === 'stage'
        ? 'connections.dialog.envStage'
        : 'connections.dialog.envProd',
  );
  const authLabel = t(
    connection.authMethod === 'publicKey'
      ? 'connections.dialog.authPublicKey'
      : 'connections.dialog.authPassword',
  );

  const handleRemove = async () => {
    if (removing) return;
    setRemoving(true);
    setRemoveFailed(false);
    try {
      await onRemove(connection.id);
      setOpen(false);
    } catch {
      setRemoveFailed(true);
    } finally {
      setRemoving(false);
    }
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 max-w-[16rem] gap-1.5 rounded-md border border-border/60 px-2 text-xs font-normal"
                aria-expanded={open}
              >
                <Cable className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="truncate text-foreground">{connection.name}</span>
                <span className="truncate text-muted-foreground">
                  {connection.host}:{connection.port}
                </span>
              </Button>
            </PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent>
            {connection.name} · {connection.host}:{connection.port}
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <PopoverContent align="start" sideOffset={4} collisionPadding={12} className="w-72 p-3">
        <div className="flex flex-col gap-2">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-foreground">{connection.name}</div>
              <div className="truncate text-xs text-muted-foreground">
                {connection.host}:{connection.port}
              </div>
            </div>
            <span className="shrink-0 rounded border border-border/60 px-1.5 py-0.5 text-[10px] text-muted-foreground">
              {envLabel}
            </span>
          </div>

          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs">
            <dt className="text-muted-foreground">{t('connections.dialog.username')}</dt>
            <dd className="truncate text-foreground">{connection.username}</dd>
            <dt className="text-muted-foreground">{t('connections.dialog.authMethod')}</dt>
            <dd className="truncate text-foreground">{authLabel}</dd>
            {connection.jumpRef ? (
              <>
                <dt className="text-muted-foreground">{t('connections.dialog.jump')}</dt>
                <dd className="break-all text-foreground">{connection.jumpRef}</dd>
              </>
            ) : null}
          </dl>

          {removeFailed ? (
            <p className="text-xs text-destructive" role="alert">
              {t('connections.reference.removeFailed')}
            </p>
          ) : null}

          <Button
            variant="ghost"
            size="sm"
            className={cn('h-7 justify-start gap-1.5 px-2 text-xs text-destructive')}
            onClick={() => void handleRemove()}
            disabled={removing}
          >
            <Trash2 className="size-3.5" aria-hidden="true" />
            {removing ? t('connections.reference.removing') : t('connections.reference.remove')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
