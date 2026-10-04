import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';
import { ChevronDown, FileDiff, FileMinus2, FilePlus2, FileText, Info, Paperclip } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import {
  fileChangeSetOwnerBranch,
  loadTurnFileChangeSet,
  readCachedTurnFileChangeSet,
  turnFileChangeSetCacheKey,
} from '@/lib/turn-file-change-set-cache';
import type {
  AcpUiEventVm,
  TurnAttachmentVm,
  TurnFileChangeSetVm,
  TurnFileChangeSummaryVm,
  TurnFileChangeVm,
  TurnFileLocatorVm,
} from '@/types';
import { useOptionalRightWorkspaceCommands } from '@/components/workspace/right-workspace-context';
import { TurnFileDiffPreview } from './TurnFileDiffPreview';

export const DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT = 3;
export const DEFAULT_TURN_ATTACHMENT_CARD_PREVIEW_LIMIT = 1;
export const TURN_FILE_HOVER_OPEN_DELAY_MS = 350;
export const TURN_FILE_HOVER_CLOSE_DELAY_MS = 150;
export const TURN_FILE_HOVER_DEBUG_STORAGE_KEY = 'goldBand.debug.turnFileHover';
export const TurnFileCardPreviewLimitContext = createContext(DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT);
export const TurnAttachmentCardPreviewLimitContext = createContext(DEFAULT_TURN_ATTACHMENT_CARD_PREVIEW_LIMIT);

