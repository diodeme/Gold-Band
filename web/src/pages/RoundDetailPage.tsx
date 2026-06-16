import { Component, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { AcpSessionVm, AcpUsageVm, AcpUiEventVm, AppConfigVm, AssetItemVm, ContentVm, GraphNodeVm, LogEntryVm, LogPageVm, LogQueryInput, NodeDetailVm, RoundDetailVm, RoundSelection } from '../types';
import { displayAppError, displayStatus } from '../i18n';
import { getLogPage, showArtifact, showAttachment } from '../api';
import { resolveNodeTokenUsage, formatDisplayToken } from '../lib/token-usage';
import { ACPChatDialog, createAcpPromptId, optimisticUserEvent, updateAcpOptimisticEvents } from '../components/acp/ACPChatDialog';
import { DetailViewerContent } from '../components/DetailViewer';
import { GraphView } from '../components/GraphView';
import { RequirementDetailSheet, RequirementTeaser, fullRequirementText } from '../components/RequirementDisclosure';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, MetricsBar, OverflowTooltip, Page, PageHeader } from '@/components/PageScaffold';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { ArrowLeft, MoreVertical, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/utils';
import { normalizeAcpEventForAttempt } from '@/lib/acp-event-normalization';
import { formatCurrentNode } from '@/lib/nodes';
import { normalizeTone } from '@/lib/status';
import { formatLocalDateTime } from '@/lib/datetime';

interface RoundDetailPageProps {
  vm: RoundDetailVm | null;
  breadcrumbs?: ReactNode;
  selection: RoundSelection;
  refreshing: boolean;
  busy: boolean;
  appConfig: AppConfigVm;
  workspaceProjectId?: string;
  onRefresh: () => void;
  onSelect: (selection: RoundSelection) => void;
  onContinueRun: (taskId: string, runId: string, promptId: string) => Promise<unknown>;
}

type NodeDrawerTab = 'detail' | 'session';

const defaultLogPageSize = 50;
const defaultHotLimit = 1000;

export function RoundDetailPage({ vm, breadcrumbs, selection, refreshing, busy, appConfig, workspaceProjectId, onRefresh, onSelect, onContinueRun }: RoundDetailPageProps) {
  const { t } = useTranslation();
  const [requirementOpen, setRequirementOpen] = useState(false);
  const [nodeDrawerOpen, setNodeDrawerOpen] = useState(false);
  const [nodeDrawerTab, setNodeDrawerTab] = useState<NodeDrawerTab>('detail');
  const [asset, setAsset] = useState<AssetItemVm | null>(null);
  const [assetContent, setAssetContent] = useState<ContentVm | null>(null);
  const [assetLoading, setAssetLoading] = useState(false);
  const [logDrawerOpen, setLogDrawerOpen] = useState(false);
  const [optimisticAcpEventsByKey, setOptimisticAcpEventsByKey] = useState<Record<string, AcpUiEventVm[]>>({});
  const selectedNodeId = selectedNodeIdFromSelection(selection);

  useEffect(() => {
    if (!asset || !vm) return undefined;
    let cancelled = false;
    setAssetLoading(true);
    const loader = asset.kind === 'attachment' ? showAttachment : showArtifact;
    loader(vm.run.taskId, vm.run.id, vm.round.id, asset.nodeId, asset.attemptId, asset.name, nodeDetailOuterNodeId(asset, vm.selectedNodeDetail), nodeDetailOuterAttemptId(asset, vm.selectedNodeDetail))
      .then((content) => { if (!cancelled) setAssetContent(content); })
      .catch((error) => { if (!cancelled) setAssetContent({ title: asset.title, kind: asset.kind, content: displayAppError(t, error), metadata: {} }); })
      .finally(() => { if (!cancelled) setAssetLoading(false); });
    return () => { cancelled = true; };
  }, [asset, t, vm]);

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;

  const requirement = fullRequirementText(vm.requirement, null, t('common.empty'));
  const roundDisplayStatus = displayPausedRuntimeStatus(vm.round.outcome ?? vm.round.status, vm.run.pauseReason);
  const roundTerminal = Boolean(vm.round.outcome) || ['success', 'danger'].includes(normalizeTone(roundDisplayStatus));
  const activeNodeId = vm.round.currentNode ?? vm.run.currentNode;
  const activeAttemptId = vm.run.currentAttempt;
  const currentNode = formatCurrentNode(t, vm.graph, activeNodeId);
  const nodeDetail = vm.selectedNodeDetail;
  const canContinueRound = isRoundContinuable(vm);

  const openNodeDrawer = (node: GraphNodeVm, tab: NodeDrawerTab = 'detail') => {
    const nodeId = canonicalNodeId(node);
    onSelect({ kind: 'node', nodeId, attemptId: node.attemptId ?? undefined, outerNodeId: node.outerNodeId ?? undefined, outerAttemptId: node.outerAttemptId ?? undefined });
    setNodeDrawerTab(tab);
    setNodeDrawerOpen(true);
    setAsset(null);
    setAssetContent(null);
  };

  const openGraphNodeLog = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node), attemptId: node.attemptId ?? undefined, outerNodeId: node.outerNodeId ?? undefined, outerAttemptId: node.outerAttemptId ?? undefined });
    setLogDrawerOpen(true);
  };

  const openAsset = (nextAsset: AssetItemVm) => {
    setAsset(nextAsset);
    setAssetContent(null);
  };

  const handleContinueRun = async () => {
    if (!activeNodeId || !activeAttemptId) return;
    const promptId = createAcpPromptId();
    const optimisticKey = acpOptimisticKey(vm.run.taskId, vm.run.id, vm.round.id, activeNodeId, activeAttemptId);
    const optimisticEvent = optimisticUserEvent(t('acp.continuePrompt'), promptId);
    const appendOptimisticEvent = (events: AcpUiEventVm[]) => [...events.filter((event) => promptIdFromAcpEvent(event) !== promptId), optimisticEvent];
    updateAcpOptimisticEvents(optimisticKey, appendOptimisticEvent);
    setOptimisticAcpEventsByKey((current) => ({
      ...current,
      [optimisticKey]: appendOptimisticEvent(current[optimisticKey] ?? []),
    }));
    const result = await onContinueRun(vm.run.taskId, vm.run.id, promptId);
    if (result) return;
    const markPromptFailed = (events: AcpUiEventVm[]) => events.map((event) => promptIdFromAcpEvent(event) === promptId ? { ...event, status: 'failed' } : event);
    updateAcpOptimisticEvents(optimisticKey, markPromptFailed);
    setOptimisticAcpEventsByKey((current) => {
      const events = markPromptFailed(current[optimisticKey] ?? []);
      if (events.length === 0) return current;
      return { ...current, [optimisticKey]: events };
    });
  };

  return (
    <Page flush className="flex flex-col overflow-y-auto overflow-x-hidden">
      <PageHeader
        className="px-5 py-3 xl:px-6"
        breadcrumbs={breadcrumbs}
        title={`${vm.run.id}/${vm.round.id}`}
        subtitle={(
          <div className="flex min-w-0 items-center gap-2 overflow-hidden text-xs">
            <span className="shrink-0 font-medium text-foreground">{t('common.requirement')}</span>
            <RequirementTeaser compact className="flex-1" text={requirement} detailLabel={t('common.viewFullRequirement')} onOpenDetail={() => setRequirementOpen(true)} />
          </div>
        )}
        actions={(
          <>
            <Button variant="outline" disabled={refreshing} onClick={onRefresh}>
              <RefreshCw className={cn(refreshing && 'animate-spin')} />
              {t('common.refresh')}
            </Button>
            <Button variant="outline" onClick={() => setLogDrawerOpen(true)}>{t('roundDetail.openLog')}</Button>
            {canContinueRound ? <Button disabled={busy} onClick={() => void handleContinueRun()}>{t('common.continueRun')}</Button> : null}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="icon" aria-label={t('roundDetail.moreActions')}>
                  <MoreVertical />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end"><DropdownMenuItem disabled>{t('roundDetail.moreActions')}</DropdownMenuItem></DropdownMenuContent>
            </DropdownMenu>
          </>
        )}
        metrics={(
          <MetricsBar className="lg:grid-cols-5 xl:grid-cols-5">
            <Metric label={t('roundDetail.trigger')} value={displayStatus(t, vm.round.trigger)} compact />
            <Metric label={t('roundDetail.maxAttempts')} value={formatLimit(vm.control?.maxAttempts, t)} compact />
            <Metric label={t('roundDetail.maxRounds')} value={formatLimit(vm.control?.maxRounds, t)} compact />
            <Metric label={roundTerminal ? t('roundDetail.finalNode') : t('common.currentNode')} value={currentNode} tooltip={currentNode} compact />
            <Metric label={t('common.outcome')} value={<StatusBadge value={roundDisplayStatus} label={displayStatus(t, roundDisplayStatus)} />} compact />
          </MetricsBar>
        )}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-4 xl:p-5">
        {vm.controlFailure ? (
          <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/25 bg-destructive/8 px-4 py-3 text-sm">
            <div className="min-w-0 space-y-1">
              <div className="font-medium text-foreground">{vm.controlFailure.title}</div>
              <div className="min-w-0 truncate text-muted-foreground">{vm.controlFailure.message}</div>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                const nodeId = vm.controlFailure?.toNodeId ?? vm.controlFailure?.nodeId ?? vm.controlFailure?.fromNodeId;
                if (!nodeId) return;
                onSelect({ kind: 'node', nodeId, attemptId: vm.controlFailure?.attemptId ?? undefined });
                setNodeDrawerTab('detail');
                setNodeDrawerOpen(true);
              }}
            >
              {t('roundDetail.viewFailureReason')}
            </Button>
          </div>
        ) : null}
        <AppCard className="flex min-h-[420px] min-w-0 flex-1 flex-col gap-0 overflow-hidden py-0">
          <CardHeader className="border-b px-4 py-2.5">
            <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <CardTitle className="shrink-0 whitespace-nowrap">{t('roundDetail.graph')}</CardTitle>
              {selectedNodeId ? <span className="min-w-0 truncate text-xs text-muted-foreground sm:flex-1 sm:text-right">{t('roundDetail.selectedNode', { node: formatCurrentNode(t, vm.graph, selectedNodeId) })}</span> : null}
            </div>
          </CardHeader>
          <CardContent className="min-h-0 flex-1 p-3">
            <GraphView
              graph={vm.graph}
              variant="actual"
              selectedNodeId={selectedNodeId}
              activeNodeId={vm.round.currentNode ?? vm.run.currentNode}
              onNodeSelect={(node) => onSelect({ kind: 'node', nodeId: canonicalNodeId(node), attemptId: node.attemptId ?? undefined, outerNodeId: node.outerNodeId ?? undefined, outerAttemptId: node.outerAttemptId ?? undefined })}
              onNodeOpenDetail={(node) => openNodeDrawer(node, 'detail')}
              onNodeOpenSession={(node) => openNodeDrawer(node, 'session')}
              onNodeOpenLog={openGraphNodeLog}
            />
          </CardContent>
        </AppCard>
      </div>
      <RequirementDetailSheet
        open={requirementOpen}
        title={t('common.fullRequirement')}
        description={t('common.fullRequirementDescription')}
        requirement={requirement}
        closeLabel={t('common.close')}
        onOpenChange={setRequirementOpen}
      />
      <NodeDetailSheet
        vm={vm}
        nodeDetail={nodeDetail}
        open={nodeDrawerOpen}
        activeTab={nodeDrawerTab}
        onOpenChange={setNodeDrawerOpen}
        onTabChange={setNodeDrawerTab}
        onRefresh={onRefresh}
        appConfig={appConfig}
        workspaceProjectId={workspaceProjectId}
        optimisticAcpEventsByKey={optimisticAcpEventsByKey}
        onOptimisticAcpEventsChange={(key, events) => setOptimisticAcpEventsByKey((current) => {
          const next = { ...current };
          if (events.length === 0) delete next[key];
          else next[key] = events;
          return next;
        })}
        onOpenAsset={openAsset}
      />
      <AssetDetailSheet
        asset={asset}
        content={assetContent}
        loading={assetLoading}
        onBack={() => {
          setNodeDrawerOpen(true);
          setAsset(null);
        }}
      />
      <LogDrawer vm={vm} open={logDrawerOpen} onOpenChange={setLogDrawerOpen} />
    </Page>
  );
}

