import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronLeft, ChevronRight, Clipboard, GitCommitHorizontal, GitMerge, LoaderCircle, Route, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';
import type { GitCommitReviewFileVm, GitCommitVm, GitSourceControlSnapshotVm } from '@/types';
import { useWorkspaceResponsiveState } from '../use-workspace-responsive-state';
import {
  gitDiffReviewWorkspaceResourceKey,
  useRightWorkspace,
  type SourceControlWorkspaceResource,
} from '../right-workspace-context';
import { diffReviewStore, gitDiffReviewItemId, type GitDiffReviewItem } from './diff-review-store';
import { SourceControlDiffFileRow } from './SourceControlDiffFileRow';
import { sourceControlStore, type SourceControlSessionSnapshot } from './source-control-store';

const HISTORY_PAGE_SIZE = 300;
export const HISTORY_LIST_MIN_WIDTH = 220;
export const HISTORY_REVIEW_MIN_WIDTH = 280;
export const HISTORY_SPLIT_MIN_WIDTH = 520;

export function SourceControlHistoryView({
  resource,
  session,
  snapshot,
  busy,
}: {
  resource: SourceControlWorkspaceResource;
  session: SourceControlSessionSnapshot;
  snapshot: GitSourceControlSnapshotVm;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const { ref, responsiveState } = useWorkspaceResponsiveState(HISTORY_SPLIT_MIN_WIDTH);
  const commitScrollRef = useRef<HTMLDivElement>(null);
  const [compactView, setCompactView] = useState<'list' | 'detail'>('list');
  const [reachabilityOpen, setReachabilityOpen] = useState(false);
  const commits = session.history?.commits ?? [];
  const pageCount = Math.max(1, Math.ceil(commits.length / HISTORY_PAGE_SIZE));
  const pageCommits = useMemo(
    () => commits.slice(session.historyPage * HISTORY_PAGE_SIZE, (session.historyPage + 1) * HISTORY_PAGE_SIZE),
    [commits, session.historyPage],
  );
  const visibleOids = useMemo(() => pageCommits.map((commit) => commit.oid), [pageCommits]);
  const hasOlderLoadedPage = session.historyPage + 1 < pageCount;
  const canShowOlderHistory = hasOlderLoadedPage || Boolean(session.history?.nextCursor);

  useEffect(() => {
    const viewport = commitScrollRef.current;
    if (viewport) viewport.scrollTop = sourceControlStore.historyScrollPositions(resource.projectId, resource.workspacePath).commitList;
  }, [resource.projectId, resource.workspacePath, session.historyPage]);

  useEffect(() => {
    if (session.selectedCommitOids.size > 0 && !responsiveState.split) setCompactView('detail');
  }, [responsiveState.split, session.selectedCommitOids]);

  const selectCommit = useCallback((commit: GitCommitVm, event: React.MouseEvent) => {
    sourceControlStore.selectCommit(resource.projectId, resource.workspacePath, commit.oid, visibleOids, {
      additive: event.ctrlKey || event.metaKey,
      range: event.shiftKey,
    });
  }, [resource.projectId, resource.workspacePath, visibleOids]);

  const openReachability = useCallback((oid: string) => {
    setReachabilityOpen(true);
    void sourceControlStore.loadCommitReachability(resource.projectId, resource.workspacePath, oid);
  }, [resource.projectId, resource.workspacePath]);

  const selectCommitForContextMenu = useCallback((oid: string) => {
    sourceControlStore.selectCommitForContextMenu(resource.projectId, resource.workspacePath, oid);
  }, [resource.projectId, resource.workspacePath]);

  const showOlder = () => {
    if (hasOlderLoadedPage) {
      sourceControlStore.setHistoryPage(resource.projectId, resource.workspacePath, session.historyPage + 1);
    } else if (session.history?.nextCursor) {
      void sourceControlStore.loadMoreHistory(resource.projectId, resource.workspacePath, true);
    }
  };

  const list = (
    <section className="flex h-full min-h-0 flex-col" aria-label={t('sourceControl.commitList')}>
      {session.selectedCommitOids.size > 1 ? (
        <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border/45 px-2 text-[11px] text-muted-foreground">
          <span className="min-w-0 flex-1 truncate">{t('sourceControl.selectedCommitCount', { count: session.selectedCommitOids.size })}</span>
          <Button type="button" size="xs" variant="ghost" onClick={() => sourceControlStore.clearCommitSelection(resource.projectId, resource.workspacePath)}>
            <X className="size-3" />{t('common.clear')}
          </Button>
        </div>
      ) : null}
      <ScrollArea
        className="min-h-0 flex-1"
        viewportRef={commitScrollRef}
        onViewportScroll={(event) => sourceControlStore.setHistoryScrollPosition(resource.projectId, resource.workspacePath, 'commit-list', event.currentTarget.scrollTop)}
      >
        <div className="py-1" role="listbox" aria-multiselectable="true">
          {pageCommits.map((commit) => (
            <CommitRow
              key={commit.oid}
              commit={commit}
              selected={session.selectedCommitOids.has(commit.oid)}
              focused={session.focusedCommitOid === commit.oid}
              runtimeLabel={t('sourceControl.runtimeCheckpoint')}
              onSelect={selectCommit}
              onContextMenu={selectCommitForContextMenu}
              onReachability={openReachability}
            />
          ))}
        </div>
      </ScrollArea>
      <div className="flex h-9 shrink-0 items-center justify-between gap-2 border-t border-border/50 px-2">
        <Button type="button" size="xs" variant="ghost" disabled={session.historyPage === 0 || busy} onClick={() => sourceControlStore.setHistoryPage(resource.projectId, resource.workspacePath, session.historyPage - 1)}>
          <ChevronLeft className="size-3" />{t('sourceControl.newerCommits')}
        </Button>
        <span className="text-[10px] tabular-nums text-muted-foreground">{t('sourceControl.historyCurrentPage', { page: session.historyPage + 1 })}</span>
        <Button type="button" size="xs" variant="ghost" disabled={!canShowOlderHistory || busy} onClick={showOlder}>
          {session.pendingAction?.kind === 'history-more' ? <LoaderCircle className="size-3 animate-spin" /> : null}
          {t('sourceControl.olderCommits')}<ChevronRight className="size-3" />
        </Button>
      </div>
    </section>
  );

  const detail = (
    <CommitReviewPanel
      resource={resource}
      session={session}
      onOpenFile={(file, fileIndex) => {
        if (!workspace.scopeKey || !session.commitReview) return;
        const items = reviewItems(resource.workspacePath, session.commitReview.files);
        const item = items[fileIndex];
        if (!item) return;
        const reviewSessionId = `${resource.projectId}:${session.commitReview.revision}:${session.commitReview.selectedOids.join(',')}`;
        diffReviewStore.save({ id: reviewSessionId, projectId: resource.projectId, revision: session.commitReview.revision, items });
        void workspace.openResource({
          kind: 'file-diff',
          key: gitDiffReviewWorkspaceResourceKey(resource.projectId, reviewSessionId),
          scopeKey: workspace.scopeKey,
          title: item.path.split('/').at(-1) ?? item.path,
          description: item.path,
          attention: false,
          projectId: resource.projectId,
          gitSource: item.source,
          reviewSessionId,
          reviewItemId: item.id,
          reviewLanding: 'top',
        });
      }}
    />
  );

  return (
    <div ref={ref} className="flex min-h-0 flex-1 flex-col" data-source-control-history-layout={responsiveState.split ? 'split' : 'compact'}>
      {session.selectedCommitOids.size > 0 && !responsiveState.split ? (
        <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border/50 px-2">
          <Button type="button" size="sm" variant={compactView === 'list' ? 'secondary' : 'ghost'} className="h-7 text-xs" onClick={() => setCompactView('list')}>{t('sourceControl.commits')}</Button>
          <Button type="button" size="sm" variant={compactView === 'detail' ? 'secondary' : 'ghost'} className="h-7 text-xs" disabled={session.selectedCommitOids.size === 0} onClick={() => setCompactView('detail')}>{t('sourceControl.changes')}</Button>
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        {responsiveState.split && session.selectedCommitOids.size > 0 ? (
          <ResizablePanelGroup orientation="horizontal" className="h-full">
            <ResizablePanel id="commit-list" defaultSize="43%" minSize={HISTORY_LIST_MIN_WIDTH} className="min-w-0">{list}</ResizablePanel>
            <ResizableHandle className="bg-border/50" />
            <ResizablePanel id="commit-review" defaultSize="57%" minSize={HISTORY_REVIEW_MIN_WIDTH} className="min-w-0">{detail}</ResizablePanel>
          </ResizablePanelGroup>
        ) : compactView === 'detail' && session.selectedCommitOids.size > 0 ? detail : list}
      </div>
      <Dialog open={reachabilityOpen} onOpenChange={(open) => {
        setReachabilityOpen(open);
        if (!open) sourceControlStore.closeCommitReachability(resource.projectId, resource.workspacePath);
      }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('sourceControl.commitReachability')}</DialogTitle>
            <DialogDescription>{t('sourceControl.commitReachabilityDescription')}</DialogDescription>
          </DialogHeader>
          {session.reachabilityLoading ? <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{t('sourceControl.loading')}</div>
            : session.commitReachability ? <ReachabilityContent reachability={session.commitReachability} /> : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}

const CommitRow = memo(function CommitRow({ commit, selected, focused, runtimeLabel, onSelect, onContextMenu, onReachability }: {
  commit: GitCommitVm;
  selected: boolean;
  focused: boolean;
  runtimeLabel: string;
  onSelect: (commit: GitCommitVm, event: React.MouseEvent) => void;
  onContextMenu: (oid: string) => void;
  onReachability: (oid: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <ContextMenu>
      <ContextMenuTrigger className="block" onContextMenu={() => onContextMenu(commit.oid)}>
        <button
          type="button"
          role="option"
          aria-selected={selected}
          onClick={(event) => {
            onSelect(commit, event);
            if (event.detail > 0) event.currentTarget.blur();
          }}
          className={cn(
            "flex h-12 w-full select-none items-center gap-2 px-2 text-left outline-none [content-visibility:auto] [contain-intrinsic-size:auto_48px] hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50",
            selected && "bg-primary/10 hover:bg-primary/15",
            focused && "border-l-2 border-primary pl-1.5",
          )}
        >
          <span className={cn("flex size-5 shrink-0 items-center justify-center rounded-full border", selected ? "border-primary bg-primary text-primary-foreground" : "border-border text-muted-foreground")}>
            {selected ? <Check className="size-3" /> : commit.parentOids.length > 1 ? <GitMerge className="size-3" /> : <GitCommitHorizontal className="size-3" />}
          </span>
          <span className="min-w-0 flex-1">
            <span className="flex min-w-0 items-center gap-1.5">
              <span className="truncate text-xs font-medium">{commit.subject}</span>
              {commit.runtimeCheckpoint ? <span className="shrink-0 rounded bg-primary/10 px-1 text-[9px] text-primary">{runtimeLabel}</span> : null}
            </span>
            <span className="mt-0.5 flex items-center gap-2 text-[10px] text-muted-foreground">
              <span className="font-mono">{commit.oid.slice(0, 8)}</span>
              <span className="truncate">{commit.author.name}</span>
              <span className="ml-auto shrink-0">{formatCommitTime(commit.author.timestamp)}</span>
            </span>
          </span>
        </button>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-52">
        <ContextMenuItem onSelect={() => void navigator.clipboard.writeText(commit.oid.slice(0, 8))}><Clipboard />{t('sourceControl.copyShortSha')}</ContextMenuItem>
        <ContextMenuItem onSelect={() => void navigator.clipboard.writeText(commit.oid)}><Clipboard />{t('sourceControl.copyFullSha')}</ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => onReachability(commit.oid)}><Route />{t('sourceControl.commitReachability')}</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
});

function CommitReviewPanel({ resource, session, onOpenFile }: {
  resource: SourceControlWorkspaceResource;
  session: SourceControlSessionSnapshot;
  onOpenFile: (file: GitCommitReviewFileVm, fileIndex: number) => void;
}) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const reviewScrollRef = useRef<HTMLDivElement>(null);
  const reviewKey = session.commitReview
    ? `${session.commitReview.revision}:${session.commitReview.selectedOids.join(',')}`
    : null;
  useEffect(() => {
    const viewport = reviewScrollRef.current;
    if (!viewport) return;
    const scrollTop = sourceControlStore.historyScrollPositions(resource.projectId, resource.workspacePath, reviewKey).reviewList;
    const frame = restoreReviewScrollPosition(viewport, scrollTop, (apply) => window.requestAnimationFrame(apply));
    return () => window.cancelAnimationFrame(frame);
  }, [resource.projectId, resource.workspacePath, reviewKey, workspace.activeTabKey]);
  if (session.historyDetailLoading) return <CenteredState icon={<LoaderCircle className="size-4 animate-spin" />} text={t('sourceControl.commitReviewLoading')} />;
  const review = session.commitReview;
  if (!review) return <CenteredState text={t('sourceControl.selectCommitHint')} />;
  const pathCounts = commitReviewPathCounts(review.files);
  return (
    <section className="flex h-full min-h-0 flex-col" aria-label={t('sourceControl.commitReview')}>
      <header className="shrink-0 border-b border-border/50 px-3 py-2">
        <div className="flex items-center gap-2 text-xs font-medium">
          <GitCommitHorizontal className="size-3.5" />
          <span>{t('sourceControl.commitReviewCount', { count: review.totals.commitCount })}</span>
        </div>
        <div className="mt-1 flex gap-3 text-[10px] tabular-nums text-muted-foreground">
          <span>{t('sourceControl.changedFileCount', { count: review.totals.fileCount })}</span>
        </div>
      </header>
      <ScrollArea
        className="min-h-0 flex-1"
        viewportRef={reviewScrollRef}
        onViewportScroll={(event) => sourceControlStore.setHistoryScrollPosition(resource.projectId, resource.workspacePath, 'review-list', event.currentTarget.scrollTop, reviewKey)}
      >
        <div className="py-1">
          {review.files.map((file, fileIndex) => (
            <SourceControlDiffFileRow
              key={`${file.beforeOid ?? ''}:${file.beforePath ?? ''}:${file.afterOid}:${file.path}`}
              path={file.path}
              oldPath={file.oldPath}
              kind={file.kind}
              addedLines={file.addedLines}
              deletedLines={file.deletedLines}
              onClick={() => onOpenFile(file, fileIndex)}
              className="px-2"
              pathDetail={(pathCounts.get(file.path) ?? 0) > 1
                ? <span className="shrink-0 font-mono text-[10px] text-muted-foreground">{file.afterOid.slice(0, 8)}</span>
                : null}
            />
          ))}
        </div>
      </ScrollArea>
    </section>
  );
}

export function restoreReviewScrollPosition(
  viewport: Pick<HTMLDivElement, 'scrollTop'>,
  scrollTop: number,
  scheduleFrame: (apply: () => void) => number,
) {
  viewport.scrollTop = scrollTop;
  return scheduleFrame(() => { viewport.scrollTop = scrollTop; });
}

export function commitReviewPathCounts(files: GitCommitReviewFileVm[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const file of files) counts.set(file.path, (counts.get(file.path) ?? 0) + 1);
  return counts;
}

function ReachabilityContent({ reachability }: { reachability: NonNullable<SourceControlSessionSnapshot['commitReachability']> }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4 text-sm">
      <div><div className="text-xs text-muted-foreground">{t('sourceControl.targetPath')}</div><div className="mt-1">{t(`sourceControl.targetPaths.${reachability.targetPath}`)} · <span className="font-mono text-xs">{reachability.targetRef}</span></div></div>
      {reachability.firstMergeOid ? <div><div className="text-xs text-muted-foreground">{t('sourceControl.firstMerge')}</div><div className="mt-1 font-mono text-xs">{reachability.firstMergeOid}</div></div> : null}
      <div><div className="text-xs text-muted-foreground">{t('sourceControl.containingRefs')}</div><div className="mt-1 flex flex-wrap gap-1">{reachability.containingRefs.length > 0 ? reachability.containingRefs.map((ref) => <span key={ref.fullName} className="rounded bg-muted px-1.5 py-0.5 text-xs">{ref.shortName}</span>) : <span>{t('sourceControl.noContainingRefs')}</span>}</div></div>
      <div><div className="text-xs text-muted-foreground">{t('sourceControl.parents')}</div><div className="mt-1 space-y-1 font-mono text-xs">{reachability.parentOids.length > 0 ? reachability.parentOids.map((oid) => <div key={oid}>{oid}</div>) : <div>{t('sourceControl.rootCommit')}</div>}</div></div>
    </div>
  );
}

function reviewItems(workspacePath: string | null | undefined, files: GitCommitReviewFileVm[]): GitDiffReviewItem[] {
  return files.map((file) => ({
    id: gitDiffReviewItemId(file.afterOid, file.beforeOid, file.beforePath, file.path),
    path: file.path,
    source: {
      kind: 'commit' as const,
      workspacePath,
      path: file.path,
      beforeOid: file.beforeOid ?? null,
      beforePath: file.beforePath ?? null,
      afterOid: file.afterOid,
    },
    stats: { addedLines: file.addedLines ?? null, deletedLines: file.deletedLines ?? null },
  }));
}

function CenteredState({ icon, text }: { icon?: React.ReactNode; text: string }) {
  return <div className="flex h-full min-h-0 items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">{icon}{text}</div>;
}

function formatCommitTime(timestamp: string) {
  const value = new Date(timestamp);
  return Number.isNaN(value.getTime()) ? timestamp : value.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}
