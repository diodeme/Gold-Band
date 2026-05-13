import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { AssetItemVm, ContentVm, GraphNodeVm, LogEntryVm, LogPageVm, LogQueryInput, NodeDetailVm, RoundDetailVm, RoundSelection } from '../types';
import { displayStatus } from '../i18n';
import { getLogPage, showArtifact, showAttachment } from '../api';
import { ACPChatDialog } from '../components/acp/ACPChatDialog';
import { DetailViewerContent } from '../components/DetailViewer';
import { GraphView } from '../components/GraphView';
import { RequirementDetailSheet, RequirementTeaser, fullRequirementText } from '../components/RequirementDisclosure';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, MetricsBar, Page, PageHeader } from '@/components/PageScaffold';
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
import { formatCurrentNode } from '@/lib/nodes';
import { normalizeTone } from '@/lib/status';

interface RoundDetailPageProps {
  vm: RoundDetailVm | null;
  breadcrumbs?: ReactNode;
  selection: RoundSelection;
  refreshing: boolean;
  onRefresh: () => void;
  onSelect: (selection: RoundSelection) => void;
}

type NodeDrawerTab = 'detail' | 'session';

const defaultLogPageSize = 50;
const defaultHotLimit = 1000;

export function RoundDetailPage({ vm, breadcrumbs, selection, refreshing, onRefresh, onSelect }: RoundDetailPageProps) {
  const { t } = useTranslation();
  const [requirementOpen, setRequirementOpen] = useState(false);
  const [nodeDrawerOpen, setNodeDrawerOpen] = useState(false);
  const [nodeDrawerTab, setNodeDrawerTab] = useState<NodeDrawerTab>('detail');
  const [asset, setAsset] = useState<AssetItemVm | null>(null);
  const [assetContent, setAssetContent] = useState<ContentVm | null>(null);
  const [assetLoading, setAssetLoading] = useState(false);
  const [logDrawerOpen, setLogDrawerOpen] = useState(false);
  const selectedNodeId = selectedNodeIdFromSelection(selection);

  useEffect(() => {
    if (!asset || !vm) return undefined;
    let cancelled = false;
    setAssetLoading(true);
    const loader = asset.kind === 'attachment' ? showAttachment : showArtifact;
    loader(vm.run.taskId, vm.run.id, vm.round.id, asset.nodeId, asset.attemptId, asset.name)
      .then((content) => { if (!cancelled) setAssetContent(content); })
      .catch((error) => { if (!cancelled) setAssetContent({ title: asset.title, kind: asset.kind, content: String(error), metadata: {} }); })
      .finally(() => { if (!cancelled) setAssetLoading(false); });
    return () => { cancelled = true; };
  }, [asset, vm]);

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;

  const requirement = fullRequirementText(vm.requirement, null, t('common.empty'));
  const roundDisplayStatus = vm.round.outcome ?? vm.round.status;
  const roundTerminal = Boolean(vm.round.outcome) || ['success', 'danger'].includes(normalizeTone(vm.round.status));
  const currentNode = formatCurrentNode(t, vm.graph, vm.round.currentNode ?? vm.run.currentNode);
  const nodeDetail = vm.selectedNodeDetail;

  const openNodeDrawer = (node: GraphNodeVm, tab: NodeDrawerTab = 'detail') => {
    const nodeId = canonicalNodeId(node);
    onSelect({ kind: 'node', nodeId });
    setNodeDrawerTab(tab);
    setNodeDrawerOpen(true);
    setAsset(null);
    setAssetContent(null);
  };

  const openGraphNodeLog = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node) });
    setLogDrawerOpen(true);
  };

  const openAsset = (nextAsset: AssetItemVm) => {
    setAsset(nextAsset);
    setAssetContent(null);
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
            <Button>{t('common.continueRun')}</Button>
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
          <MetricsBar className="lg:grid-cols-4 xl:grid-cols-4">
            <Metric label={t('roundDetail.trigger')} value={displayStatus(t, vm.round.trigger)} compact />
            <Metric label={t('roundDetail.repairLoopsUsed')} value={vm.round.repairLoopsUsed} compact />
            <Metric label={roundTerminal ? t('roundDetail.finalNode') : t('common.currentNode')} value={currentNode} tooltip={currentNode} compact />
            <Metric label={t('common.outcome')} value={<StatusBadge value={roundDisplayStatus} label={displayStatus(t, roundDisplayStatus)} />} compact />
          </MetricsBar>
        )}
      />
      <div className="min-h-0 flex-1 overflow-hidden p-4 xl:p-5">
        <AppCard className="flex h-full min-h-[420px] min-w-0 flex-col gap-0 overflow-hidden py-0">
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
              activeStatus={vm.round.status}
              onNodeSelect={(node) => onSelect({ kind: 'node', nodeId: canonicalNodeId(node) })}
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

