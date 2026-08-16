import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { ArrowLeft, CircleDot, ExternalLink, GitPullRequest, LoaderCircle, LogIn, Plus, RefreshCw, Search, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  cancelGitHubOperation,
  openExternalUrl,
  preflightGitHubPullRequest,
  startGitHubLogin,
  startGitHubPullRequestCreate,
} from '@/api';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Switch } from '@/components/ui/switch';
import type {
  GitHubCapabilityVm,
  GitHubIssueDetailVm,
  GitHubIssueSummaryVm,
  GitHubListStateVm,
  GitHubOperationVm,
  GitHubPullRequestPreflightVm,
  GitHubPullRequestDetailVm,
  GitHubPullRequestSummaryVm,
  GitSourceControlSnapshotVm,
} from '@/types';
import { WorkspaceFileEditor } from '../files/WorkspaceFileEditor';
import { gitDiffReviewWorkspaceResourceKey, useRightWorkspace } from '../right-workspace-context';
import { SourceControlDiffFileRow } from './SourceControlDiffFileRow';
import { diffReviewStore, gitComparisonReviewItemId } from './diff-review-store';
import { githubOperationEventStore } from './github-operation-store';
import {
  githubDataStore,
  githubRepositorySessionKey,
  useGitHubRepositoryNavigation,
  type GitHubRepositoryDetailSection,
} from './github-data-store';

const GITHUB_CLI_INSTALL_URL = 'https://cli.github.com/';

export function SourceControlGitHubView({
  projectId,
  workspacePath,
  snapshot,
  busy,
  onPush,
}: {
  projectId: string;
  workspacePath?: string | null;
  snapshot: GitSourceControlSnapshotVm;
  busy: boolean;
  onPush: (remote: string, branch: string) => void;
}) {
  const { t } = useTranslation();
  const sessionKey = githubRepositorySessionKey(
    projectId,
    snapshot.repository.commonDir,
    snapshot.repository.workspacePath,
  );
  const [capability, setCapability] = useState<GitHubCapabilityVm | null>(
    () => githubDataStore.peekCapability(sessionKey),
  );
  const [login, setLogin] = useState<GitHubOperationVm | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);

  const detect = useCallback(async (force = false) => {
    setErrorCode(null);
    try {
      setCapability(await githubDataStore.getCapability(sessionKey, projectId, workspacePath, force));
    } catch (reason) {
      setErrorCode(errorCodeFrom(reason, 'github.capability-failed'));
    }
  }, [projectId, sessionKey, workspacePath]);

  useEffect(() => {
    setCapability(githubDataStore.peekCapability(sessionKey));
    void detect();
  }, [detect, sessionKey]);
  useEffect(() => {
    if (!capability || !['not-installed', 'not-authenticated'].includes(capability.status)) return;
    const onFocus = () => { void detect(true); };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [capability, detect]);
  useEffect(() => githubOperationEventStore.subscribe((operation) => {
    setLogin((current) => current?.operationId === operation.operationId ? operation : current);
  }), []);
  useEffect(() => {
    if (login?.status === 'succeeded') {
      githubDataStore.invalidateRepository(sessionKey);
      void detect(true);
    }
    if (login?.error?.code) setErrorCode(login.error.code);
  }, [detect, login, sessionKey]);

  if (!capability) return <GitHubState icon={<LoaderCircle className="size-4 animate-spin" />} text={t('sourceControl.githubDetecting')} />;
  if (errorCode) return <GitHubState icon={<TriangleAlert className="size-4 text-destructive" />} text={t(`errors.${errorCode}`, { defaultValue: t('sourceControl.githubDetectionFailed') })} action={<Button size="sm" variant="outline" onClick={() => void detect(true)}>{t('sourceControl.detectAgain')}</Button>} />;
  if (capability.status === 'not-installed') {
    return <GitHubState icon={<GitPullRequest className="size-5" />} text={t('sourceControl.githubNotInstalled')} description={t('sourceControl.githubNotInstalledDescription')} action={<div className="flex gap-2"><Button size="sm" onClick={() => void openExternalUrl(GITHUB_CLI_INSTALL_URL)}>{t('sourceControl.openInstallPage')}</Button><Button size="sm" variant="outline" onClick={() => void detect(true)}>{t('sourceControl.detectAgain')}</Button></div>} />;
  }
  if (capability.status === 'not-authenticated') {
    const waiting = login && ['queued', 'running'].includes(login.status);
    return <GitHubState icon={waiting ? <LoaderCircle className="size-5 animate-spin" /> : <LogIn className="size-5" />} text={waiting ? t('sourceControl.githubLoginWaiting') : t('sourceControl.githubNotAuthenticated')} description={t('sourceControl.githubLoginDescription')} action={waiting ? <div className="flex gap-2"><Button size="sm" variant="outline" disabled={!login.cancelable} onClick={() => void cancelGitHubOperation(login.operationId).then((operation) => setLogin(githubOperationEventStore.reconcile(operation)))}>{t('common.cancel')}</Button><Button size="sm" variant="ghost" onClick={() => void detect(true)}>{t('sourceControl.detectAgain')}</Button></div> : <div className="flex gap-2"><Button size="sm" onClick={() => void startGitHubLogin(projectId, workspacePath, capability.host ?? 'github.com').then((operation) => setLogin(githubOperationEventStore.reconcile(operation)))}><LogIn className="size-3.5" />{t('sourceControl.loginWithBrowser')}</Button><Button size="sm" variant="outline" onClick={() => void detect(true)}>{t('sourceControl.detectAgain')}</Button></div>} />;
  }
  if (capability.status === 'repository-unresolved' || !capability.repository || !capability.host) {
    return <GitHubState icon={<TriangleAlert className="size-5" />} text={t('sourceControl.githubRepositoryUnresolved')} description={t('sourceControl.githubRepositoryUnresolvedDescription')} action={<Button size="sm" variant="outline" onClick={() => void detect(true)}><RefreshCw className="size-3.5" />{t('sourceControl.detectAgain')}</Button>} />;
  }
  return <GitHubReadyView sessionKey={sessionKey} projectId={projectId} workspacePath={workspacePath} capability={capability} snapshot={snapshot} busy={busy} onPush={onPush} />;
}