function displayPausedRuntimeStatus(status: string, pauseReason?: string | null) {
  if (status === 'paused' && pauseReason === 'error-blocked') return pauseReason;
  return status;
}

function isRoundContinuable(vm: RoundDetailVm) {
  const activeNodeId = vm.round.currentNode ?? vm.run.currentNode;
  const activeAttemptId = vm.run.currentAttempt;
  const currentPausedNode = vm.graph.nodes.some((node) => {
    const sameNode = node.nodeId === activeNodeId || node.id === activeNodeId || node.outerNodeId === activeNodeId;
    const sameAttempt = !activeAttemptId || node.attemptId === activeAttemptId || node.outerAttemptId === activeAttemptId;
    return node.current && sameNode && sameAttempt && node.status === 'paused' && !node.outcome;
  });

  return Boolean(
    vm.run.resumable
    && vm.run.status === 'paused'
    && !vm.run.outcome
    && vm.round.status === 'paused'
    && !vm.round.outcome
    && vm.run.currentRound === vm.round.id
    && activeNodeId
    && activeAttemptId
    && currentPausedNode,
  );
}

function NodeDetailSheet({ vm, nodeDetail, open, activeTab, appConfig, workspaceProjectId, optimisticAcpEventsByKey, onOpenChange, onTabChange, onRefresh, onOptimisticAcpEventsChange, onOpenAsset }: { vm: RoundDetailVm; nodeDetail?: NodeDetailVm | null; open: boolean; activeTab: NodeDrawerTab; appConfig: AppConfigVm; workspaceProjectId?: string; optimisticAcpEventsByKey: Record<string, AcpUiEventVm[]>; onOpenChange: (open: boolean) => void; onTabChange: (tab: NodeDrawerTab) => void; onRefresh: () => void; onOptimisticAcpEventsChange: (key: string, events: AcpUiEventVm[]) => void; onOpenAsset: (asset: AssetItemVm) => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className="gap-0 overflow-hidden border-border bg-card p-0 shadow-2xl shadow-background/30"
        resizeStorageKey={`round-detail/node-detail/${activeTab}`}
        defaultSize={760}
        minSize={560}
        maxSize={1120}
        closeLabel={t('common.close')}
        showOverlay={false}
      >
        <Tabs value={activeTab} onValueChange={(value) => onTabChange(value as NodeDrawerTab)} className="h-full min-h-0 gap-0">
          <SheetHeader className="shrink-0 gap-0 border-b bg-muted/10 px-5 py-2.5 text-left">
            <SheetTitle className="sr-only">{nodeDetail?.label ?? t('roundDetail.nodeDetail')}</SheetTitle>
            <SheetDescription className="sr-only">{t('roundDetail.detailDrawerDescription')}</SheetDescription>
            <TabsList className="h-8 w-fit rounded-full border bg-background/70 p-1 shadow-sm">
              <TabsTrigger value="detail" className="h-6 rounded-full px-3 text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground data-[state=active]:shadow-none">
                {t('roundDetail.detailTab')}
              </TabsTrigger>
              <TabsTrigger value="session" className="h-6 rounded-full px-3 text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground data-[state=active]:shadow-none" disabled={!nodeDetail?.attemptId}>
                {t('roundDetail.sessionTab')}
              </TabsTrigger>
            </TabsList>
          </SheetHeader>
          <TabsContent value="detail" className="min-h-0 flex-1 overflow-hidden">
            <ScrollArea className="h-full">
              <div className="space-y-5 p-6">
                {nodeDetail ? <NodeDetailContent detail={nodeDetail} controlFailure={vm.controlFailure} runPauseReason={vm.run.pauseReason} onOpenAsset={onOpenAsset} /> : <EmptyState>{t('roundDetail.selectNodeForDetail')}</EmptyState>}
              </div>
            </ScrollArea>
          </TabsContent>
          <TabsContent value="session" className="min-h-0 flex-1 overflow-hidden">
            {nodeDetail ? <SessionContent vm={vm} detail={nodeDetail} appConfig={appConfig} workspaceProjectId={workspaceProjectId} onRefresh={onRefresh} optimisticAcpEventsByKey={optimisticAcpEventsByKey} onOptimisticAcpEventsChange={onOptimisticAcpEventsChange} /> : <EmptyState>{t('roundDetail.noSession')}</EmptyState>}
          </TabsContent>
        </Tabs>
      </SheetContent>
    </Sheet>
  );
}