const TURN_FILE_HOVER_LOG_PREFIX = '[GoldBand][Turn file hover]';
// Single files, grouped files and their edits share one row shape.
const FILE_LIST_CLASS = 'divide-y divide-border/35';
const FILE_ROW_CLASS = 'flex h-8 w-full items-center gap-2 px-3 text-left';
const FILE_ROW_INTERACTIVE_CLASS = 'outline-none hover:bg-muted/40 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring';
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
  const ownerBranch = fileChangeSetOwnerBranch(raw);
  const belongsToLocator = !ownerBranch || !locator || ownerBranch === locator.branchId;
  const inlineSummary = summaryValue(raw?.summary);
  const inlineAttachmentCount = numberValue(raw?.attachmentCount);
  const locatorKey = locator
    ? [locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.branchId, locator.outerNodeId, locator.outerAttemptId].join('\0')
    : '';
  const requestKey = locator && changeSetId && belongsToLocator ? turnFileChangeSetCacheKey(locator, changeSetId) : '';
  const initialChangeSet = locator && changeSetId && belongsToLocator ? readCachedTurnFileChangeSet(locator, changeSetId) : null;
  const [loadState, setLoadState] = useState<{ key: string; changeSet: TurnFileChangeSetVm | null; error: boolean }>(() => ({
    key: requestKey,
    changeSet: initialChangeSet,
    error: false,
  }));

  useEffect(() => {
    if (!locator || !changeSetId || !belongsToLocator) return;
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
  const incomplete = (changeSet?.status ?? event.status) === 'partial';
  const changes = changeSet?.changes ?? [];
  const attachments = changeSet?.attachments ?? [];
  const workspaceRoot = changeSet?.workspaceRoot ?? null;
  const fileGroups = useMemo(() => {
    const groups = new Map<string, TurnFileChangeVm[]>();
    for (const change of changeSet?.changes ?? []) {
      const edits = groups.get(change.logicalPath) ?? [];
      edits.push(change);
      groups.set(change.logicalPath, edits);
    }
    return [...groups.values()];
  }, [changeSet]);
  const previewGroups = fileGroups.slice(0, previewLimit);
  const hiddenCount = Math.max(0, fileGroups.length - previewLimit);
  const attachmentCount = changeSet ? attachments.length : inlineAttachmentCount;
  const hasRegularChanges = (summary?.fileCount ?? 0) > 0 || incomplete;
  if (!changeSetId || (!hasRegularChanges && attachmentCount === 0) || !belongsToLocator) return null;

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
    <>
      <TurnAttachmentsCard
        attachments={attachments}
        count={attachmentCount}
        error={error}
        locator={locator}
        changeSetId={changeSetId}
      />
      {hasRegularChanges && summary ? (
      <Card className="mb-3 w-full max-w-[46rem] gap-0 overflow-hidden py-0" data-turn-file-changes-card={changeSetId}>
      <CardHeader className="grid-cols-[1fr_auto] items-center gap-3 px-3 py-2.5">
        <CardTitle className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <FileDiff className="size-4 shrink-0 text-foreground" />
          <span>{t(incomplete && summary.fileCount === 0 ? 'turnFiles.incompleteTitle' : 'turnFiles.title', { count: summary.fileCount })}</span>
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button" aria-label={t('turnFiles.recordedOnly')} className="inline-flex size-6 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                <Info className="size-3.5" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent className="max-w-[min(20rem,calc(100vw-2rem))]">
              {t('turnFiles.recordedOnly')}
            </TooltipContent>
          </Tooltip>
        </CardTitle>
        {!incomplete && <div className="flex items-center gap-2 text-xs tabular-nums">
          <span className="text-emerald-600 dark:text-emerald-400">+{summary.addedLines}</span>
          <span className="text-destructive">-{summary.deletedLines}</span>
        </div>}
      </CardHeader>
      <CardContent className="border-t border-border/50 px-0 py-0">
        {error ? (
          <div className="px-3 py-2 text-xs text-destructive">{t('turnFiles.loadFailed')}</div>
        ) : changes.length === 0 ? (
          <div className="px-3 py-2 text-xs text-muted-foreground">{t(incomplete ? 'turnFiles.partial' : 'turnFiles.loading')}</div>
        ) : (
          <Collapsible open={expanded} onOpenChange={handleOpenChange}>
            {!expanded ? (
              <div role="list" aria-label={t('turnFiles.fileList')} className={FILE_LIST_CLASS}>
                {previewGroups.map((edits) => (
                  <RecordedFileGroup key={edits[0]!.logicalPath} edits={edits} workspaceRoot={workspaceRoot} locator={locator} changeSetId={changeSetId} onOpen={openChange} />
                ))}
              </div>
            ) : null}
            <CollapsibleContent className={cn(
              'overflow-hidden',
              hasUserToggled && 'data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down',
            )}>
              <ScrollArea className={cn(fileGroups.length > 8 ? 'h-64' : 'h-auto')}>
                <div role="list" aria-label={t('turnFiles.fileList')} className={FILE_LIST_CLASS}>
                  {fileGroups.map((edits) => (
                    <RecordedFileGroup key={edits[0]!.logicalPath} edits={edits} workspaceRoot={workspaceRoot} locator={locator} changeSetId={changeSetId} onOpen={openChange} />
                  ))}
                </div>
              </ScrollArea>
            </CollapsibleContent>
            {hiddenCount > 0 ? (
              <CollapsibleTrigger asChild>
                <Button type="button" variant="ghost" className="h-8 w-full justify-center gap-1 rounded-none border-t border-border/40 text-xs text-muted-foreground" aria-label={expanded ? t('turnFiles.collapse') : t('turnFiles.showMore', { count: hiddenCount })}>
                  <ChevronDown className={cn('size-3.5 transition-transform', expanded && 'rotate-180')} />
                  {expanded ? t('turnFiles.collapse') : t('turnFiles.showMore', { count: hiddenCount })}
                </Button>
              </CollapsibleTrigger>
            ) : null}
          </Collapsible>
        )}
      </CardContent>
      </Card>
      ) : null}
    </>
  );
}