type GitHubSelection = { kind: 'pr'; detail: GitHubPullRequestDetailVm } | { kind: 'issue'; detail: GitHubIssueDetailVm };

function GitHubReadyView({ sessionKey, projectId, workspacePath, capability, snapshot, busy, onPush }: { sessionKey: string; projectId: string; workspacePath?: string | null; capability: GitHubCapabilityVm; snapshot: GitSourceControlSnapshotVm; busy: boolean; onPush: (remote: string, branch: string) => void }) {
  const { t } = useTranslation();
  const { scopeKey, openResource } = useRightWorkspace();
  const host = capability.host!;
  const repository = capability.repository!;
  const navigation = useGitHubRepositoryNavigation(sessionKey);
  const [searchDraft, setSearchDraft] = useState(navigation.search);
  const [prs, setPrs] = useState<GitHubPullRequestSummaryVm[]>([]);
  const [issues, setIssues] = useState<GitHubIssueSummaryVm[]>([]);
  const [selection, setSelection] = useState<GitHubSelection | null>(null);
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);

  useEffect(() => setSearchDraft(navigation.search), [navigation.search]);

  const openPullRequestFile = useCallback((detail: GitHubPullRequestDetailVm, path: string) => {
    if (!scopeKey) return;
    const items = pullRequestReviewItems(workspacePath, host, repository, detail);
    const item = items.find((candidate) => candidate.path === path);
    if (!item) return;
    const reviewSessionId = `${projectId}:github-pr:${workspacePath ?? 'main'}:${host}:${repository}:${detail.number}:${detail.baseRefOid}:${detail.headRefOid}`;
    diffReviewStore.save({ id: reviewSessionId, projectId, revision: `${detail.baseRefOid}:${detail.headRefOid}`, items });
    void openResource({
      kind: 'file-diff',
      key: gitDiffReviewWorkspaceResourceKey(projectId, reviewSessionId),
      scopeKey,
      title: path.split('/').at(-1) ?? path,
      description: path,
      attention: false,
      projectId,
      gitSource: item.source,
      reviewSessionId,
      reviewItemId: item.id,
      reviewLanding: 'top',
    });
  }, [host, openResource, projectId, repository, scopeKey, workspacePath]);

  const load = useCallback(async (force = false) => {
    setLoading(true);
    setErrorCode(null);
    try {
      if (navigation.section === 'prs') {
        setPrs(await githubDataStore.listPullRequests(sessionKey, projectId, workspacePath, host, repository, { state: navigation.listState, search: navigation.search || null, author: null, base: null, head: null, label: null }, force));
      } else {
        setIssues(await githubDataStore.listIssues(sessionKey, projectId, workspacePath, host, repository, { state: navigation.listState, search: navigation.search || null, author: null, assignee: null, label: null, milestone: null }, force));
      }
    } catch (reason) {
      setErrorCode(errorCodeFrom(reason, navigation.section === 'prs' ? 'github.pr-list-failed' : 'github.issue-list-failed'));
    } finally {
      setLoading(false);
    }
  }, [host, navigation.listState, navigation.search, navigation.section, projectId, repository, sessionKey, workspacePath]);
  useEffect(() => {
    if (!navigation.selection) void load();
  }, [load, navigation.selection]);

  useEffect(() => {
    let cancelled = false;
    const locator = navigation.selection;
    if (!locator) {
      setSelection(null);
      setDetailLoading(false);
      return () => { cancelled = true; };
    }
    const cached = locator.kind === 'pr'
      ? githubDataStore.peekPullRequest(sessionKey, host, repository, locator.number)
      : githubDataStore.peekIssue(sessionKey, host, repository, locator.number);
    if (cached) {
      setSelection({ kind: locator.kind, detail: cached } as GitHubSelection);
      setDetailLoading(false);
      return () => { cancelled = true; };
    }
    setSelection(null);
    setDetailLoading(true);
    setErrorCode(null);
    const request = locator.kind === 'pr'
      ? githubDataStore.getPullRequest(sessionKey, projectId, workspacePath, host, repository, locator.number)
      : githubDataStore.getIssue(sessionKey, projectId, workspacePath, host, repository, locator.number);
    void request.then((detail) => {
      if (!cancelled) setSelection({ kind: locator.kind, detail } as GitHubSelection);
    }).catch((reason: unknown) => {
      if (!cancelled) setErrorCode(errorCodeFrom(reason, locator.kind === 'pr' ? 'github.pr-detail-failed' : 'github.issue-detail-failed'));
    }).finally(() => {
      if (!cancelled) setDetailLoading(false);
    });
    return () => { cancelled = true; };
  }, [host, navigation.selection, projectId, repository, sessionKey, workspacePath]);

  if (navigation.selection) {
    if (selection) return <GitHubDetail selection={selection} host={host} repository={repository} section={navigation.detailSection} onSectionChange={(section) => githubDataStore.setDetailSection(sessionKey, section)} onBack={() => githubDataStore.select(sessionKey, null)} onOpenPullRequestFile={openPullRequestFile} />;
    return <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden" data-source-control-github-detail-state={detailLoading ? 'loading' : 'error'}><GitHubState icon={detailLoading ? <LoaderCircle className="size-4 animate-spin" /> : <TriangleAlert className="size-4 text-destructive" />} text={detailLoading ? t('sourceControl.githubDetailLoading') : t(`errors.${errorCode}`, { defaultValue: t('sourceControl.operationFailed') })} action={!detailLoading ? <Button size="sm" variant="outline" onClick={() => githubDataStore.select(sessionKey, null)}>{t('common.back')}</Button> : undefined} /></div>;
  }

  return (
    <>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden" data-source-control-github-ready="true">
      <div className="flex h-9 min-w-0 shrink-0 items-center gap-2 overflow-hidden border-b border-border/50 px-3 text-xs"><GitPullRequest className="size-3.5 shrink-0" /><span className="min-w-0 flex-1 truncate font-medium">{repository}</span><span className="max-w-32 min-w-0 truncate text-muted-foreground">@{capability.account}</span></div>
      <Tabs value={navigation.section} onValueChange={(value) => githubDataStore.setListContext(sessionKey, { section: value as typeof navigation.section })} className="min-h-0 min-w-0 flex-1 gap-0 overflow-hidden">
        <TabsList variant="line" className="h-9 w-full justify-start border-b border-border/50 px-2"><TabsTrigger value="prs">{t('sourceControl.pullRequests')}</TabsTrigger><TabsTrigger value="issues">{t('sourceControl.issues')}</TabsTrigger></TabsList>
        <div className="flex h-10 items-center gap-1.5 border-b border-border/40 px-2">
          <Select value={navigation.listState} onValueChange={(value) => githubDataStore.setListContext(sessionKey, { listState: value as GitHubListStateVm })}><SelectTrigger size="sm" className="w-24"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="open">{t('sourceControl.open')}</SelectItem><SelectItem value="closed">{t('sourceControl.closed')}</SelectItem><SelectItem value="all">{t('common.all')}</SelectItem></SelectContent></Select>
          <Input value={searchDraft} onChange={(event) => setSearchDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter') githubDataStore.setListContext(sessionKey, { search: searchDraft.trim() }); }} className="h-8 min-w-0 flex-1 text-xs" placeholder={t('sourceControl.githubSearch')} />
          <Button size="icon-xs" variant="ghost" aria-label={t('sourceControl.githubSearch')} onClick={() => githubDataStore.setListContext(sessionKey, { search: searchDraft.trim() })}><Search className="size-3.5" /></Button>
          <Button size="icon-xs" variant="ghost" aria-label={t('common.refresh')} onClick={() => void load(true)}><RefreshCw className={loading ? 'size-3.5 animate-spin' : 'size-3.5'} /></Button>
          {navigation.section === 'prs' ? <Button size="icon-xs" variant="ghost" disabled={busy} aria-label={t('sourceControl.createPullRequest')} onClick={() => setCreateOpen(true)}><Plus className="size-3.5" /></Button> : null}
        </div>
        {errorCode ? <div className="px-3 py-2 text-xs text-destructive">{t(`errors.${errorCode}`, { defaultValue: t('sourceControl.operationFailed') })}</div> : null}
        <TabsContent value="prs" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1 overflow-hidden"><div className="min-w-0 divide-y divide-border/40">{prs.map((item) => <GitHubListRow key={item.number} number={item.number} title={item.title} state={item.state} subtitle={`${item.headRefName} → ${item.baseRefName}`} labels={item.labels.map((label) => label.name)} onClick={() => githubDataStore.select(sessionKey, { kind: 'pr', number: item.number })} />)}</div>{!loading && prs.length === 0 ? <GitHubState text={t('sourceControl.noPullRequests')} /> : null}</ScrollArea></TabsContent>
        <TabsContent value="issues" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1"><ScrollArea className="min-h-0 min-w-0 flex-1 overflow-hidden"><div className="min-w-0 divide-y divide-border/40">{issues.map((item) => <GitHubListRow key={item.number} number={item.number} title={item.title} state={item.state} subtitle={item.author?.login ?? ''} labels={item.labels.map((label) => label.name)} onClick={() => githubDataStore.select(sessionKey, { kind: 'issue', number: item.number })} />)}</div>{!loading && issues.length === 0 ? <GitHubState text={t('sourceControl.noIssues')} /> : null}</ScrollArea></TabsContent>
      </Tabs>
      </div>
      <CreatePullRequestDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        projectId={projectId}
        workspacePath={workspacePath}
        capability={capability}
        snapshot={snapshot}
        busy={busy}
        onPush={onPush}
        onCreated={() => void load(true)}
      />
    </>
  );
}