function NodeDetailContent({ detail, controlFailure, runPauseReason, onOpenAsset }: { detail: NodeDetailVm; controlFailure?: RoundDetailVm['controlFailure']; runPauseReason?: string | null; onOpenAsset: (asset: AssetItemVm) => void }) {
  const { t } = useTranslation();
  const detailDisplayStatus = displayPausedRuntimeStatus(detail.outcome ?? detail.status, detail.current ? runPauseReason : null);
  const acpUsage = resolveNodeTokenUsage(detail);
  const baseItems: Array<[ReactNode, ReactNode]> = [
    [t('roundDetail.nodeId'), detail.nodeId],
    [t('roundDetail.sequence'), detail.sequence ?? '-'],
    [t('agentManagement.agentType'), detail.provider ?? '-'],
    [t('agentManagement.displayName'), detail.providerDisplayName ?? '-'],
    [t('workflowEditor.sessionMode'), detail.sessionMode ?? '-'],
    ['Continue From', detail.continueFromNodeId ?? '-'],
    [t('roundDetail.attemptId'), detail.attemptId],
    [t('roundDetail.attemptCount'), detail.acpConversations?.reduce((count, conversation) => count + conversation.attempts.length, 0) ?? 1],
    [t('roundDetail.startedAt'), formatLocalDateTime(detail.startedAt)],
    [t('roundDetail.finishedAt'), formatLocalDateTime(detail.finishedAt)],
    [t('workflowEditor.manualCheck'), detail.manualCheckEnabled ? (detail.manualCheckPending ? t('acp.manualCheckPending') : t('workflowEditor.enabled')) : t('workflowEditor.disabled')],
    [t('common.artifacts'), detail.artifactCount],
    [t('common.attachments'), detail.attachmentCount],
  ];
  if (acpUsage) {
    if (acpUsage.used != null && acpUsage.size != null) {
      baseItems.push([t('acp.usagePanel.contextWindow'), `${formatDisplayToken(acpUsage.used)} / ${formatDisplayToken(acpUsage.size)}`]);
    }
    baseItems.push([t('acp.usagePanel.input'), formatDisplayToken(acpUsage.inputTokens)]);
    baseItems.push([t('acp.usagePanel.output'), formatDisplayToken(acpUsage.outputTokens)]);
    baseItems.push([t('acp.usagePanel.cacheRead'), formatDisplayToken(acpUsage.cachedReadTokens)]);
    baseItems.push([t('acp.usagePanel.total'), formatDisplayToken(acpUsage.totalTokens)]);
  }
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-2">
        <StatusBadge value={detailDisplayStatus} label={displayStatus(t, detailDisplayStatus)} />
        <Badge variant="secondary" className="rounded-full px-3">{displayStatus(t, detail.nodeType)}</Badge>
        {detail.current ? <Badge className="rounded-full px-3">{t('graph.current')}</Badge> : null}
      </div>
      <InfoGrid items={baseItems} />
      {detail.dynamic ? <DynamicDetailSection detail={detail} /> : null}
      {controlFailure ? <ControlFailureDetail failure={controlFailure} /> : null}
      <AssetList title={t('common.artifacts')} items={detail.artifacts} emptyLabel={t('roundDetail.noArtifacts')} onOpenAsset={onOpenAsset} />
      <AssetList title={t('common.attachments')} items={detail.attachments} emptyLabel={t('roundDetail.noAttachments')} onOpenAsset={onOpenAsset} />
    </div>
  );
}