function NodeDetailSheet({ vm, nodeDetail, open, activeTab, onOpenChange, onTabChange, onOpenAsset }: { vm: RoundDetailVm; nodeDetail?: NodeDetailVm | null; open: boolean; activeTab: NodeDrawerTab; onOpenChange: (open: boolean) => void; onTabChange: (tab: NodeDrawerTab) => void; onOpenAsset: (asset: AssetItemVm) => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className="gap-0 overflow-hidden border-border bg-card p-0 shadow-2xl shadow-background/30"
        style={{ width: 'min(760px, calc(100vw - 32px))', maxWidth: 'min(760px, calc(100vw - 32px))' }}
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
                {nodeDetail ? <NodeDetailContent detail={nodeDetail} onOpenAsset={onOpenAsset} /> : <EmptyState>{t('roundDetail.selectNodeForDetail')}</EmptyState>}
              </div>
            </ScrollArea>
          </TabsContent>
          <TabsContent value="session" className="min-h-0 flex-1 overflow-hidden">
            {nodeDetail ? <SessionContent vm={vm} detail={nodeDetail} /> : <EmptyState>{t('roundDetail.noSession')}</EmptyState>}
          </TabsContent>
        </Tabs>
      </SheetContent>
    </Sheet>
  );
}

function NodeDetailContent({ detail, onOpenAsset }: { detail: NodeDetailVm; onOpenAsset: (asset: AssetItemVm) => void }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-2">
        <StatusBadge value={detail.outcome ?? detail.status} label={displayStatus(t, detail.outcome ?? detail.status)} />
        <Badge variant="secondary" className="rounded-full px-3">{displayStatus(t, detail.nodeType)}</Badge>
        {detail.current ? <Badge className="rounded-full px-3">{t('graph.current')}</Badge> : null}
      </div>
      <InfoGrid items={[
        [t('roundDetail.nodeId'), detail.nodeId],
        [t('roundDetail.sequence'), detail.sequence ?? '-'],
        [t('roundDetail.attemptId'), detail.attemptId],
        [t('roundDetail.startedAt'), detail.startedAt || '-'],
        [t('roundDetail.finishedAt'), detail.finishedAt || '-'],
        [t('common.artifacts'), detail.artifactCount],
        [t('common.attachments'), detail.attachmentCount],
      ]} />
      <AssetList title={t('common.artifacts')} items={detail.artifacts} emptyLabel={t('roundDetail.noArtifacts')} onOpenAsset={onOpenAsset} />
      <AssetList title={t('common.attachments')} items={detail.attachments} emptyLabel={t('roundDetail.noAttachments')} onOpenAsset={onOpenAsset} />
    </div>
  );
}

function InfoGrid({ items }: { items: Array<[ReactNode, ReactNode]> }) {
  return (
    <div className="grid gap-2 sm:grid-cols-3">
      {items.map(([label, value], index) => (
        <div className="rounded-xl border border-border/70 bg-muted/10 px-3 py-3" key={index}>
          <div className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">{label}</div>
          <div className="mt-1.5 min-w-0 truncate text-sm font-medium text-foreground" title={String(value)}>{value}</div>
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
        style={{ width: 'min(680px, calc(100vw - 128px))', maxWidth: 'min(680px, calc(100vw - 128px))' }}
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

function SessionContent({ vm, detail }: { vm: RoundDetailVm; detail: NodeDetailVm }) {
  return (
    <ACPChatDialog
      session={detail.acpSession}
      taskId={vm.run.taskId}
      runId={vm.run.id}
      roundId={vm.round.id}
      nodeId={detail.nodeId}
      attemptId={detail.attemptId}
    />
  );
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
        style={{ width: 'min(860px, calc(100vw - 96px))', maxWidth: 'min(860px, calc(100vw - 96px))' }}
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
      .catch((error) => { if (!cancelled) setData({ items: [{ id: 'error', timestamp: '', entryType: 'error', summary: String(error), source: effectiveQuery.source ?? 'system', raw: String(error) }], page, pageSize, total: 1, hasPrevious: false, hasNext: false, tier: 'hot', hotLimit: effectiveQuery.hotLimit ?? defaultHotLimit, archiveRetentionDays: 30 }); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [effectiveQuery, page, pageSize]);

  const items = data?.items ?? [];
  const start = data && data.total > 0 ? data.page * data.pageSize + 1 : 0;
  const end = data ? Math.min(data.total, (data.page + 1) * data.pageSize) : 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className={cn('flex shrink-0 flex-wrap items-center justify-between gap-3 border-b text-sm text-muted-foreground', compact ? 'px-6 py-2.5' : 'px-5 py-3')}>
        {!compact ? <span>{t('roundDetail.hotLogHint', { count: data?.hotLimit ?? defaultHotLimit, days: data?.archiveRetentionDays ?? 30 })}</span> : <span>{query.source}</span>}
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
      <span className="truncate text-muted-foreground" title={item.timestamp}>{item.timestamp || '-'}</span>
      <span className="truncate"><Badge variant="secondary" className="rounded-full px-2.5 text-[11px]">{item.entryType}</Badge></span>
      {!compact ? <span className="truncate text-muted-foreground" title={item.nodeId ?? undefined}>{item.nodeId ?? '-'}</span> : null}
      {!compact ? <span className="truncate text-muted-foreground" title={item.stage ?? undefined}>{item.stage ?? '-'}</span> : null}
      <span className="min-w-0 truncate" title={item.summary}>{item.summary}</span>
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

function selectedNodeIdFromSelection(selection: RoundSelection) {
  if (selection.kind === 'node' || selection.kind === 'artifact' || selection.kind === 'attachment' || selection.kind === 'worker-ref') {
    return selection.nodeId;
  }
  if (selection.kind === 'event' || selection.kind === 'log') {
    return selection.nodeId ?? selection.contextNodeId;
  }
  return selection.contextNodeId;
}
