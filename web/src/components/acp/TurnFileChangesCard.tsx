import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import { ChevronDown, FileDiff, FileMinus2, FilePlus2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import {
  loadTurnFileChangeSet,
  readCachedTurnFileChangeSet,
  turnFileChangeSetCacheKey,
} from '@/lib/turn-file-change-set-cache';
import type {
  AcpUiEventVm,
  TurnFileChangeSetVm,
  TurnFileChangeSummaryVm,
  TurnFileChangeVm,
  TurnFileLocatorVm,
} from '@/types';
import { useOptionalRightWorkspaceCommands } from '@/components/workspace/right-workspace-context';
import { TurnFileDiffPreview } from './TurnFileDiffPreview';

export const DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT = 3;
export const TURN_FILE_HOVER_OPEN_DELAY_MS = 350;
export const TURN_FILE_HOVER_CLOSE_DELAY_MS = 150;
export const TURN_FILE_HOVER_DEBUG_STORAGE_KEY = 'goldBand.debug.turnFileHover';
export const TurnFileCardPreviewLimitContext = createContext(DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT);

const TURN_FILE_HOVER_LOG_PREFIX = '[GoldBand][Turn file hover]';
let turnFileHoverInstanceSequence = 0;
let turnFileHoverLogSequence = 0;

type TurnFileHoverSuppression =
  | { kind: 'keyboard' }
  | { kind: 'pointer'; clientX: number; clientY: number };

export function TurnFileChangesCard({ event, locator }: { event: AcpUiEventVm; locator: TurnFileLocatorVm | null }) {
  const { t } = useTranslation();
  const configuredPreviewLimit = useContext(TurnFileCardPreviewLimitContext);
  const previewLimit = Math.max(1, Math.floor(configuredPreviewLimit));
  const workspace = useOptionalRightWorkspaceCommands();
  const [expanded, setExpanded] = useState(false);
  const [hasUserToggled, setHasUserToggled] = useState(false);
  const raw = objectValue(event.raw);
  const changeSetId = stringValue(raw?.changeSetId);
  const inlineSummary = summaryValue(raw?.summary);
  const locatorKey = locator
    ? [locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.branchId, locator.outerNodeId, locator.outerAttemptId].join('\0')
    : '';
  const requestKey = locator && changeSetId ? turnFileChangeSetCacheKey(locator, changeSetId) : '';
  const initialChangeSet = locator && changeSetId ? readCachedTurnFileChangeSet(locator, changeSetId) : null;
  const [loadState, setLoadState] = useState<{ key: string; changeSet: TurnFileChangeSetVm | null; error: boolean }>(() => ({
    key: requestKey,
    changeSet: initialChangeSet,
    error: false,
  }));

  useEffect(() => {
    if (!locator || !changeSetId) return;
    let cancelled = false;
    const cached = readCachedTurnFileChangeSet(locator, changeSetId);
    setLoadState({ key: requestKey, changeSet: cached, error: false });
    if (cached) return () => { cancelled = true; };
    void loadTurnFileChangeSet(locator, changeSetId)
      .then((next) => { if (!cancelled) setLoadState({ key: requestKey, changeSet: next, error: false }); })
      .catch(() => { if (!cancelled) setLoadState({ key: requestKey, changeSet: null, error: true }); });
    return () => { cancelled = true; };
  // The primitive locator key prevents completed cards from refetching when a provider value is recreated.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [changeSetId, locatorKey, requestKey]);

  const changeSet = loadState.key === requestKey ? loadState.changeSet : initialChangeSet;
  const error = loadState.key === requestKey && loadState.error;
  const summary = changeSet?.summary ?? inlineSummary;
  const changes = changeSet?.changes ?? [];
  const previewChanges = changes.slice(0, previewLimit);
  const hiddenCount = Math.max(0, changes.length - previewLimit);
  if (!summary || summary.fileCount === 0 || !changeSetId) return null;

  const handleOpenChange = (open: boolean) => {
    setHasUserToggled(true);
    setExpanded(open);
  };

  const openChange = (change: TurnFileChangeVm) => {
    if (!workspace?.scopeKey || !locator || change.changeKind === 'deleted') return;
    const kind = change.changeKind === 'added' ? 'file-version' : 'file-diff';
    void workspace.openResource({
      kind,
      key: `${kind}:${changeSetId}:${change.id}`,
      scopeKey: workspace.scopeKey,
      title: fileName(change.logicalPath),
      description: change.logicalPath,
      // A capture/rendering limitation is explained by the read-only viewer itself.
      // Opening a diff is ordinary navigation, not a Tab-level attention event.
      attention: false,
      locator,
      changeSetId,
      changeId: change.id,
    });
  };

  return (
    <Card className="w-full max-w-[46rem] gap-0 overflow-hidden py-0" data-turn-file-changes-card={changeSetId}>
      <CardHeader className="grid-cols-[1fr_auto] items-center gap-3 px-3 py-2.5">
        <CardTitle className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <FileDiff className="size-4 shrink-0 text-foreground" />
          <span>{t('turnFiles.title', { count: summary.fileCount })}</span>
        </CardTitle>
        <div className="flex items-center gap-2 text-xs tabular-nums">
          <span className="text-emerald-600 dark:text-emerald-400">+{summary.addedLines}</span>
          <span className="text-destructive">-{summary.deletedLines}</span>
        </div>
      </CardHeader>
      <CardContent className="border-t border-border/50 px-0 py-0">
        {error ? (
          <div className="px-3 py-2 text-xs text-destructive">{t('turnFiles.loadFailed')}</div>
        ) : changes.length === 0 ? (
          <div className="px-3 py-2 text-xs text-muted-foreground">{t('turnFiles.loading')}</div>
        ) : (
          <Collapsible open={expanded} onOpenChange={handleOpenChange}>
            {!expanded ? (
              <div role="list" aria-label={t('turnFiles.fileList')}>
                {previewChanges.map((change) => (
                  <TurnFileChangeRow key={change.id} change={change} locator={locator} changeSetId={changeSetId} onOpen={openChange} />
                ))}
              </div>
            ) : null}
            <CollapsibleContent className={cn(
              'overflow-hidden',
              hasUserToggled && 'data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down',
            )}>
              <ScrollArea className={cn(changes.length > 8 ? 'h-64' : 'h-auto')}>
                <div role="list" aria-label={t('turnFiles.fileList')}>
                  {changes.map((change) => (
                    <TurnFileChangeRow key={change.id} change={change} locator={locator} changeSetId={changeSetId} onOpen={openChange} />
                  ))}
                </div>
              </ScrollArea>
            </CollapsibleContent>
            {hiddenCount > 0 ? (
              <CollapsibleTrigger asChild>
                <Button type="button" variant="ghost" className="h-8 w-full justify-center gap-1 rounded-none border-t border-border/40 text-xs text-muted-foreground" aria-label={expanded ? t('turnFiles.collapse') : t('turnFiles.showMore', { count: hiddenCount })}>
                  <ChevronDown className={cn('size-3.5 transition-transform motion-reduce:transition-none', expanded && 'rotate-180')} />
                  {expanded ? t('turnFiles.collapse') : t('turnFiles.showMore', { count: hiddenCount })}
                </Button>
              </CollapsibleTrigger>
            ) : null}
          </Collapsible>
        )}
      </CardContent>
    </Card>
  );
}

function TurnFileChangeRow({
  change,
  locator,
  changeSetId,
  onOpen,
}: {
  change: TurnFileChangeVm;
  locator: TurnFileLocatorVm | null;
  changeSetId: string;
  onOpen: (change: TurnFileChangeVm) => void;
}) {
  const { t } = useTranslation();
  const [previewOpen, setPreviewOpen] = useState(false);
  const diagnosticInstanceIdRef = useRef<string | null>(null);
  const previewContentRef = useRef<HTMLDivElement>(null);
  const pointerFocusPendingRef = useRef(false);
  const previewSuppressionRef = useRef<TurnFileHoverSuppression | null>(null);
  if (!diagnosticInstanceIdRef.current) {
    turnFileHoverInstanceSequence += 1;
    diagnosticInstanceIdRef.current = `turn-file-row-${turnFileHoverInstanceSequence}`;
  }
  const diagnosticInstanceId = diagnosticInstanceIdRef.current;
  const logDiagnostic = (event: string, details?: Record<string, unknown>) => {
    logTurnFileHoverDiagnostic({
      event,
      instanceId: diagnosticInstanceId,
      changeId: change.id,
      changeKind: change.changeKind,
      ...details,
    });
  };

  useEffect(() => {
    logTurnFileHoverDiagnostic({
      event: 'row-mount',
      instanceId: diagnosticInstanceId,
      changeId: change.id,
      changeKind: change.changeKind,
    });
    return () => logTurnFileHoverDiagnostic({
      event: 'row-unmount',
      instanceId: diagnosticInstanceId,
      changeId: change.id,
      changeKind: change.changeKind,
    });
  }, [change.changeKind, change.id, diagnosticInstanceId]);

  useEffect(() => {
    logTurnFileHoverDiagnostic({
      event: 'preview-state-commit',
      instanceId: diagnosticInstanceId,
      changeId: change.id,
      changeKind: change.changeKind,
      previewOpen,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
    const content = previewContentRef.current;
    logTurnFileHoverDiagnostic({
      event: 'content-snapshot',
      instanceId: diagnosticInstanceId,
      changeId: change.id,
      changeKind: change.changeKind,
      present: Boolean(content),
      dataState: content?.dataset.state ?? null,
      dataSide: content?.dataset.side ?? null,
    });
    if (!content || !isTurnFileHoverDebugEnabled()) return;
    const observer = new MutationObserver(() => {
      logTurnFileHoverDiagnostic({
        event: 'content-data-state',
        instanceId: diagnosticInstanceId,
        changeId: change.id,
        changeKind: change.changeKind,
        dataState: content.dataset.state ?? null,
        dataSide: content.dataset.side ?? null,
      });
    });
    observer.observe(content, { attributes: true, attributeFilter: ['data-state', 'data-side'] });
    return () => observer.disconnect();
  }, [change.changeKind, change.id, diagnosticInstanceId, previewOpen]);
  const icon = change.changeKind === 'added'
    ? <FilePlus2 className="size-3.5 text-emerald-600 dark:text-emerald-400" />
    : change.changeKind === 'deleted'
      ? <FileMinus2 className="size-3.5 text-destructive" />
      : <FileDiff className="size-3.5 text-gold-running" />;
  const content = (
    <>
      {icon}
      <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{change.logicalPath}</span>
      <span className="text-xs tabular-nums text-emerald-600 dark:text-emerald-400">+{change.addedLines ?? 0}</span>
      <span className="text-xs tabular-nums text-destructive">-{change.deletedLines ?? 0}</span>
    </>
  );
  const className = 'flex h-8 w-full items-center gap-2 border-b border-border/35 px-3 text-left last:border-b-0';
  const openWorkspaceChange = (event: ReactMouseEvent<HTMLButtonElement>) => {
    pointerFocusPendingRef.current = false;
    const nextSuppression: TurnFileHoverSuppression = event.detail === 0
      ? { kind: 'keyboard' }
      : { kind: 'pointer', clientX: event.clientX, clientY: event.clientY };
    logDiagnostic('click', {
      previewOpen,
      pointer: pointerDiagnostic(event),
      previousSuppression: summarizePreviewSuppression(previewSuppressionRef.current),
      nextSuppression: summarizePreviewSuppression(nextSuppression),
    });
    previewSuppressionRef.current = nextSuppression;
    setPreviewOpen(false);
    logDiagnostic('workspace-open-dispatch', { suppression: summarizePreviewSuppression(nextSuppression) });
    onOpen(change);
    logDiagnostic('workspace-open-returned');
  };
  const handlePreviewOpenChange = (open: boolean) => {
    const suppression = previewSuppressionRef.current;
    const pointerFocusBlocked = open && pointerFocusPendingRef.current;
    logDiagnostic('open-request', {
      requestedOpen: open,
      previewOpen,
      blocked: Boolean(pointerFocusBlocked || (open && suppression)),
      blockedByPointerFocus: pointerFocusBlocked,
      suppression: summarizePreviewSuppression(suppression),
    });
    if (pointerFocusBlocked || (open && suppression)) return;
    setPreviewOpen(open);
  };
  const handlePreviewBlur = () => {
    logDiagnostic('blur', {
      previewOpen,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
    if (previewSuppressionRef.current?.kind === 'keyboard') {
      previewSuppressionRef.current = null;
    }
    setPreviewOpen(false);
  };
  const handlePreviewFocus = () => {
    const pointerDriven = pointerFocusPendingRef.current;
    logDiagnostic('focus', {
      previewOpen,
      pointerDriven,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
    if (pointerDriven) return;
    handlePreviewOpenChange(true);
  };
  const handlePreviewPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    pointerFocusPendingRef.current = true;
    logDiagnostic('pointer-down', {
      pointer: pointerDiagnostic(event),
      previewOpen,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
  };
  const handlePreviewPointerUp = (event: ReactPointerEvent<HTMLElement>) => {
    logDiagnostic('pointer-up', {
      pointer: pointerDiagnostic(event),
      previewOpen,
      pointerFocusPending: pointerFocusPendingRef.current,
    });
    pointerFocusPendingRef.current = false;
  };
  const handlePreviewPointerCancel = (event: ReactPointerEvent<HTMLElement>) => {
    logDiagnostic('pointer-cancel', {
      pointer: pointerDiagnostic(event),
      previewOpen,
      pointerFocusPending: pointerFocusPendingRef.current,
    });
    pointerFocusPendingRef.current = false;
  };
  const handlePreviewPointerEnter = (event: ReactPointerEvent<HTMLElement>) => {
    logDiagnostic('pointer-enter', {
      pointer: pointerDiagnostic(event),
      previewOpen,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
  };
  const handlePreviewPointerLeave = (event: ReactPointerEvent<HTMLElement>) => {
    logDiagnostic('pointer-leave', {
      pointer: pointerDiagnostic(event),
      previewOpen,
      suppression: summarizePreviewSuppression(previewSuppressionRef.current),
    });
    pointerFocusPendingRef.current = false;
  };
  const handlePreviewPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const suppression = previewSuppressionRef.current;
    if (suppression) {
      logDiagnostic('pointer-move-suppressed', {
        pointer: pointerDiagnostic(event),
        previewOpen,
        suppression: summarizePreviewSuppression(suppression),
      });
    }
    if (
      suppression?.kind === 'pointer'
      && (event.clientX !== suppression.clientX || event.clientY !== suppression.clientY)
    ) {
      logDiagnostic('pointer-suppression-cleared', {
        pointer: pointerDiagnostic(event),
        suppression: summarizePreviewSuppression(suppression),
      });
      previewSuppressionRef.current = null;
      setPreviewOpen(true);
    }
  };
  const row = change.changeKind === 'deleted' ? (
    <div
      role="listitem"
      tabIndex={0}
      onFocus={handlePreviewFocus}
      onBlur={handlePreviewBlur}
      onPointerDown={handlePreviewPointerDown}
      onPointerUp={handlePreviewPointerUp}
      onPointerCancel={handlePreviewPointerCancel}
      onPointerEnter={handlePreviewPointerEnter}
      onPointerLeave={handlePreviewPointerLeave}
      onPointerMove={handlePreviewPointerMove}
      className={cn(className, 'cursor-default text-muted-foreground outline-none hover:bg-muted/40 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring')}
      aria-label={t('turnFiles.previewDeleted', { path: change.logicalPath })}
    >
      {content}
    </div>
  ) : (
    <button type="button" role="listitem" className={cn(className, 'outline-none hover:bg-muted/40 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring')} onFocus={handlePreviewFocus} onBlur={handlePreviewBlur} onPointerDown={handlePreviewPointerDown} onPointerUp={handlePreviewPointerUp} onPointerCancel={handlePreviewPointerCancel} onPointerEnter={handlePreviewPointerEnter} onPointerLeave={handlePreviewPointerLeave} onPointerMove={handlePreviewPointerMove} onClick={openWorkspaceChange} aria-label={t(change.changeKind === 'added' ? 'turnFiles.openVersion' : 'turnFiles.openDiff', { path: change.logicalPath })}>
      {content}
    </button>
  );
  if (!locator) return row;
  return (
    <HoverCard open={previewOpen} onOpenChange={handlePreviewOpenChange} openDelay={TURN_FILE_HOVER_OPEN_DELAY_MS} closeDelay={TURN_FILE_HOVER_CLOSE_DELAY_MS}>
      <HoverCardTrigger asChild>{row}</HoverCardTrigger>
      <HoverCardContent
        ref={previewContentRef}
        side="top"
        align="start"
        sideOffset={6}
        collisionPadding={8}
        className="w-auto max-w-[calc(100vw-1rem)] overflow-hidden p-0"
      >
        <TurnFileDiffPreview locator={locator} changeSetId={changeSetId} change={change} />
      </HoverCardContent>
    </HoverCard>
  );
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function stringValue(value: unknown) {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function summaryValue(value: unknown): TurnFileChangeSummaryVm | null {
  const summary = objectValue(value);
  if (!summary || typeof summary.fileCount !== 'number') return null;
  return {
    fileCount: summary.fileCount,
    addedFiles: numberValue(summary.addedFiles),
    modifiedFiles: numberValue(summary.modifiedFiles),
    deletedFiles: numberValue(summary.deletedFiles),
    addedLines: numberValue(summary.addedLines),
    deletedLines: numberValue(summary.deletedLines),
  };
}

function numberValue(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function fileName(path: string) {
  return path.replaceAll('\\', '/').split('/').at(-1) || path;
}

function isTurnFileHoverDebugEnabled() {
  if (typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(TURN_FILE_HOVER_DEBUG_STORAGE_KEY) === '1';
  } catch {
    return false;
  }
}

function logTurnFileHoverDiagnostic(details: Record<string, unknown>) {
  if (!isTurnFileHoverDebugEnabled()) return;
  turnFileHoverLogSequence += 1;
  console.info(`${TURN_FILE_HOVER_LOG_PREFIX} ${JSON.stringify({
    sequence: turnFileHoverLogSequence,
    timestamp: new Date().toISOString(),
    ...details,
  })}`);
}

function summarizePreviewSuppression(
  suppression: TurnFileHoverSuppression | null,
) {
  if (!suppression) return null;
  return suppression.kind === 'keyboard'
    ? { kind: suppression.kind }
    : { kind: suppression.kind, clientX: suppression.clientX, clientY: suppression.clientY };
}

function pointerDiagnostic(event: ReactPointerEvent<HTMLElement> | ReactMouseEvent<HTMLElement>) {
  return {
    clientX: event.clientX,
    clientY: event.clientY,
    detail: event.detail,
    pointerType: 'pointerType' in event ? event.pointerType : null,
    movementX: 'movementX' in event ? event.movementX : null,
    movementY: 'movementY' in event ? event.movementY : null,
    buttons: event.buttons,
    relatedTarget: describeDiagnosticTarget(event.relatedTarget),
  };
}

function describeDiagnosticTarget(target: EventTarget | null) {
  if (typeof Element === 'undefined' || !(target instanceof Element)) return null;
  return {
    tag: target.tagName.toLowerCase(),
    role: target.getAttribute('role'),
    slot: target.getAttribute('data-slot'),
  };
}