function DynamicDetailSection({ detail }: { detail: NodeDetailVm }) {
  const { t } = useTranslation();
  const dynamic = detail.dynamic;
  if (!dynamic) return null;
  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold">Dynamic Graph</h3>
        <Badge variant="secondary" className="rounded-full px-2.5">{dynamic.summary.internalNodeCount}</Badge>
      </div>
      <div className="rounded-xl border border-border/70 bg-muted/10 p-3">
        <div className="mb-3 grid gap-2 sm:grid-cols-3">
          <div className="rounded-xl bg-background/70 px-3 py-2"><div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">Status</div><div className="mt-1 text-sm font-medium">{displayStatus(t, dynamic.summary.status)}</div></div>
          <div className="rounded-xl bg-background/70 px-3 py-2"><div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">Groups</div><div className="mt-1 text-sm font-medium">{dynamic.summary.groupCount}</div></div>
          <div className="rounded-xl bg-background/70 px-3 py-2"><div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">Proposals</div><div className="mt-1 text-sm font-medium">{dynamic.summary.proposalCount}</div></div>
        </div>
        <div className="h-[320px] overflow-hidden rounded-xl border border-border/70 bg-background/80 p-2">
          <GraphView graph={dynamic.graph} variant="actual" />
        </div>
      </div>
      <div className="grid gap-3 lg:grid-cols-2">
        <section className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <h4 className="text-sm font-semibold">Groups</h4>
            <Badge variant="secondary" className="rounded-full px-2.5">{dynamic.groups.length}</Badge>
          </div>
          <div className="space-y-2">
            {dynamic.groups.map((group) => (
              <div key={group.id} className="rounded-xl border border-border/70 bg-muted/10 px-3 py-3 text-sm">
                <div className="flex items-center justify-between gap-3"><span className="font-medium">{group.id}</span><Badge variant="secondary" className="rounded-full px-2.5">{displayStatus(t, group.status)}</Badge></div>
                <div className="mt-2 text-xs text-muted-foreground">roots: {group.rootNodeIds.join(', ') || '-'} · terminals: {group.terminalNodeIds.join(', ') || '-'}</div>
              </div>
            ))}
            {dynamic.groups.length === 0 ? <div className="rounded-xl border border-dashed border-border/70 bg-muted/10 py-6 text-center text-sm text-muted-foreground">No groups</div> : null}
          </div>
        </section>
        <section className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <h4 className="text-sm font-semibold">Proposals</h4>
            <Badge variant="secondary" className="rounded-full px-2.5">{dynamic.proposals.length}</Badge>
          </div>
          <div className="space-y-2">
            {dynamic.proposals.map((proposal) => (
              <div key={proposal.id} className="rounded-xl border border-border/70 bg-muted/10 px-3 py-3 text-sm">
                <div className="flex items-center justify-between gap-3"><span className="font-medium">{proposal.id}</span><Badge variant="secondary" className="rounded-full px-2.5">{displayStatus(t, proposal.validationStatus)}</Badge></div>
                <div className="mt-2 text-xs text-muted-foreground">source: {proposal.sourceNodeId}</div>
                {proposal.validationErrors.length > 0 ? <div className="mt-2 space-y-1">{proposal.validationErrors.map((error) => <div key={`${proposal.id}:${error.code}:${error.message}`} className="rounded-lg border border-destructive/20 bg-destructive/5 px-2.5 py-2 text-xs text-destructive">[{error.code}] {error.message}</div>)}</div> : null}
              </div>
            ))}
            {dynamic.proposals.length === 0 ? <div className="rounded-xl border border-dashed border-border/70 bg-muted/10 py-6 text-center text-sm text-muted-foreground">No proposals</div> : null}
          </div>
        </section>
      </div>
    </section>
  );
}

