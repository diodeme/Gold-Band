import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Check,
  CheckCircle2,
  CircleX,
  Download,
  GitBranch,
  GitCommitHorizontal,
  LoaderCircle,
  MoreHorizontal,
  TriangleAlert,
  Undo2,
  X,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openExternalUrl, resolveWorkspaceFileLink } from '@/api';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type {
  GitFileChangeVm,
  GitMutationRequestVm,
  GitOperationErrorVm,
  GitOperationRequestVm,
  GitSourceControlSnapshotVm,
} from '@/types';
import {
  gitDiffReviewWorkspaceResourceKey,
  fileWorkspaceResourceKey,
  useRightWorkspace,
  type SourceControlWorkspaceResource,
} from '../right-workspace-context';
import { SourceControlRepositoryView } from './SourceControlRepositoryView';
import { SourceControlDiffFileRow } from './SourceControlDiffFileRow';
import { SourceControlChangesToolbar, SourceControlSyncActions } from './SourceControlChangesToolbar';
import { SourceControlGitHubView } from './SourceControlGitHubView';
import { SourceControlHistoryView } from './SourceControlHistoryView';
import { githubDataStore, githubRepositorySessionKey } from './github-data-store';
import { diffReviewStore, gitComparisonReviewItemId, type GitDiffReviewItem } from './diff-review-store';
import { sourceControlStore, useSourceControlSession, type SourceControlSessionSnapshot, type SourceControlTab } from './source-control-store';

const GIT_DOWNLOAD_URL = 'https://git-scm.com/downloads';

