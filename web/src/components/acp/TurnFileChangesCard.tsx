import { createContext, useContext, useEffect, useState } from 'react';
import { ChevronDown, FileDiff, FileMinus2, FilePlus2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getTurnFileChangeSet } from '@/api';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
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
  const [changeSet, setChangeSet] = useState<TurnFileChangeSetVm | null>(null);
  const [error, setError] = useState(false);
  const raw = objectValue(event.raw);
  const changeSetId = stringValue(raw?.changeSetId);
  const inlineSummary = summaryValue(raw?.summary);
  const locatorKey = locator
    ? [locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.branchId, locator.outerNodeId, locator.outerAttemptId].join('\0')
    : '';

  useEffect(() => {
    if (!locator || !changeSetId) return;
    let cancelled = false;
    setError(false);
    void getTurnFileChangeSet(locator, changeSetId)
      .then((next) => { if (!cancelled) setChangeSet(next); })
      .catch(() => { if (!cancelled) setError(true); });
    return () => { cancelled = true; };
  // The primitive locator key prevents completed cards from refetching when a provider value is recreated.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [changeSetId, locatorKey]);

  const summary = changeSet?.summary ?? inlineSummary;
  const changes = changeSet?.changes ?? [];
  const previewChanges = changes.slice(0, previewLimit);
  const hiddenCount = Math.max(0, changes.length - previewLimit);
  if (!summary || summary.fileCount === 0 || !changeSetId) return null;

  const openChange = (change: TurnFileChangeVm) => {
    if (!workspace?.scopeKey || !locator || change.changeKind === 'deleted') return;
    const kind = change.changeKind === 'added' ? 'file-version' : 'file-diff';
    void workspace.openResource({
      kind,
      key: `${kind}:${changeSetId}:${change.id}`,
      scopeKey: workspace.scopeKey,
      title: fileName(change.logicalPath),
      description: change.logicalPath,
      attention: Boolean(change.limitationCode),
      locator,
      changeSetId,
      changeId: change.id,
    });
  };

  return (
    <Card className="ml-10 max-w-[min(46rem,calc(100%-2.5rem))] gap-0 overflow-hidden rounded-xl border-border/60 bg-muted/10 py-0 shadow-none" data-turn-file-changes-card={changeSetId}>
      <CardHeader className="grid-cols-[1fr_auto] items-center gap-3 px-3 py-2.5">
        <CardTitle className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <FileDiff className="size-4 shrink-0 text-primary" />
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
          <Collapsible open={expanded} onOpenChange={setExpanded}>
            {!expanded ? (
              <div role="list" aria-label={t('turnFiles.fileList')}>
                {previewChanges.map((change) => (
                  <TurnFileChangeRow key={change.id} change={change} onOpen={openChange} />
                ))}
              </div>
            ) : null}
            <CollapsibleContent className="data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down overflow-hidden">
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
      : <FileDiff className="size-3.5 text-amber-600 dark:text-amber-400" />;
  const content = (
    <>
      {icon}
      <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground" title={change.logicalPath}>{change.logicalPath}</span>
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
