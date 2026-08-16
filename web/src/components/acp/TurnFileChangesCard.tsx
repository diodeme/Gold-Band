import { createContext, useContext, useEffect, useState } from 'react';
import { ChevronDown, FileDiff, FileMinus2, FilePlus2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
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

export const DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT = 3;
export const TurnFileCardPreviewLimitContext = createContext(DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT);

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
                  <TurnFileChangeRow key={change.id} change={change} onOpen={openChange} />
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
                    <TurnFileChangeRow key={change.id} change={change} onOpen={openChange} />
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

function TurnFileChangeRow({ change, onOpen }: { change: TurnFileChangeVm; onOpen: (change: TurnFileChangeVm) => void }) {
  const { t } = useTranslation();
  const icon = change.changeKind === 'added'
    ? <FilePlus2 className="size-3.5 text-emerald-600 dark:text-emerald-400" />
    : change.changeKind === 'deleted'
      ? <FileMinus2 className="size-3.5 text-destructive" />
      : <FileDiff className="size-3.5 text-gold-running" />;
  const content = (
    <>
      {icon}
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{change.logicalPath}</span>
        </TooltipTrigger>
        <TooltipContent className="max-w-[360px] break-all">{change.logicalPath}</TooltipContent>
      </Tooltip>
      <span className="text-xs tabular-nums text-emerald-600 dark:text-emerald-400">+{change.addedLines ?? 0}</span>
      <span className="text-xs tabular-nums text-destructive">-{change.deletedLines ?? 0}</span>
    </>
  );
  const className = 'flex h-8 w-full items-center gap-2 border-b border-border/35 px-3 text-left last:border-b-0';
  if (change.changeKind === 'deleted') {
    return <div role="listitem" className={cn(className, 'cursor-default text-muted-foreground')} aria-label={t('turnFiles.deletedFile', { path: change.logicalPath })}>{content}</div>;
  }
  return (
    <button type="button" role="listitem" className={cn(className, 'outline-none hover:bg-muted/40 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring')} onClick={() => onOpen(change)} aria-label={t(change.changeKind === 'added' ? 'turnFiles.openVersion' : 'turnFiles.openDiff', { path: change.logicalPath })}>
      {content}
    </button>
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