export function SourceControlWorkspacePanel({ resource }: { resource: SourceControlWorkspaceResource }) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const session = useSourceControlSession(resource.projectId, resource.workspacePath);
  const {
    activeOperation,
    activeTab,
    body,
    capability,
    error,
    pendingAction,
    repositoryTab,
    snapshot,
    subject,
  } = session;

  const load = useCallback(async () => {
    await sourceControlStore.refresh(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  useEffect(() => {
    void sourceControlStore.ensureLoaded(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  useEffect(() => {
    const repository = snapshot?.repository;
    if (!repository) return;
    const sessionKey = githubRepositorySessionKey(resource.projectId, repository.commonDir, repository.workspacePath);
    void githubDataStore.getCapability(
      sessionKey,
      resource.projectId,
      repository.workspacePath,
    ).catch(() => undefined);
  }, [resource.projectId, snapshot?.repository]);

  const changeTab = useCallback((value: SourceControlTab) => {
    sourceControlStore.setActiveTab(resource.projectId, resource.workspacePath, value);
  }, [resource.projectId, resource.workspacePath]);

  const changeRepositoryTab = useCallback((value: import('./source-control-store').SourceControlRepositoryTab) => {
    sourceControlStore.setRepositoryTab(resource.projectId, resource.workspacePath, value);
  }, [resource.projectId, resource.workspacePath]);

  const mutate = useCallback((input: GitMutationRequestVm) => {
    void sourceControlStore.mutate(resource.projectId, resource.workspacePath, input);
  }, [resource.projectId, resource.workspacePath]);

  const startOperation = useCallback((input: GitOperationRequestVm) => {
    void sourceControlStore.startOperation(resource.projectId, resource.workspacePath, input);
  }, [resource.projectId, resource.workspacePath]);

  const cancelOperation = useCallback(() => {
    void sourceControlStore.cancelOperation(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  const dismissOperation = useCallback(() => {
    sourceControlStore.dismissOperationResult(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  const initializeRepository = useCallback(() => {
    void sourceControlStore.initializeRepository(resource.projectId, resource.workspacePath);
  }, [resource.projectId, resource.workspacePath]);

  const openDiff = useCallback((change: GitFileChangeVm, area: 'staged' | 'unstaged') => {
    if (!workspace.scopeKey || !snapshot) return;
    const changes = area === 'staged'
      ? snapshot.status.staged
      : [...snapshot.status.unstaged, ...snapshot.status.untracked];
    const items = workspaceReviewItems(resource.workspacePath, area, changes);
    const item = items.find((candidate) => candidate.path === change.path);
    if (!item) return;
    const reviewSessionId = `${resource.projectId}:workspace:${resource.workspacePath ?? 'main'}:${area}:${snapshot.repository.revision}`;
    diffReviewStore.save({
      id: reviewSessionId,
      projectId: resource.projectId,
      revision: snapshot.repository.revision,
      items,
    });
    void workspace.openResource({
      kind: 'file-diff',
      key: gitDiffReviewWorkspaceResourceKey(resource.projectId, reviewSessionId),
      scopeKey: workspace.scopeKey,
      title: change.path.split('/').at(-1) ?? change.path,
      description: change.path,
      attention: false,
      projectId: resource.projectId,
      gitSource: item.source,
      reviewSessionId,
      reviewItemId: item.id,
      reviewLanding: 'top',
    });
  }, [resource.projectId, resource.workspacePath, snapshot, workspace]);

  const openConflictFile = useCallback(async (change: GitFileChangeVm) => {
    if (!workspace.scopeKey) return;
    const resolved = await resolveWorkspaceFileLink(resource.projectId, change.path, snapshot?.repository.workspacePath ?? null);
    workspace.openResource({
      kind: 'file',
      key: fileWorkspaceResourceKey(resource.projectId, resolved.locator.canonicalPath),
      scopeKey: workspace.scopeKey,
      projectId: resource.projectId,
      title: change.path.split('/').at(-1) ?? change.path,
      description: change.path,
      attention: false,
      locator: resolved.locator,
      target: null,
      targetRevision: 0,
    });
  }, [resource.projectId, snapshot?.repository.workspacePath, workspace]);

  if (!snapshot && !error) {
    if (session.status === 'unavailable' && capability) {
      return (
        <SourceControlUnavailableState
          capability={capability}
          initializing={pendingAction?.kind === 'repository-initialize'}
          onInitialize={initializeRepository}
          onRetry={load}
        />
      );
    }
    return <PanelState icon={<LoaderCircle className="size-4 animate-spin" />} text={t('sourceControl.loading')} />;
  }
  if (!snapshot) {
    return <PanelState icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`errors.${error?.code}`, { ...error?.params, defaultValue: t('sourceControl.loadFailed') })} action={<Button size="sm" variant="outline" onClick={() => void load()}>{t('common.refresh')}</Button>} />;
  }

  const locked = snapshot.repository.lock.locked;
  const conflictWorkflowActive = snapshot.status.operationInProgress?.kind === 'merge' || snapshot.status.operationInProgress?.kind === 'rebase';
  const writeLocked = locked || conflictWorkflowActive;
  const hasConflicts = snapshot.status.conflicts.length > 0;
  const activeOperationPending = Boolean(activeOperation && ['queued', 'running'].includes(activeOperation.status));
  const busyActionKind = pendingAction?.kind ?? (activeOperationPending ? activeOperation?.kind ?? null : null);
  const busy = busyActionKind !== null;
  const workspaceClean = snapshot.status.conflicts.length
    + snapshot.status.staged.length
    + snapshot.status.unstaged.length
    + snapshot.status.untracked.length === 0;
  const canCommit = snapshot.status.staged.length > 0 && !hasConflicts && !writeLocked && subject.trim().length > 0;

  return (
    <section
      className="flex min-h-0 flex-1 flex-col"
      data-source-control-workspace="true"
      data-source-control-workspace-path={resource.workspacePath ?? 'main'}
      data-theme-role="diff"
    >
      <header className="shrink-0 border-b border-border/60 px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <GitBranch className="size-4 shrink-0 text-foreground" />
          <span className="min-w-0 flex-1 truncate text-sm font-medium">{snapshot.repository.currentBranch ?? t('sourceControl.detached')}</span>
          <SourceControlSyncActions snapshot={snapshot} busyActionKind={busyActionKind} locked={writeLocked} onOperation={startOperation} />
        </div>
        {locked ? <div className="mt-1 text-ui-caption text-amber-600 dark:text-amber-400">{t('sourceControl.locked', { operation: snapshot.repository.lock.operation ?? '' })}</div> : null}
        {activeOperation ? <SourceControlOperationStatus operation={activeOperation} onCancel={cancelOperation} onDismiss={dismissOperation} /> : null}
        {error ? <SourceControlError error={error} /> : null}
        {snapshot.status.operationInProgress?.kind === 'merge' || snapshot.status.operationInProgress?.kind === 'rebase'
          ? <SourceControlConflictWorkflow operation={snapshot.status.operationInProgress} busy={busy} onOperation={startOperation} />
          : null}
      </header>

      <Tabs value={activeTab} onValueChange={(value) => changeTab(value as SourceControlTab)} className="min-h-0 flex-1 gap-0">
        <TabsList variant="line" className="h-9 w-full shrink-0 justify-start border-b border-border/50 px-2">
          <TabsTrigger value="changes" className="text-xs">{t('sourceControl.changes')}</TabsTrigger>
          <TabsTrigger value="history" className="text-xs">{t('sourceControl.history')}</TabsTrigger>
          <TabsTrigger value="repository" className="text-xs">{t('sourceControl.repository')}</TabsTrigger>
          <TabsTrigger value="github" className="text-xs">GitHub</TabsTrigger>
        </TabsList>

        <TabsContent value="changes" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          <SourceControlChangesToolbar snapshot={snapshot} busyActionKind={busyActionKind} locked={writeLocked} onMutation={mutate} onOperation={startOperation} />
          {workspaceClean ? (
            <div
              className="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground"
              data-source-control-changes-empty="true"
            >
              {t('sourceControl.clean')}
            </div>
          ) : (
            <ScrollArea className="min-h-0 flex-1">
              <div className="py-1">
                <ChangeGroup title={t('sourceControl.conflicts')} changes={snapshot.status.conflicts} tone="conflict" onOpen={(change) => void openConflictFile(change)} />
                <ChangeGroup
                  title={t('sourceControl.staged')}
                  changes={snapshot.status.staged}
                  tone="staged"
                  onOpen={(change) => openDiff(change, 'staged')}
                  actionLabel={t('sourceControl.unstage')}
                  actionIcon={<Undo2 className="size-3" />}
                  pendingPath={pendingAction?.kind === 'unstage-paths' ? pendingAction.path : null}
                  disabled={writeLocked || busy}
                  onAction={(change) => mutate({ kind: 'unstage-paths', paths: [change.path] })}
                />
                <ChangeGroup
                  title={t('sourceControl.unstaged')}
                  changes={snapshot.status.unstaged}
                  tone="unstaged"
                  onOpen={(change) => openDiff(change, 'unstaged')}
                  actionLabel={t('sourceControl.stage')}
                  actionIcon={<Check className="size-3" />}
                  pendingPath={pendingAction?.kind === 'stage-paths' ? pendingAction.path : null}
                  disabled={writeLocked || busy}
                  onAction={(change) => mutate({ kind: 'stage-paths', paths: [change.path] })}
                />
                <ChangeGroup
                  title={t('sourceControl.untracked')}
                  changes={snapshot.status.untracked}
                  tone="untracked"
                  onOpen={(change) => openDiff(change, 'unstaged')}
                  actionLabel={t('sourceControl.stage')}
                  actionIcon={<Check className="size-3" />}
                  pendingPath={pendingAction?.kind === 'stage-paths' ? pendingAction.path : null}
                  disabled={writeLocked || busy}
                  onAction={(change) => mutate({ kind: 'stage-paths', paths: [change.path] })}
                />
              </div>
            </ScrollArea>
          )}
          <div className="shrink-0 border-t border-border/60 p-2.5">
            <Input
              value={subject}
              onChange={(event) => sourceControlStore.setSubject(resource.projectId, resource.workspacePath, event.target.value)}
              placeholder={t('sourceControl.commitSubject')}
              disabled={writeLocked || busy}
              className="h-8 text-xs"
            />
            <Textarea
              value={body}
              onChange={(event) => sourceControlStore.setBody(resource.projectId, resource.workspacePath, event.target.value)}
              placeholder={t('sourceControl.commitBody')}
              disabled={writeLocked || busy}
              className="mt-2 min-h-16 resize-y text-xs"
            />
            <div className="mt-2 flex items-center justify-between gap-2">
              <span className="text-ui-caption text-muted-foreground">{t('sourceControl.stagedCount', { count: snapshot.status.staged.length })}</span>
              <Button
                size="sm"
                disabled={!canCommit || busy}
                onClick={() => mutate({ kind: 'commit', subject: subject.trim(), body: body.trim() || null })}
              >
                {pendingAction?.kind === 'commit' ? <LoaderCircle className="size-3.5 animate-spin" /> : <GitCommitHorizontal className="size-3.5" />}
                {t('sourceControl.commit')}
              </Button>
            </div>
          </div>
        </TabsContent>

        <TabsContent value="history" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          <SourceControlHistoryView resource={resource} session={session} snapshot={snapshot} busy={busy} />
        </TabsContent>

        <TabsContent value="repository" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1 data-[state=active]:flex-col">
          <SourceControlRepositoryView snapshot={snapshot} busyActionKind={busyActionKind} busyActionPath={pendingAction?.path ?? null} locked={writeLocked} onMutation={mutate} onOperation={startOperation} activeTab={repositoryTab} onTabChange={changeRepositoryTab} />
        </TabsContent>

        <TabsContent value="github" className="min-h-0 data-[state=active]:flex data-[state=active]:flex-1">
          <SourceControlGitHubView
            projectId={resource.projectId}
            workspacePath={resource.workspacePath}
            snapshot={snapshot}
            busy={busy}
            onPush={(remote, branch) => startOperation({
              kind: 'push',
              remote,
              branch,
              setUpstream: branch === snapshot.repository.currentBranch && !snapshot.repository.upstream,
            })}
          />
        </TabsContent>
      </Tabs>
    </section>
  );
}

function SourceControlUnavailableState({ capability, initializing, onInitialize, onRetry }: {
  capability: NonNullable<SourceControlSessionSnapshot['capability']>;
  initializing: boolean;
  onInitialize: () => void;
  onRetry: () => void | Promise<void>;
}) {
  const { t } = useTranslation();
  if (capability.status === 'not-installed') {
    return <PanelState icon={<Download className="size-4" />} text={t('sourceControl.gitNotInstalled')} description={t('sourceControl.gitNotInstalledDescription')} action={<div className="flex flex-wrap justify-center gap-2"><Button size="sm" onClick={() => void openExternalUrl(GIT_DOWNLOAD_URL)}>{t('sourceControl.openGitDownload')}</Button><Button size="sm" variant="outline" onClick={() => void onRetry()}>{t('sourceControl.checkAgain')}</Button></div>} />;
  }
  if (capability.status === 'repository-required') {
    return <PanelState icon={<GitBranch className="size-4" />} text={t('sourceControl.repositoryRequired')} description={t('sourceControl.repositoryRequiredDescription')} action={<Button size="sm" disabled={initializing} onClick={onInitialize}>{initializing ? <LoaderCircle className="size-3.5 animate-spin" /> : null}{initializing ? t('sourceControl.initializingRepository') : t('sourceControl.initializeRepository')}</Button>} />;
  }
  return <PanelState icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`sourceControl.capability.${capability.status}.title`)} description={t(`sourceControl.capability.${capability.status}.description`)} action={<Button size="sm" variant="outline" onClick={() => void onRetry()}>{t('sourceControl.checkAgain')}</Button>} />;
}

function SourceControlOperationStatus({ operation, onCancel, onDismiss }: {
  operation: NonNullable<SourceControlSessionSnapshot['activeOperation']>;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const pending = operation.status === 'queued' || operation.status === 'running';
  const failed = operation.status === 'failed' || operation.status === 'conflicted';
  const operationName = t(`sourceControl.operationNames.${operation.kind}`);
  const text = pending
    ? t(`sourceControl.operationKinds.${operation.kind}`)
    : t(`sourceControl.operationResults.${operation.status}`, { operation: operationName });
  return (
    <div
      className={cn('mt-1 flex min-w-0 items-center gap-2 text-ui-caption', failed ? 'text-destructive' : operation.status === 'succeeded' ? 'text-emerald-600 dark:text-emerald-400' : 'text-muted-foreground')}
      role={failed ? 'alert' : 'status'}
      aria-live="polite"
      data-source-control-operation-status={operation.status}
    >
      {pending ? <LoaderCircle className="size-3 shrink-0 animate-spin" /> : operation.status === 'succeeded' ? <CheckCircle2 className="size-3 shrink-0" /> : <CircleX className="size-3 shrink-0" />}
      <span className="min-w-0 flex-1 truncate">{text}</span>
      {pending && operation.cancelable ? <Button size="xs" variant="ghost" onClick={onCancel}>{t('common.cancel')}</Button> : null}
      {!pending ? <Button type="button" size="icon-xs" variant="ghost" aria-label={t('sourceControl.dismissOperationResult')} onClick={onDismiss}><X className="size-3" /></Button> : null}
    </div>
  );
}

function SourceControlConflictWorkflow({ operation, busy, onOperation }: {
  operation: NonNullable<GitSourceControlSnapshotVm['status']['operationInProgress']>;
  busy: boolean;
  onOperation: (input: GitOperationRequestVm) => void;
}) {
  const { t } = useTranslation();
  const [confirm, setConfirm] = useState<'continue' | 'abort' | 'skip' | null>(null);
  const rebase = operation.kind === 'rebase';
  const submit = () => {
    if (confirm === 'continue') onOperation({ kind: rebase ? 'rebase-continue' : 'merge-continue' });
    if (confirm === 'abort') onOperation({ kind: rebase ? 'rebase-abort' : 'merge-abort' });
    if (confirm === 'skip' && rebase) onOperation({ kind: 'rebase-skip' });
    setConfirm(null);
  };
  return (
    <div className="mt-2 flex min-w-0 items-center gap-2 rounded-md bg-amber-500/10 px-2 py-1.5 text-xs text-amber-700 dark:text-amber-300" data-source-control-conflict-workflow={operation.kind}>
      <span className="min-w-0 flex-1 truncate">{t(`sourceControl.conflictWorkflow.${operation.kind}InProgress`)}</span>
      <Button type="button" size="xs" disabled={busy} onClick={() => setConfirm('continue')}>{t(`sourceControl.conflictWorkflow.${rebase ? 'continueRebase' : 'completeMerge'}`)}</Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild><Button type="button" size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.conflictWorkflow.moreActions')}><MoreHorizontal className="size-3.5" /></Button></DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {rebase ? <DropdownMenuItem variant="destructive" onSelect={() => setConfirm('skip')}>{t('sourceControl.conflictWorkflow.skipCommit')}</DropdownMenuItem> : null}
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirm('abort')}>{t(`sourceControl.conflictWorkflow.${rebase ? 'abortRebase' : 'abortMerge'}`)}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <AlertDialog open={confirm !== null} onOpenChange={(open) => { if (!open) setConfirm(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t(`sourceControl.conflictWorkflow.confirm.${confirm ?? 'continue'}.title`, { operation: rebase ? 'Rebase' : 'Merge' })}</AlertDialogTitle>
            <AlertDialogDescription>{t(`sourceControl.conflictWorkflow.confirm.${confirm ?? 'continue'}.description`, {
              operation: rebase ? 'Rebase' : 'Merge',
              sha: operation.currentOid?.slice(0, 8) ?? '',
              subject: operation.currentSubject ?? '',
            })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel><AlertDialogAction className={confirm === 'continue' ? undefined : 'bg-destructive text-destructive-foreground hover:bg-destructive/90'} onClick={submit}>{t('common.confirm')}</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function SourceControlError({ error }: { error: GitOperationErrorVm }) {
  const { t } = useTranslation();
  const reason = typeof error.params.reason === 'string' ? error.params.reason.trim() : '';
  return (
    <div className="mt-1 text-ui-caption text-destructive" role="alert" aria-live="polite">
      <div>{t(`errors.${error.code}`, { ...error.params, defaultValue: t('sourceControl.operationFailed') })}</div>
      {reason ? <div className="mt-0.5 whitespace-pre-wrap break-words text-destructive/85">{reason}</div> : null}
    </div>
  );
}

function ChangeGroup({
  title,
  changes,
  tone,
  onOpen,
  actionLabel,
  actionIcon,
  pendingPath,
  disabled,
  onAction,
}: {
  title: string;
  changes: GitFileChangeVm[];
  tone: 'conflict' | 'staged' | 'unstaged' | 'untracked';
  onOpen: (change: GitFileChangeVm) => void;
  actionLabel?: string;
  actionIcon?: ReactNode;
  pendingPath?: string | null;
  disabled?: boolean;
  onAction?: (change: GitFileChangeVm) => void;
}) {
  if (changes.length === 0) return null;
  return (
    <TooltipProvider>
    <section data-source-control-group={tone}>
      <div className="flex h-7 items-center gap-2 px-3 text-ui-caption font-medium uppercase tracking-wide text-muted-foreground">
        <span>{title}</span><span className="tabular-nums">{changes.length}</span>
      </div>
      {changes.map((change) => {
        const actionPending = pendingPath === change.path;
        return (
          <SourceControlDiffFileRow
            key={`${change.path}:${change.indexStatus ?? ''}:${change.worktreeStatus ?? ''}`}
            path={change.path}
            oldPath={change.oldPath}
            kind={change.kind}
            addedLines={change.addedLines}
            deletedLines={change.deletedLines}
            onClick={() => onOpen(change)}
            trailing={onAction && (!disabled || actionPending) ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button type="button" size="icon-xs" variant="ghost" className={cn('opacity-0 group-hover:opacity-100 focus-visible:opacity-100', actionPending && 'opacity-100 disabled:opacity-100')} disabled={disabled} aria-busy={actionPending} aria-label={`${actionLabel}: ${change.path}`} onClick={() => onAction(change)}>
                    {actionPending ? <LoaderCircle className="size-3 animate-spin" /> : actionIcon}
                  </Button>
                </TooltipTrigger>
                <TooltipContent>{actionLabel}</TooltipContent>
              </Tooltip>
            ) : null}
          />
        );
      })}
    </section>
    </TooltipProvider>
  );
}

export function workspaceReviewItems(workspacePath: string | null | undefined, area: 'staged' | 'unstaged', changes: GitFileChangeVm[]): GitDiffReviewItem[] {
  return changes.map((change) => {
    const source = { kind: 'workspace' as const, workspacePath, path: change.path, area };
    return {
      id: gitComparisonReviewItemId(source),
      path: change.path,
      source,
      stats: { addedLines: change.addedLines ?? null, deletedLines: change.deletedLines ?? null },
    };
  });
}

function PanelState({ icon, text, description, action }: { icon?: ReactNode; text: string; description?: string; action?: ReactNode }) {
  return <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-sm text-muted-foreground"><span className="flex items-center gap-2 font-medium text-foreground">{icon}{text}</span>{description ? <p className="max-w-sm text-xs leading-relaxed">{description}</p> : null}{action}</div>;
}

function formatCommitTime(timestamp: string) {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}