function TurnAttachmentsCard({
  attachments,
  count,
  error,
  locator,
  changeSetId,
}: {
  attachments: TurnAttachmentVm[];
  count: number;
  error: boolean;
  locator: TurnFileLocatorVm | null;
  changeSetId: string;
}) {
  const { t } = useTranslation();
  const workspace = useOptionalRightWorkspaceCommands();
  const configuredPreviewLimit = useContext(TurnAttachmentCardPreviewLimitContext);
  const previewLimit = Math.max(1, Math.floor(configuredPreviewLimit));
  const [expanded, setExpanded] = useState(false);
  const [hasUserToggled, setHasUserToggled] = useState(false);
  const previewAttachments = attachments.slice(0, previewLimit);
  const hiddenCount = Math.max(0, attachments.length - previewLimit);
  if (count === 0) return null;

  const openAttachment = (attachment: TurnAttachmentVm) => {
    if (!workspace?.scopeKey || !locator) return;
    void workspace.openResource({
      kind: 'turn-attachment',
      key: `turn-attachment:${changeSetId}:${attachment.id}`,
      scopeKey: workspace.scopeKey,
      title: attachment.name,
      description: attachment.relativePath,
      attention: false,
      locator,
      changeSetId,
      attachmentId: attachment.id,
    });
  };
  const handleOpenChange = (open: boolean) => {
    setHasUserToggled(true);
    setExpanded(open);
  };

  return (
    <Card className="mb-3 w-full max-w-[46rem] gap-0 overflow-hidden py-0" data-turn-attachments-card={changeSetId}>
      <CardHeader className="px-3 py-2.5">
        <CardTitle className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <Paperclip className="size-4 shrink-0 text-foreground" />
          <span>{t('turnFiles.attachmentsTitle', { count })}</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="border-t border-border/50 px-0 py-0">
        {error ? (
          <div className="px-3 py-2 text-xs text-destructive">{t('turnFiles.loadFailed')}</div>
        ) : attachments.length === 0 ? (
          <div className="px-3 py-2 text-xs text-muted-foreground">{t('turnFiles.loading')}</div>
        ) : (
          <Collapsible open={expanded} onOpenChange={handleOpenChange}>
            {!expanded ? (
              <div role="list" aria-label={t('turnFiles.attachmentList')}>
                {previewAttachments.map((attachment) => (
                  <TurnAttachmentRow key={attachment.id} attachment={attachment} onOpen={openAttachment} />
                ))}
              </div>
            ) : null}
            <CollapsibleContent className={cn(
              'overflow-hidden',
              hasUserToggled && 'data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down',
            )}>
              <ScrollArea className={cn(attachments.length > 8 ? 'h-64' : 'h-auto')}>
                <div role="list" aria-label={t('turnFiles.attachmentList')}>
                  {attachments.map((attachment) => (
                    <TurnAttachmentRow key={attachment.id} attachment={attachment} onOpen={openAttachment} />
                  ))}
                </div>
              </ScrollArea>
            </CollapsibleContent>
            {hiddenCount > 0 ? (
              <CollapsibleTrigger asChild>
                <Button type="button" variant="ghost" className="h-8 w-full justify-center gap-1 rounded-none border-t border-border/40 text-xs text-muted-foreground" aria-label={expanded ? t('turnFiles.collapse') : t('turnFiles.showMoreAttachments', { count: hiddenCount })}>
                  <ChevronDown className={cn('size-3.5 transition-transform', expanded && 'rotate-180')} />
                  {expanded ? t('turnFiles.collapse') : t('turnFiles.showMoreAttachments', { count: hiddenCount })}
                </Button>
              </CollapsibleTrigger>
            ) : null}
          </Collapsible>
        )}
      </CardContent>
    </Card>
  );
}