function CreatePullRequestDialog({
  open,
  onOpenChange,
  projectId,
  workspacePath,
  capability,
  snapshot,
  busy,
  onPush,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string;
  workspacePath?: string | null;
  capability: GitHubCapabilityVm;
  snapshot: GitSourceControlSnapshotVm;
  busy: boolean;
  onPush: (remote: string, branch: string) => void;
  onCreated: () => void;
}) {
  const { t } = useTranslation();
  const localBranches = snapshot.refs.filter((ref) => ref.kind === 'local-branch').map((ref) => ref.shortName);
  const defaultHead = snapshot.repository.currentBranch ?? localBranches[0] ?? '';
  const defaultBase = capability.defaultBranch ?? localBranches.find((branch) => branch !== defaultHead) ?? '';
  const baseBranches = Array.from(new Set([capability.defaultBranch, ...localBranches].filter((branch): branch is string => Boolean(branch))));
  const [head, setHead] = useState(defaultHead);
  const [base, setBase] = useState(defaultBase);
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [draft, setDraft] = useState(false);
  const [checking, setChecking] = useState(false);
  const [preflight, setPreflight] = useState<GitHubPullRequestPreflightVm | null>(null);
  const [operation, setOperation] = useState<GitHubOperationVm | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [notifiedOperationId, setNotifiedOperationId] = useState<string | null>(null);
  const operationRunning = Boolean(operation && ['queued', 'running'].includes(operation.status));

  useEffect(() => {
    if (!open) return;
    setHead(defaultHead);
    setBase(defaultBase);
    setTitle('');
    setBody('');
    setDraft(false);
    setChecking(false);
    setPreflight(null);
    setOperation(null);
    setErrorCode(null);
    setNotifiedOperationId(null);
  }, [defaultBase, defaultHead, open]);

  useEffect(() => githubOperationEventStore.subscribe((next) => {
    setOperation((current) => current?.operationId === next.operationId ? next : current);
  }), []);

  useEffect(() => {
    if (!operation || operation.status !== 'succeeded' || notifiedOperationId === operation.operationId) return;
    setNotifiedOperationId(operation.operationId);
    onCreated();
  }, [notifiedOperationId, onCreated, operation]);

  const changeBranch = (kind: 'head' | 'base', value: string) => {
    if (kind === 'head') setHead(value);
    else setBase(value);
    setPreflight(null);
    setErrorCode(null);
  };

  const create = async () => {
    if (!capability.host || !capability.repository || !head || !base || !title.trim()) return;
    setChecking(true);
    setPreflight(null);
    setErrorCode(null);
    const preflightInput = { host: capability.host, repository: capability.repository, head, base };
    try {
      const nextPreflight = await preflightGitHubPullRequest(projectId, workspacePath, preflightInput);
      setPreflight(nextPreflight);
      if (!nextPreflight.headPublished || nextPreflight.existingPullRequest) return;
      const started = await startGitHubPullRequestCreate(projectId, workspacePath, {
        ...preflightInput,
        title: title.trim(),
        body,
        draft,
      });
      setOperation(githubOperationEventStore.reconcile(started));
    } catch (reason) {
      setErrorCode(errorCodeFrom(reason, 'github.pr-create-failed'));
    } finally {
      setChecking(false);
    }
  };

  const cancel = () => {
    if (!operation?.cancelable) return;
    void cancelGitHubOperation(operation.operationId)
      .then((next) => setOperation(githubOperationEventStore.reconcile(next)))
      .catch((reason: unknown) => setErrorCode(errorCodeFrom(reason, 'github.operation-failed')));
  };

  const disabled = checking || operationRunning;
  return (
    <Dialog open={open} onOpenChange={(next) => { if (!operationRunning) onOpenChange(next); }}>
      <DialogContent className="flex h-[min(82vh,760px)] max-w-3xl flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="shrink-0 border-b border-border/50 px-5 py-4">
          <DialogTitle>{t('sourceControl.createPullRequest')}</DialogTitle>
          <DialogDescription>{t('sourceControl.createPullRequestDescription')}</DialogDescription>
        </DialogHeader>
        <div className="grid shrink-0 grid-cols-2 gap-3 border-b border-border/40 px-5 py-3">
          <div className="grid gap-1.5">
            <Label>{t('sourceControl.headBranch')}</Label>
            <Select value={head} disabled={disabled} onValueChange={(value) => changeBranch('head', value)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{localBranches.map((branch) => <SelectItem key={branch} value={branch}>{branch}</SelectItem>)}</SelectContent></Select>
          </div>
          <div className="grid gap-1.5">
            <Label>{t('sourceControl.baseBranch')}</Label>
            <Select value={base} disabled={disabled} onValueChange={(value) => changeBranch('base', value)}><SelectTrigger className="w-full"><SelectValue /></SelectTrigger><SelectContent>{baseBranches.map((branch) => <SelectItem key={branch} value={branch}>{branch}</SelectItem>)}</SelectContent></Select>
          </div>
          <div className="col-span-2 grid gap-1.5">
            <Label htmlFor="github-pr-title">{t('sourceControl.pullRequestTitle')}</Label>
            <Input id="github-pr-title" value={title} disabled={disabled} onChange={(event) => setTitle(event.target.value)} />
          </div>
        </div>
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex h-9 shrink-0 items-center justify-between border-b border-border/40 px-5">
            <Label>{t('sourceControl.pullRequestBody')}</Label>
            <label className="flex items-center gap-2 text-xs text-muted-foreground"><Switch checked={draft} disabled={disabled} onCheckedChange={setDraft} />{t('sourceControl.createAsDraft')}</label>
          </div>
          <div className="min-h-0 flex-1">
            <WorkspaceFileEditor documentKey={`github-pr-create:${capability.repository}:${head}:${base}`} value={body} editable={!disabled} language="markdown" highlight contentRevision={0} target={null} targetRevision={0} onChange={setBody} onSave={() => undefined} initialStateJson={null} onPersistState={() => undefined} markdownMode="live-preview" markdownLivePreviewAvailable />
          </div>
        </div>
        <div className="shrink-0 border-t border-border/50 px-5 py-3">
          {errorCode ? <div className="mb-2 text-xs text-destructive">{t(`errors.${errorCode}`, { defaultValue: t('sourceControl.operationFailed') })}</div> : null}
          {preflight && !preflight.headPublished ? <div className="mb-2 flex items-center gap-2 text-xs text-amber-600 dark:text-amber-400"><span className="min-w-0 flex-1">{t('sourceControl.pullRequestHeadNotPublished', { head: preflight.head, remote: preflight.remote })}</span><Button size="xs" variant="outline" disabled={busy} onClick={() => { onPush(preflight.remote, preflight.head); setPreflight(null); }}>{t('sourceControl.pushHeadFirst')}</Button></div> : null}
          {preflight?.existingPullRequest ? <div className="mb-2 flex items-center gap-2 text-xs text-amber-600 dark:text-amber-400"><span className="min-w-0 flex-1">{t('sourceControl.pullRequestAlreadyExists', { number: preflight.existingPullRequest.number })}</span><Button size="xs" variant="outline" onClick={() => void openExternalUrl(preflight.existingPullRequest!.url)}>{t('sourceControl.openOnGitHub')}</Button></div> : null}
          {operationRunning ? <div className="mb-2 flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('sourceControl.creatingPullRequest')}</div> : null}
          {operation?.status === 'succeeded' && operation.resultUrl ? <div className="mb-2 flex items-center gap-2 text-xs text-emerald-600"><span className="min-w-0 flex-1">{t('sourceControl.pullRequestCreated')}</span><Button size="xs" variant="outline" onClick={() => void openExternalUrl(operation.resultUrl!)}>{t('sourceControl.openOnGitHub')}</Button></div> : null}
          <DialogFooter>
            {operationRunning ? <Button variant="outline" disabled={!operation?.cancelable} onClick={cancel}>{t('common.cancel')}</Button> : <Button variant="outline" onClick={() => onOpenChange(false)}>{operation?.status === 'succeeded' ? t('common.close') : t('common.cancel')}</Button>}
            {operation?.status !== 'succeeded' ? <Button disabled={disabled || busy || !head || !base || !title.trim()} onClick={() => void create()}>{checking ? <LoaderCircle className="size-3.5 animate-spin" /> : <GitPullRequest className="size-3.5" />}{t('sourceControl.checkAndCreatePullRequest')}</Button> : null}
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function GitHubDetail({
  selection,
  host,
  repository,
  section,
  onSectionChange,
  onBack,
  onOpenPullRequestFile,
}: {
  selection: GitHubSelection;
  host: string;
  repository: string;
  section: GitHubRepositoryDetailSection;
  onSectionChange: (section: GitHubRepositoryDetailSection) => void;
  onBack: () => void;
  onOpenPullRequestFile: (detail: GitHubPullRequestDetailVm, path: string) => void;
}) {
  const { t } = useTranslation();
  const detail = selection.detail;
  const openMarkdownLink = (href: string) => {
    const url = githubMarkdownUrl(href, host, repository, selection.kind === 'pr' ? selection.detail.headRefName : 'HEAD');
    if (url) void openExternalUrl(url);
  };
  const body = <WorkspaceFileEditor documentKey={`github:${repository}:${selection.kind}:${detail.number}`} value={detail.body || t('sourceControl.noDescription')} editable={false} language="markdown" highlight contentRevision={0} target={null} targetRevision={0} onChange={() => undefined} onSave={() => undefined} initialStateJson={null} onPersistState={() => undefined} markdownMode="live-preview" markdownLivePreviewAvailable onMarkdownLinkClick={openMarkdownLink} />;
  return (
    <div className="flex min-h-0 min-w-0 max-w-full flex-1 flex-col overflow-hidden" data-source-control-github-detail="true">
      <div className="flex min-h-10 min-w-0 shrink-0 items-center gap-2 overflow-hidden border-b border-border/50 px-2">
        <Button size="icon-xs" variant="ghost" onClick={onBack} aria-label={t('common.back')}><ArrowLeft className="size-3.5" /></Button>
        <span className="min-w-0 flex-1 truncate text-xs font-medium" data-source-control-github-detail-title="true">#{detail.number} {detail.title}</span>
        <Button size="icon-xs" variant="ghost" onClick={() => void openExternalUrl(detail.url)} aria-label={t('sourceControl.openOnGitHub')}><ExternalLink className="size-3.5" /></Button>
      </div>
      <div className="flex h-8 min-w-0 shrink-0 items-center gap-2 overflow-hidden border-b border-border/40 px-3 text-ui-micro text-muted-foreground" data-source-control-github-detail-meta="true">
        <Badge variant="outline" className="h-5 shrink-0">{detail.state}</Badge>
        <span className="max-w-24 min-w-0 truncate">{detail.author?.login}</span>
        {selection.kind === 'pr' ? <><span className="min-w-0 flex-1 truncate" data-source-control-github-detail-branches="true">{selection.detail.headRefName} → {selection.detail.baseRefName}</span><span className="shrink-0 text-emerald-600">+{selection.detail.additions}</span><span className="shrink-0 text-destructive">-{selection.detail.deletions}</span></> : null}
      </div>
      {selection.kind === 'issue' ? <div className="min-h-0 min-w-0 flex-1 overflow-hidden">{body}</div> : (
        <Tabs value={section} onValueChange={(value) => onSectionChange(value as GitHubRepositoryDetailSection)} className="min-h-0 min-w-0 flex-1 gap-0 overflow-hidden">
          <TabsList variant="line" className="h-9 w-full justify-start border-b border-border/50 px-2">
            <TabsTrigger value="overview">{t('sourceControl.overview')}</TabsTrigger>
            <TabsTrigger value="files">{t('sourceControl.githubChangedFiles', { count: selection.detail.files.length })}</TabsTrigger>
          </TabsList>
          <TabsContent value="overview" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1">{body}</TabsContent>
          <TabsContent value="files" className="min-h-0 min-w-0 overflow-hidden data-[state=active]:flex data-[state=active]:flex-1">
            <ScrollArea className="min-h-0 min-w-0 flex-1 overflow-hidden">
              <div className="min-w-0 divide-y divide-border/40">
                {selection.detail.files.map((file) => (
                  <SourceControlDiffFileRow key={file.path} path={file.path} oldPath={file.oldPath} kind={file.kind} addedLines={file.additions} deletedLines={file.deletions} onClick={() => onOpenPullRequestFile(selection.detail, file.path)} className="px-2" />
                ))}
              </div>
              {selection.detail.files.length === 0 ? <GitHubState text={t('sourceControl.noChangedFiles')} /> : null}
            </ScrollArea>
          </TabsContent>
        </Tabs>
      )}
    </div>
  );
}

function GitHubListRow({ number, title, state, subtitle, labels, onClick }: { number: number; title: string; state: string; subtitle: string; labels: string[]; onClick: () => void }) {
  return <button type="button" className="flex w-full min-w-0 max-w-full items-start gap-2 overflow-hidden px-3 py-2 text-left hover:bg-muted/40" onClick={onClick}><CircleDot className="mt-0.5 size-3.5 shrink-0 text-emerald-500" /><span className="min-w-0 flex-1 overflow-hidden"><span className="block truncate text-xs font-medium">{title}</span><span className="mt-0.5 flex min-w-0 items-center gap-1.5 overflow-hidden text-ui-micro text-muted-foreground"><span className="shrink-0">#{number}</span><span className="shrink-0">{state}</span><span className="min-w-0 flex-1 truncate">{subtitle}</span>{labels.slice(0, 2).map((label) => <Badge key={label} variant="outline" className="h-4 max-w-24 shrink-0 truncate px-1 text-ui-nano">{label}</Badge>)}</span></span></button>;
}

function GitHubState({ icon, text, description, action }: { icon?: ReactNode; text: string; description?: string; action?: ReactNode }) {
  return <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 px-6 py-8 text-center"><span className="flex items-center gap-2 text-sm font-medium">{icon}{text}</span>{description ? <p className="max-w-sm text-xs text-muted-foreground">{description}</p> : null}{action ? <div className="mt-1">{action}</div> : null}</div>;
}

export function pullRequestReviewItems(workspacePath: string | null | undefined, host: string, repository: string, detail: GitHubPullRequestDetailVm) {
  return detail.files.map((file) => {
    const source = { kind: 'github-pr' as const, workspacePath, host, repository, prNumber: detail.number, baseOid: detail.baseRefOid, headOid: detail.headRefOid, path: file.path, beforePath: file.oldPath ?? null };
    return {
      id: gitComparisonReviewItemId(source),
      path: file.path,
      source,
      stats: { addedLines: file.additions, deletedLines: file.deletions },
    };
  });
}

function githubMarkdownUrl(href: string, host: string, repository: string, refName: string) {
  const value = href.trim();
  if (/^https?:\/\//iu.test(value)) return value;
  const shorthand = value.match(/^#(\d+)$/u);
  if (shorthand) return `https://${host}/${repository}/issues/${shorthand[1]}`;
  if (value.startsWith('/') || value.startsWith('#') || value.includes('..')) return null;
  return `https://${host}/${repository}/blob/${encodeURIComponent(refName)}/${value.replace(/^\.\//u, '')}`;
}

function errorCodeFrom(reason: unknown, fallback: string) {
  return typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string' ? reason.code : fallback;
}
