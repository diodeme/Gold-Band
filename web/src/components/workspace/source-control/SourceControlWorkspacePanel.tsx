import { useCallback, useEffect, useMemo, type ReactNode } from 'react';
import {
  Check,
  ChevronLeft,
  ChevronRight,
  GitBranch,
  GitCommitHorizontal,
  GitCompareArrows,
  LoaderCircle,
  RefreshCw,
  TriangleAlert,
  Undo2,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';
import type {
  GitCommitFileChangeVm,
  GitFileChangeVm,
  GitMutationRequestVm,
  GitOperationRequestVm,
} from '@/types';
import {
  gitFileComparisonWorkspaceResourceKey,
  useRightWorkspace,
  type SourceControlWorkspaceResource,
} from '../right-workspace-context';
import { SourceControlRepositoryView } from './SourceControlRepositoryView';
import { SourceControlGitHubView } from './SourceControlGitHubView';
import { CommitGraph, commitGraphPageSize } from './CommitGraph';
import { toCommitGraphEntries } from './commit-graph-model';
import { SourceControlHistoryDetail } from './SourceControlHistoryDetail';
import { sourceControlStore, useSourceControlSession, type SourceControlTab } from './source-control-store';

export function SourceControlWorkspacePanel({ resource }: { resource: SourceControlWorkspaceResource }) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const session = useSourceControlSession(resource.projectId, resource.workspacePath);
  const {
    activeOperation,
    activeTab,
    body,
    commitDetail,
    commitRelations,
    errorCode,
    focusedCommitOid,
    history,
    historyDetailKind,
    historyDetailLoading,
    historyPage,
    pendingOperation,
    selectedCommitOids,
    snapshot,
    subject,
  } = session;

  const load = useCallback(async () => {
    await sourceControlStore.refresh(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  useEffect(() => {
    void sourceControlStore.ensureLoaded(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  const mutate = useCallback((input: GitMutationRequestVm, operation: string) => {
    void sourceControlStore.mutate(resource.projectId, resource.workspacePath, input, operation);
  }, [resource.projectId, resource.workspacePath]);

  const startOperation = useCallback((input: GitOperationRequestVm, operation: string) => {
    void sourceControlStore.startOperation(resource.projectId, resource.workspacePath, input, operation);
  }, [resource.projectId, resource.workspacePath]);

  const cancelOperation = useCallback(() => {
    void sourceControlStore.cancelOperation(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  const openDiff = useCallback((change: GitFileChangeVm, area: 'staged' | 'unstaged') => {
    if (!workspace.scopeKey) return;
    const source = { kind: 'workspace' as const, workspacePath: resource.workspacePath, path: change.path, area };
    void workspace.openResource({
      kind: 'file-diff',
      key: gitFileComparisonWorkspaceResourceKey(resource.projectId, source),
      scopeKey: workspace.scopeKey,
      title: change.path.split('/').at(-1) ?? change.path,
      description: change.path,
      attention: false,
      projectId: resource.projectId,
      gitSource: source,
    });
  }, [resource.projectId, resource.workspacePath, workspace]);

  const loadMoreHistory = useCallback((advancePage = false) => {
    void sourceControlStore.loadMoreHistory(resource.projectId, resource.workspacePath, advancePage);
  }, [resource.projectId, resource.workspacePath]);

  const graphEntries = useMemo(
    () => toCommitGraphEntries(history?.commits ?? [], snapshot?.repository.currentBranch),
    [history?.commits, snapshot?.repository.currentBranch],
  );
  const historyPageCount = Math.max(1, Math.ceil(graphEntries.length / commitGraphPageSize));
  const hasOlderLoadedPage = historyPage + 1 < historyPageCount;
  const canShowOlderHistory = hasOlderLoadedPage || Boolean(history?.nextCursor);
  const toggleCommitSelection = useCallback((oid: string) => {
    sourceControlStore.toggleCommitSelection(resource.projectId, resource.workspacePath, oid);
  }, [resource.projectId, resource.workspacePath]);
  const showOlderHistory = useCallback(() => {
    if (hasOlderLoadedPage) {
      sourceControlStore.setHistoryPage(resource.projectId, resource.workspacePath, historyPage + 1);
      return;
    }
    if (history?.nextCursor) loadMoreHistory(true);
  }, [hasOlderLoadedPage, history?.nextCursor, historyPage, loadMoreHistory, resource.projectId, resource.workspacePath]);
  const openCommitDetail = useCallback((oid: string) => {
    void sourceControlStore.openCommitDetail(resource.projectId, resource.workspacePath, oid);
  }, [resource.projectId, resource.workspacePath]);
  const analyzeSelectedCommits = useCallback(() => {
    void sourceControlStore.analyzeSelectedCommits(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);
  const closeHistoryDetail = useCallback(() => {
    sourceControlStore.closeHistoryDetail(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);
  const openCommitComparison = useCallback((change: GitCommitFileChangeVm, beforeOid: string | null, afterOid: string) => {
    if (!workspace.scopeKey) return;
    const source = {
      kind: 'commit' as const,
      workspacePath: resource.workspacePath,
      path: change.path,
      beforeOid,
      afterOid,
    };
    void workspace.openResource({
      kind: 'file-diff',
      key: gitFileComparisonWorkspaceResourceKey(resource.projectId, source),
      scopeKey: workspace.scopeKey,
      title: change.path.split('/').at(-1) ?? change.path,
      description: change.path,
      attention: false,
      projectId: resource.projectId,
      gitSource: source,
    });
  }, [resource.projectId, resource.workspacePath, workspace]);

  if (!snapshot && !errorCode) {
    return <PanelState icon={<LoaderCircle className="size-4 animate-spin" />} text={t('sourceControl.loading')} />;
  }
  if (!snapshot) {
    return <PanelState icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`errors.${errorCode}`, { defaultValue: t('sourceControl.loadFailed') })} action={<Button size="sm" variant="outline" onClick={() => void load()}>{t('common.refresh')}</Button>} />;
  }

  const locked = snapshot.repository.lock.locked;
  const hasConflicts = snapshot.status.conflicts.length > 0;
  const canCommit = snapshot.status.staged.length > 0 && !hasConflicts && !locked && subject.trim().length > 0;

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-source-control-workspace="true">
      <header className="shrink-0 border-b border-border/60 px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <GitBranch className="size-4 shrink-0 text-primary" />
          <span className="min-w-0 flex-1 truncate text-sm font-medium">{snapshot.repository.currentBranch ?? t('sourceControl.detached')}</span>
          {snapshot.repository.upstream ? (
            <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
              ↑{snapshot.repository.upstream.ahead} ↓{snapshot.repository.upstream.behind}
            </span>
          ) : null}
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            disabled={pendingOperation !== null}
            aria-label={t('common.refresh')}
            onClick={() => void load()}
          >
            <RefreshCw className={cn('size-3.5', pendingOperation === 'refresh' && 'animate-spin')} />
          </Button>
        </div>
        {locked ? <div className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">{t('sourceControl.locked', { operation: snapshot.repository.lock.operation ?? '' })}</div> : null}
        {activeOperation && ['queued', 'running'].includes(activeOperation.status) ? <div className="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground"><LoaderCircle className="size-3 animate-spin" /><span className="min-w-0 flex-1 truncate">{t(`sourceControl.operationKinds.${activeOperation.kind}`)}</span>{activeOperation.cancelable ? <Button size="xs" variant="ghost" onClick={cancelOperation}>{t('common.cancel')}</Button> : null}</div> : null}
        {errorCode ? <div className="mt-1 text-[11px] text-destructive">{t(`errors.${errorCode}`, { defaultValue: t('sourceControl.operationFailed') })}</div> : null}
      </header>

      <Tabs value={activeTab} onValueChange={(value) => sourceControlStore.setActiveTab(resource.projectId, resource.workspacePath, value as SourceControlTab)} className="min-h-0 flex-1 gap-0">
        <TabsList variant="line" className="h-9 w-full shrink-0 justify-start border-b border-border/50 px-2">
          <TabsTrigger value="changes" className="text-xs">{t('sourceControl.changes')}</TabsTrigger>
          <TabsTrigger value="history" className="text-xs">{t('sourceControl.history')}</TabsTrigger>
          <TabsTrigger value="repository" className="text-xs">{t('sourceControl.repository')}</TabsTrigger>
          <TabsTrigger value="github" className="text-xs">GitHub</TabsTrigger>
        </TabsList>

        <TabsContent value="changes" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          <ScrollArea className="min-h-0 flex-1">
            <div className="py-1">
              <ChangeGroup title={t('sourceControl.conflicts')} changes={snapshot.status.conflicts} tone="conflict" onOpen={(change) => openDiff(change, 'unstaged')} />
              <ChangeGroup
                title={t('sourceControl.staged')}
                changes={snapshot.status.staged}
                tone="staged"
                onOpen={(change) => openDiff(change, 'staged')}
                actionLabel={t('sourceControl.unstage')}
                actionIcon={<Undo2 className="size-3" />}
                disabled={locked || pendingOperation !== null}
                onAction={(change) => mutate({ kind: 'unstage-paths', paths: [change.path] }, `unstage:${change.path}`)}
              />
              <ChangeGroup
                title={t('sourceControl.unstaged')}
                changes={snapshot.status.unstaged}
                tone="unstaged"
                onOpen={(change) => openDiff(change, 'unstaged')}
                actionLabel={t('sourceControl.stage')}
                actionIcon={<Check className="size-3" />}
                disabled={locked || pendingOperation !== null}
                onAction={(change) => mutate({ kind: 'stage-paths', paths: [change.path] }, `stage:${change.path}`)}
              />
              <ChangeGroup
                title={t('sourceControl.untracked')}
                changes={snapshot.status.untracked}
                tone="untracked"
                onOpen={(change) => openDiff(change, 'unstaged')}
                actionLabel={t('sourceControl.stage')}
                actionIcon={<Check className="size-3" />}
                disabled={locked || pendingOperation !== null}
                onAction={(change) => mutate({ kind: 'stage-paths', paths: [change.path] }, `stage:${change.path}`)}
              />
              {snapshot.status.conflicts.length + snapshot.status.staged.length + snapshot.status.unstaged.length + snapshot.status.untracked.length === 0
                ? <PanelState text={t('sourceControl.clean')} />
                : null}
            </div>
          </ScrollArea>
          <div className="shrink-0 border-t border-border/60 p-2.5">
            <Input
              value={subject}
              onChange={(event) => sourceControlStore.setSubject(resource.projectId, resource.workspacePath, event.target.value)}
              placeholder={t('sourceControl.commitSubject')}
              disabled={locked || pendingOperation !== null}
              className="h-8 text-xs"
            />
            <Textarea
              value={body}
              onChange={(event) => sourceControlStore.setBody(resource.projectId, resource.workspacePath, event.target.value)}
              placeholder={t('sourceControl.commitBody')}
              disabled={locked || pendingOperation !== null}
              className="mt-2 min-h-16 resize-y text-xs"
            />
            <div className="mt-2 flex items-center justify-between gap-2">
              <span className="text-[11px] text-muted-foreground">{t('sourceControl.stagedCount', { count: snapshot.status.staged.length })}</span>
              <Button
                size="sm"
                disabled={!canCommit || pendingOperation !== null}
                onClick={() => mutate({ kind: 'commit', subject: subject.trim(), body: body.trim() || null }, 'commit')}
              >
                {pendingOperation === 'commit' ? <LoaderCircle className="size-3.5 animate-spin" /> : <GitCommitHorizontal className="size-3.5" />}
                {t('sourceControl.commit')}
              </Button>
            </div>
          </div>
        </TabsContent>

        <TabsContent value="history" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          {historyDetailLoading || commitDetail || commitRelations ? (
            <SourceControlHistoryDetail
              kind={historyDetailKind}
              detail={commitDetail}
              relations={commitRelations}
              loading={historyDetailLoading}
              t={t}
              onBack={closeHistoryDetail}
              onOpenFile={openCommitComparison}
            />
          ) : (
            <>
              {selectedCommitOids.size > 0 ? (
                <div className="flex h-8 shrink-0 items-center justify-between gap-2 border-b border-border/45 px-2 text-[11px] text-muted-foreground">
                  <span className="min-w-0 flex-1 truncate">{t('sourceControl.selectedCommitCount', { count: selectedCommitOids.size })}</span>
                  {selectedCommitOids.size >= 2 ? (
                    <Button size="xs" variant="secondary" onClick={analyzeSelectedCommits}>
                      <GitCompareArrows className="size-3" />{t('sourceControl.analyzeRelations')}
                    </Button>
                  ) : null}
                  <Button size="xs" variant="ghost" onClick={() => sourceControlStore.clearCommitSelection(resource.projectId, resource.workspacePath)}>{t('common.clear')}</Button>
                </div>
              ) : null}
              <ScrollArea className="min-h-0 flex-1">
                <CommitGraph
                  entries={graphEntries}
                  currentBranch={snapshot.repository.currentBranch}
                  page={historyPage}
                  selectedOids={selectedCommitOids}
                  focusedOid={focusedCommitOid}
                  runtimeLabel={t('sourceControl.runtimeCheckpoint')}
                  selectLabel={(entry) => t('sourceControl.selectCommit', { oid: entry.hash.slice(0, 8) })}
                  formatTimestamp={formatCommitTime}
                  onToggleSelected={toggleCommitSelection}
                  onOpenCommit={openCommitDetail}
                />
              </ScrollArea>
              <div className="flex h-9 shrink-0 items-center justify-between gap-2 border-t border-border/50 px-2">
                <Button size="xs" variant="ghost" disabled={historyPage === 0 || pendingOperation !== null} onClick={() => sourceControlStore.setHistoryPage(resource.projectId, resource.workspacePath, historyPage - 1)}>
                  <ChevronLeft className="size-3" />{t('sourceControl.newerCommits')}
                </Button>
                <span className="text-[10px] tabular-nums text-muted-foreground">{t('sourceControl.historyPage', { page: historyPage + 1, count: historyPageCount })}</span>
                <Button size="xs" variant="ghost" disabled={!canShowOlderHistory || pendingOperation !== null} onClick={showOlderHistory}>
                  {pendingOperation === 'history-more' ? <LoaderCircle className="size-3 animate-spin" /> : null}
                  {t('sourceControl.olderCommits')}<ChevronRight className="size-3" />
                </Button>
              </div>
            </>
          )}
        </TabsContent>

        <TabsContent value="repository" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          <SourceControlRepositoryView snapshot={snapshot} busy={pendingOperation !== null} locked={locked} onMutation={mutate} onOperation={startOperation} />
        </TabsContent>

        <TabsContent value="github" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1">
          <SourceControlGitHubView
            projectId={resource.projectId}
            workspacePath={resource.workspacePath}
            snapshot={snapshot}
            busy={pendingOperation !== null}
            onPush={(remote, branch) => startOperation({
              kind: 'push',
              remote,
              branch,
              setUpstream: branch === snapshot.repository.currentBranch && !snapshot.repository.upstream,
            }, 'push')}
          />
        </TabsContent>
      </Tabs>
    </section>
  );
}

function ChangeGroup({
  title,
  changes,
  tone,
  onOpen,
  actionLabel,
  actionIcon,
  disabled,
  onAction,
}: {
  title: string;
  changes: GitFileChangeVm[];
  tone: 'conflict' | 'staged' | 'unstaged' | 'untracked';
  onOpen: (change: GitFileChangeVm) => void;
  actionLabel?: string;
  actionIcon?: ReactNode;
  disabled?: boolean;
  onAction?: (change: GitFileChangeVm) => void;
}) {
  if (changes.length === 0) return null;
  return (
    <section data-source-control-group={tone}>
      <div className="flex h-7 items-center gap-2 px-3 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        <span>{title}</span><span className="tabular-nums">{changes.length}</span>
      </div>
      {changes.map((change) => (
        <div key={`${change.path}:${change.indexStatus ?? ''}:${change.worktreeStatus ?? ''}`} className="group flex min-w-0 items-center px-1.5 hover:bg-muted/45">
          <button type="button" className="flex h-8 min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50" onClick={() => onOpen(change)}>
            <ChangeStatus kind={change.kind} tone={tone} />
            <span className="min-w-0 flex-1 truncate text-xs">{change.path}</span>
            {change.oldPath ? <span className="max-w-20 truncate text-[10px] text-muted-foreground">← {change.oldPath}</span> : null}
            {change.addedLines != null ? <span className="text-[10px] tabular-nums text-emerald-600">+{change.addedLines}</span> : null}
            {change.deletedLines != null ? <span className="text-[10px] tabular-nums text-destructive">-{change.deletedLines}</span> : null}
            <ChevronRight className="size-3 text-muted-foreground/60" />
          </button>
          {onAction ? <Button type="button" size="icon-xs" variant="ghost" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" disabled={disabled} aria-label={`${actionLabel}: ${change.path}`} onClick={() => onAction(change)}>{actionIcon}</Button> : null}
        </div>
      ))}
    </section>
  );
}

function ChangeStatus({ kind, tone }: { kind: GitFileChangeVm['kind']; tone: string }) {
  const label = kind === 'untracked' ? '?' : kind === 'added' ? 'A' : kind === 'deleted' ? 'D' : kind === 'renamed' ? 'R' : kind === 'unmerged' ? '!' : 'M';
  return <span className={cn('flex size-4 shrink-0 items-center justify-center rounded text-[10px] font-semibold', tone === 'conflict' ? 'bg-destructive/15 text-destructive' : tone === 'staged' ? 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400' : 'bg-muted text-muted-foreground')}>{label}</span>;
}

function PanelState({ icon, text, action }: { icon?: ReactNode; text: string; action?: ReactNode }) {
  return <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-sm text-muted-foreground"><span className="flex items-center gap-2">{icon}{text}</span>{action}</div>;
}

function formatCommitTime(timestamp: string) {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}