function ControlFailureDetail({ failure }: { failure: NonNullable<RoundDetailVm['controlFailure']> }) {
  const { t } = useTranslation();
  return (
    <section className="rounded-xl border border-destructive/25 bg-destructive/8 p-4 text-sm">
      <div className="font-medium text-foreground">{failure.title}</div>
      <div className="mt-1 text-muted-foreground">{failure.message}</div>
      <div className="mt-3 grid gap-2 sm:grid-cols-3">
        <div><span className="text-muted-foreground">{t('roundDetail.transition')}</span><div className="font-mono text-xs">{failure.fromNodeId ?? '-'} → {failure.toNodeId ?? failure.target ?? '-'}</div></div>
        <div><span className="text-muted-foreground">{t('roundDetail.edgeOutcome')}</span><div className="font-mono text-xs">{failure.edgeOutcome ?? '-'}</div></div>
        <div><span className="text-muted-foreground">{t('roundDetail.limitUsage')}</span><div className="font-mono text-xs">{failure.proposedCount ?? '-'} / {failure.limit ?? '-'}</div></div>
      </div>
    </section>
  );
}

function InfoGrid({ items }: { items: Array<[ReactNode, ReactNode]> }) {
  return (
    <div className="grid gap-2 sm:grid-cols-3">
      {items.map(([label, value], index) => (
        <div className="rounded-xl border border-border/70 bg-muted/10 px-3 py-3" key={index}>
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">{label}</div>
          <OverflowTooltip className="mt-1.5 min-w-0" content={String(value)}>
            <div className="min-w-0 truncate text-sm font-medium text-foreground">{value}</div>
          </OverflowTooltip>
        </div>
      ))}
    </div>
  );
}

function AssetList({ title, items, emptyLabel, onOpenAsset }: { title: string; items: AssetItemVm[]; emptyLabel: string; onOpenAsset: (asset: AssetItemVm) => void }) {
  return (
    <section className="space-y-2.5">
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold">{title}</h3>
        <Badge variant="secondary" className="rounded-full px-2.5">{items.length}</Badge>
      </div>
      <div className="space-y-2">
        {items.map((item) => (
          <Button variant="outline" className="h-11 w-full justify-start gap-3 rounded-xl border-border/70 bg-background/60 px-3 text-left shadow-none hover:bg-muted/25" key={`${item.kind}-${item.name}`} onClick={() => onOpenAsset(item)}>
            <Badge variant="secondary" className="shrink-0 rounded-full px-2.5 text-[11px]">{item.kind}</Badge>
            <span className="min-w-0 flex-1 truncate text-sm font-medium">{item.title}</span>
          </Button>
        ))}
        {items.length === 0 ? <div className="rounded-xl border border-dashed border-border/70 bg-muted/10 py-8 text-center text-sm text-muted-foreground">{emptyLabel}</div> : null}
      </div>
    </section>
  );
}