function TurnAttachmentRow({
  attachment,
  onOpen,
}: {
  attachment: TurnAttachmentVm;
  onOpen: (attachment: TurnAttachmentVm) => void;
}) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      role="listitem"
      className="flex h-10 w-full items-center gap-2 px-3 text-left outline-none hover:bg-muted/40 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      onClick={() => onOpen(attachment)}
      aria-label={t('turnFiles.openAttachment', { path: attachment.relativePath })}
    >
      <FileText className="size-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate text-xs text-foreground">{attachment.name}</span>
      <span className="shrink-0 text-ui-micro tabular-nums text-muted-foreground">{formatByteLength(attachment.byteLength)}</span>
    </button>
  );
}

function RecordedFileGroup({ edits, workspaceRoot, locator, changeSetId, onOpen }: {
  edits: TurnFileChangeVm[];
  workspaceRoot: string | null;
  locator: TurnFileLocatorVm | null;
  changeSetId: string;
  onOpen: (change: TurnFileChangeVm) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const first = edits[0]!;
  const path = splitDisplayPath(first.logicalPath, workspaceRoot);
  if (edits.length === 1) {
    return (
      <TurnFileChangeRow change={first} locator={locator} changeSetId={changeSetId} onOpen={onOpen}>
        <FileRowContent kind={first.changeKind} name={path.name} directory={path.directory} stats={recordedStats(edits)} />
      </TurnFileChangeRow>
    );
  }
  return (
    <Collapsible open={expanded} onOpenChange={setExpanded}>
      <CollapsibleTrigger asChild>
        <button
          type="button"
          role="listitem"
          data-recorded-file-group={first.logicalPath}
          className={cn(FILE_ROW_CLASS, FILE_ROW_INTERACTIVE_CLASS)}
          aria-label={`${first.logicalPath} · ${t('turnFiles.editCount', { count: edits.length })}`}
        >
          <FileRowContent
            kind={pathChangeKind(edits)}
            name={path.name}
            directory={path.directory}
            stats={recordedStats(edits)}
            trailing={(
              <>
                <span data-file-edit-count className="tabular-nums">×{edits.length}</span>
                <ChevronDown className={cn('size-3.5 shrink-0 transition-transform', expanded && 'rotate-180')} />
              </>
            )}
          />
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ScrollArea className={edits.length > 6 ? 'h-60' : 'h-auto'}>
          <div className={cn(FILE_LIST_CLASS, 'border-t border-border/35')}>
            {expanded && edits.map((change, index) => (
              <div key={change.id} data-recorded-edit={change.id} className="pl-5">
                <TurnFileChangeRow change={change} locator={locator} changeSetId={changeSetId} onOpen={onOpen}>
                  <FileRowContent kind={change.changeKind} name={t('turnFiles.editNumber', { number: index + 1 })} stats={recordedStats([change])} />
                </TurnFileChangeRow>
              </div>
            ))}
          </div>
        </ScrollArea>
      </CollapsibleContent>
    </Collapsible>
  );
}

function FileRowContent({ kind, name, directory, stats, trailing }: {
  kind: TurnFileChangeVm['changeKind'];
  name: string;
  directory?: string;
  stats: { added: number; deleted: number } | null;
  trailing?: ReactNode;
}) {
  const icon = kind === 'added'
    ? <FilePlus2 className="size-3.5 shrink-0 text-emerald-600 dark:text-emerald-400" />
    : kind === 'deleted'
      ? <FileMinus2 className="size-3.5 shrink-0 text-destructive" />
      : <FileDiff className="size-3.5 shrink-0 text-gold-running" />;
  return (
    <>
      {icon}
      <span className="flex min-w-0 flex-1 items-baseline gap-1.5">
        <span data-file-name className="min-w-0 max-w-full shrink-0 truncate text-xs font-medium text-foreground">{name}</span>
        {directory ? (
          // RTL keeps the tail of a long directory visible and elides its start.
          <span className="min-w-0 flex-1 truncate text-left text-ui-micro text-muted-foreground [direction:rtl]">
            <bdi data-file-directory>{directory}</bdi>
          </span>
        ) : null}
      </span>
      {trailing ? (
        <span className="flex shrink-0 items-center gap-0.5 text-xs text-muted-foreground">{trailing}</span>
      ) : null}
      {/* Stats are always the last column so every row aligns with the header total. */}
      <span data-file-stats className="flex shrink-0 gap-2 text-xs tabular-nums">
        {stats ? (
          <>
            <span className="text-emerald-600 dark:text-emerald-400">+{stats.added}</span>
            <span className="text-destructive">-{stats.deleted}</span>
          </>
        ) : null}
      </span>
    </>
  );
}

function TurnFileChangeRow({
  change,
  locator,
  changeSetId,
  onOpen,
  children,
}: {
  change: TurnFileChangeVm;
  locator: TurnFileLocatorVm | null;
  changeSetId: string;
  onOpen: (change: TurnFileChangeVm) => void;
  children: ReactNode;
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
  const content = children;
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
      className={cn(FILE_ROW_CLASS, FILE_ROW_INTERACTIVE_CLASS, 'cursor-default text-muted-foreground')}
      aria-label={t('turnFiles.previewDeleted', { path: change.logicalPath })}
    >
      {content}
    </div>
  ) : (
    <button type="button" role="listitem" className={cn(FILE_ROW_CLASS, FILE_ROW_INTERACTIVE_CLASS)} onFocus={handlePreviewFocus} onBlur={handlePreviewBlur} onPointerDown={handlePreviewPointerDown} onPointerUp={handlePreviewPointerUp} onPointerCancel={handlePreviewPointerCancel} onPointerEnter={handlePreviewPointerEnter} onPointerLeave={handlePreviewPointerLeave} onPointerMove={handlePreviewPointerMove} onClick={openWorkspaceChange} aria-label={t(change.changeKind === 'added' ? 'turnFiles.openVersion' : 'turnFiles.openDiff', { path: change.logicalPath })}>
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

/** Paths under the recorded workspace root are shown relative; others stay absolute. */
export function splitDisplayPath(logicalPath: string, workspaceRoot: string | null) {
  const path = logicalPath.replaceAll('\\', '/');
  const root = workspaceRoot?.replaceAll('\\', '/').replace(/\/+$/u, '') ?? '';
  // Drive-letter roots compare case-insensitively, matching Windows path identity.
  const caseless = /^[a-z]:\//iu.test(root);
  const prefix = `${root}/`;
  const inside = root.length > 0 && (caseless
    ? path.slice(0, prefix.length).toLowerCase() === prefix.toLowerCase()
    : path.startsWith(prefix));
  const shown = inside ? path.slice(prefix.length) : path;
  const slash = shown.lastIndexOf('/');
  return { name: shown.slice(slash + 1) || shown, directory: slash > 0 ? shown.slice(0, slash) : '' };
}

/** Sum of each recorded comparison; unknown when any edit lacks statistics. */
function recordedStats(edits: TurnFileChangeVm[]) {
  let added = 0;
  let deleted = 0;
  for (const edit of edits) {
    if (edit.addedLines == null || edit.deletedLines == null) return null;
    added += edit.addedLines;
    deleted += edit.deletedLines;
  }
  return { added, deleted };
}

/** Existence across the turn decides the kind; unlinked edits in between do not. */
function pathChangeKind(edits: TurnFileChangeVm[]): TurnFileChangeVm['changeKind'] {
  const first = edits[0]!.changeKind;
  const last = edits[edits.length - 1]!.changeKind;
  if (edits.length === 1) return first;
  if (first === 'added' && last !== 'deleted') return 'added';
  if (last === 'deleted' && first !== 'added') return 'deleted';
  return 'modified';
}

function formatByteLength(byteLength: number) {
  if (byteLength < 1024) return `${byteLength} B`;
  if (byteLength < 1024 * 1024) return `${Math.max(0.1, byteLength / 1024).toFixed(1)} KB`;
  return `${Math.max(0.1, byteLength / (1024 * 1024)).toFixed(1)} MB`;
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