function AssetDetailSheet({ asset, content, loading, onBack }: { asset: AssetItemVm | null; content: ContentVm | null; loading: boolean; onBack: () => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={Boolean(asset)} onOpenChange={(open) => { if (!open) onBack(); }}>
      <SheetContent
        className="gap-0 overflow-hidden border-border bg-card p-0 shadow-2xl shadow-background/30"
        resizeStorageKey={`round-detail/asset-detail/${asset?.kind ?? 'unknown'}`}
        defaultSize={680}
        minSize={480}
        maxSize={1040}
        closeLabel={t('common.close')}
        showOverlay={false}
      >
        <SheetHeader className="shrink-0 border-b px-5 py-4 text-left">
          <Button variant="ghost" size="sm" className="w-fit px-2" onClick={onBack}><ArrowLeft />{t('roundDetail.backToNode')}</Button>
          <SheetTitle className="truncate text-base">{asset?.title ?? t('roundDetail.assetDetail')}</SheetTitle>
          <SheetDescription className="sr-only">{t('roundDetail.assetDetail')}</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1">
          {loading ? <EmptyState>{t('common.loading')}</EmptyState> : <DetailViewerContent content={content} emptyLabel={t('common.empty')} />}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function promptIdFromAcpEvent(event: AcpUiEventVm) {
  if (!event.raw || typeof event.raw !== 'object' || Array.isArray(event.raw)) return null;
  const promptId = (event.raw as { promptId?: unknown }).promptId;
  return typeof promptId === 'string' ? promptId : null;
}

function acpOptimisticKey(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string) {
  return `${taskId}:${runId}:${roundId}:${nodeId}:${attemptId}`;
}

function SessionContent({ vm, detail, appConfig, workspaceProjectId, onRefresh, optimisticAcpEventsByKey, onOptimisticAcpEventsChange }: { vm: RoundDetailVm; detail: NodeDetailVm; appConfig: AppConfigVm; workspaceProjectId?: string; onRefresh: () => void; optimisticAcpEventsByKey: Record<string, AcpUiEventVm[]>; onOptimisticAcpEventsChange: (key: string, events: AcpUiEventVm[]) => void }) {
  const { t } = useTranslation();
  const conversations = detail.acpConversations?.length ? detail.acpConversations : [];
  const initialKey = detail.selectedConversationKey ?? conversations[0]?.key ?? 'current';
  const [conversationKey, setConversationKey] = useState(initialKey);
  useEffect(() => setConversationKey(initialKey), [initialKey]);
  const selectedConversation = conversations.find((conversation) => conversation.key === conversationKey) ?? conversations[0];
  const selectedAttempt = selectedConversation?.attempts.find((attempt) => attempt.attemptId === detail.attemptId);
  const activeAttempt = selectedAttempt
    ?? selectedConversation?.attempts.find((attempt) => attempt.attemptId === selectedConversation.activeAttemptId)
    ?? selectedConversation?.attempts.at(-1);
  const session = useMemo(
    () => selectedConversation ? mergedConversationSession(selectedConversation, detail.acpSession) : detail.acpSession,
    [detail.acpSession, selectedConversation],
  );
  const systemPromptOptions = useMemo(
    () => buildConversationSystemPromptOptions(selectedConversation, detail.acpSession, detail.attemptId),
    [detail.acpSession, detail.attemptId, selectedConversation],
  );
  const attemptId = activeAttempt?.attemptId ?? detail.attemptId;
  const runtimeStatus = activeAttempt?.status ?? detail.status;
  const optimisticKey = acpOptimisticKey(vm.run.taskId, vm.run.id, vm.round.id, detail.nodeId, attemptId);
  return (
    <div className="flex h-full min-h-0 flex-col">
      {conversations.length > 1 ? (
        <div className="shrink-0 border-b bg-muted/10 px-4 py-3">
          <Select value={selectedConversation?.key ?? conversationKey} onValueChange={setConversationKey}>
            <SelectTrigger className="h-8 w-[280px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {conversations.map((conversation) => <SelectItem value={conversation.key} key={conversation.key}>{conversation.label}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        <SessionErrorBoundary>
          <ACPChatDialog
          key={`${selectedConversation?.key ?? 'current'}:${detail.nodeId}:${attemptId}:${detail.outerNodeId ?? ''}:${detail.outerAttemptId ?? ''}`}
          session={session}
          projectId={workspaceProjectId ?? 'default'}
          systemPromptOptions={systemPromptOptions}
          eventIdPrefix={selectedConversation ? attemptId : undefined}
          eventPageSize={appConfig.acpChatEventPageSize}
          taskId={vm.run.taskId}
          runId={vm.run.id}
          roundId={vm.round.id}
          nodeId={detail.nodeId}
          attemptId={attemptId}
          outerNodeId={detail.outerNodeId}
          outerAttemptId={detail.outerAttemptId}
          runtimeComposerContext={{
            runtimeStatus,
            runtimeDisplay: undefined,
            workflowValid: true,
          }}
          manualCheckPending={detail.manualCheckPending && attemptId === detail.attemptId}
          optimisticEvents={optimisticAcpEventsByKey[optimisticKey]}
          onOptimisticEventsChange={(events) => onOptimisticAcpEventsChange(optimisticKey, events)}
          onManualCheckSubmitted={onRefresh}
          onSessionStopped={onRefresh}
          />
        </SessionErrorBoundary>
      </div>
    </div>
  );
}

class SessionErrorBoundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null };

  static getDerivedStateFromError(error: unknown) {
    return { error: error instanceof Error ? error.message : String(error) };
  }

  render() {
    if (this.state.error) {
      return <div className="m-4 rounded-xl border border-destructive/25 bg-destructive/8 p-4 text-sm text-destructive">{this.state.error}</div>;
    }
    return this.props.children;
  }
}

export function mergedConversationSession(conversation: NonNullable<NodeDetailVm['acpConversations']>[number], fallback?: AcpSessionVm | null): AcpSessionVm | null {
  const activeAttempt = conversation.attempts.find((attempt) => attempt.attemptId === conversation.activeAttemptId);
  const base = activeAttempt?.acpSession
    ?? conversation.attempts.at(-1)?.acpSession
    ?? fallback
    ?? null;
  if (!base) return null;
  let seq = 1;
  const events: AcpUiEventVm[] = [];
  conversation.attempts.forEach((attempt, index) => {
    if (index > 0) {
      events.push({
        id: `separator:${attempt.attemptId}`,
        seq: seq++,
        timestamp: attempt.acpSession?.sessionStartedAt ?? base.sessionStartedAt ?? '',
        kind: 'attemptSeparator',
        sessionId: conversation.sessionId ?? attempt.acpSessionId ?? null,
        title: attempt.attemptId,
        content: null,
        toolCallId: null,
        status: attempt.status,
        raw: { attemptId: attempt.attemptId, goldBandScope: { attemptId: attempt.attemptId, separator: true } },
      });
    }
    for (const event of attempt.acpSession?.events ?? []) {
      events.push(normalizeAcpEventForAttempt(event, attempt.attemptId, seq++));
    }
  });
  return {
    ...base,
    sessionId: conversation.sessionId ?? base.sessionId,
    restored: conversation.attempts.some((attempt) => attempt.acpSession?.restored),
    systemPromptAppend: activeAttempt?.acpSession?.systemPromptAppend
      ?? fallback?.systemPromptAppend
      ?? base.systemPromptAppend,
    events,
    eventPage: {
      ...base.eventPage,
      loadedCount: events.length,
      total: Math.max(base.eventPage.total, events.length),
    },
  };
}

export function buildConversationSystemPromptOptions(
  conversation: NonNullable<NodeDetailVm['acpConversations']>[number] | undefined,
  fallback?: AcpSessionVm | null,
  fallbackAttemptId?: string | null,
) {
  if (!conversation) {
    return fallbackAttemptId && fallback?.systemPromptAppend?.trim()
      ? [{ attemptId: fallbackAttemptId, prompt: fallback.systemPromptAppend }]
      : undefined;
  }
  const options = conversation.attempts.map((attempt) => ({
    attemptId: attempt.attemptId,
    prompt: attempt.acpSession?.systemPromptAppend,
  }));
  if (fallbackAttemptId && fallback?.systemPromptAppend?.trim()) {
    const index = options.findIndex((option) => option.attemptId === fallbackAttemptId);
    if (index >= 0) {
      if (!options[index]?.prompt?.trim()) {
        options[index] = { attemptId: fallbackAttemptId, prompt: fallback.systemPromptAppend };
      }
    } else {
      options.push({ attemptId: fallbackAttemptId, prompt: fallback.systemPromptAppend });
    }
  }
  return options;
}

function LogDrawer({ vm, open, onOpenChange }: { vm: RoundDetailVm; open: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation();
  const query = useMemo<LogQueryInput>(() => ({
    source: 'system',
    page: 0,
    pageSize: defaultLogPageSize,
    hotLimit: defaultHotLimit,
    scope: { taskId: vm.run.taskId, runId: vm.run.id, roundId: vm.round.id },
  }), [vm.round.id, vm.run.id, vm.run.taskId]);
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className="gap-0 overflow-hidden border-border bg-card p-0 shadow-2xl shadow-background/30"
        resizeStorageKey="round-detail/log-drawer"
        defaultSize={860}
        minSize={620}
        maxSize={1280}
        closeLabel={t('common.close')}
        showOverlay={false}
      >
        <SheetHeader className="shrink-0 border-b px-5 py-4 text-left">
          <SheetTitle>{t('roundDetail.systemLogs')}</SheetTitle>
          <SheetDescription>{t('roundDetail.hotLogHint', { count: defaultHotLimit, days: 30 })}</SheetDescription>
        </SheetHeader>
        <LogPageList query={query} exportable />
      </SheetContent>
    </Sheet>
  );
}

function LogPageList({ query, exportable = false, compact = false }: { query: LogQueryInput; exportable?: boolean; compact?: boolean }) {
  const { t } = useTranslation();
  const [page, setPage] = useState(query.page ?? 0);
  const [pageSize, setPageSize] = useState(query.pageSize ?? defaultLogPageSize);
  const [data, setData] = useState<LogPageVm | null>(null);
  const [loading, setLoading] = useState(false);
  const effectiveQuery = useMemo<LogQueryInput>(() => ({ ...query, page, pageSize }), [page, pageSize, query]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getLogPage(effectiveQuery)
      .then((result) => { if (!cancelled) setData(result); })
      .catch((error) => {
        if (!cancelled) {
          const message = displayAppError(t, error);
          setData({ items: [{ id: 'error', timestamp: '', entryType: 'error', summary: message, source: effectiveQuery.source ?? 'system', raw: message }], page, pageSize, total: 1, hasPrevious: false, hasNext: false, tier: 'hot', hotLimit: effectiveQuery.hotLimit ?? defaultHotLimit, archiveRetentionDays: 30 });
        }
      })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [effectiveQuery, page, pageSize, t]);

  const items = data?.items ?? [];
  const start = data && data.total > 0 ? data.page * data.pageSize + 1 : 0;
  const end = data ? Math.min(data.total, (data.page + 1) * data.pageSize) : 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className={cn('flex shrink-0 flex-wrap items-center justify-between gap-3 border-b text-sm text-muted-foreground', compact ? 'px-6 py-2.5' : 'px-5 py-3')}>
        <div className="flex items-center gap-2">
          {exportable ? <Button variant="outline" size="sm" onClick={() => exportLogItems(items)}>{t('roundDetail.exportLog')}</Button> : null}
          <span>{t('common.pageSize')}</span>
          <Select value={String(pageSize)} onValueChange={(value) => { setPageSize(Number(value)); setPage(0); }}>
            <SelectTrigger className="h-8 w-20"><SelectValue /></SelectTrigger>
            <SelectContent>
              {[25, 50, 100].map((value) => <SelectItem value={String(value)} key={value}>{value}</SelectItem>)}
            </SelectContent>
          </Select>
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className={cn('divide-y divide-border/70', compact ? 'px-4 py-2' : 'px-3 py-2')}>
          <div className={cn('grid gap-3 px-2 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground', compact ? 'grid-cols-[112px_96px_minmax(0,1fr)]' : 'grid-cols-[128px_110px_128px_96px_minmax(0,1fr)]')}>
            <span>{t('roundDetail.logTime')}</span>
            <span>{t('roundDetail.logType')}</span>
            {!compact ? <span>{t('roundDetail.logNode')}</span> : null}
            {!compact ? <span>{t('roundDetail.logStage')}</span> : null}
            <span>{t('roundDetail.logSummary')}</span>
          </div>
          {items.map((item) => <LogRow item={item} compact={compact} key={item.id} />)}
          {!loading && items.length === 0 ? <EmptyState className="py-10">{t('roundDetail.noLogs')}</EmptyState> : null}
          {loading ? <EmptyState className="py-10">{t('common.loading')}</EmptyState> : null}
        </div>
      </ScrollArea>
      <div className="flex shrink-0 items-center justify-between gap-3 border-t px-5 py-3 text-sm text-muted-foreground">
        <span>{t('common.pageRange', { start, end, total: data?.total ?? 0 })}</span>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" disabled={!data?.hasPrevious} onClick={() => setPage((value) => Math.max(0, value - 1))}>{t('common.previousPage')}</Button>
          <Button variant="outline" size="sm" disabled={!data?.hasNext} onClick={() => setPage((value) => value + 1)}>{t('common.nextPage')}</Button>
        </div>
      </div>
    </div>
  );
}

function LogRow({ item, compact }: { item: LogEntryVm; compact?: boolean }) {
  return (
    <div className={cn('grid gap-3 px-2 py-2.5 text-sm', compact ? 'grid-cols-[112px_96px_minmax(0,1fr)]' : 'grid-cols-[128px_110px_128px_96px_minmax(0,1fr)]')}>
      <OverflowTooltip className="min-w-0" content={formatLocalDateTime(item.timestamp)}><span className="block truncate text-muted-foreground">{formatLocalDateTime(item.timestamp)}</span></OverflowTooltip>
      <span className="truncate"><Badge variant="secondary" className="rounded-full px-2.5 text-[11px]">{item.entryType}</Badge></span>
      {!compact ? <OverflowTooltip className="min-w-0" content={item.nodeId ?? '-'}><span className="block truncate text-muted-foreground">{item.nodeId ?? '-'}</span></OverflowTooltip> : null}
      {!compact ? <OverflowTooltip className="min-w-0" content={item.stage ?? '-'}><span className="block truncate text-muted-foreground">{item.stage ?? '-'}</span></OverflowTooltip> : null}
      <OverflowTooltip className="min-w-0" content={item.summary}><span className="block min-w-0 truncate">{item.summary}</span></OverflowTooltip>
    </div>
  );
}

function exportLogItems(items: LogEntryVm[]) {
  const blob = new Blob([items.map((item) => JSON.stringify(item.raw)).join('\n')], { type: 'application/x-ndjson;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'gold-band-logs.jsonl';
  link.click();
  URL.revokeObjectURL(url);
}

function canonicalNodeId(node: GraphNodeVm) {
  return node.nodeId ?? node.id;
}

function nodeDetailOuterNodeId(asset: AssetItemVm, detail?: NodeDetailVm | null) {
  if (!detail || detail.nodeId !== asset.nodeId || detail.attemptId !== asset.attemptId) return undefined;
  return detail.outerNodeId ?? undefined;
}

function nodeDetailOuterAttemptId(asset: AssetItemVm, detail?: NodeDetailVm | null) {
  if (!detail || detail.nodeId !== asset.nodeId || detail.attemptId !== asset.attemptId) return undefined;
  return detail.outerAttemptId ?? undefined;
}

function formatLimit(value: number | null | undefined, t: (key: string) => string) {
  return value ?? t('workflow.unlimited');
}

function selectedNodeIdFromSelection(selection: RoundSelection) {
  if (selection.kind === 'node' || selection.kind === 'artifact' || selection.kind === 'attachment' || selection.kind === 'worker-ref') {
    return selection.nodeId;
  }
  if (selection.kind === 'event' || selection.kind === 'log') {
    return selection.nodeId ?? selection.contextNodeId;
  }
  return selection.contextNodeId;
}
