import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { GraphNodeVm, RoundDetailVm, RoundSelection, StreamItemVm } from '../types';
import { displayStatus } from '../i18n';
import { DetailViewerContent } from '../components/DetailViewer';
import { GraphView } from '../components/GraphView';
import { RequirementDetailSheet, RequirementTeaser, fullRequirementText } from '../components/RequirementDisclosure';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, MetricsBar, Page, PageHeader } from '@/components/PageScaffold';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sheet, SheetContent } from '@/components/ui/sheet';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { MoreVertical, Pin, PinOff, RefreshCw, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { toneSurfaceClass } from '@/lib/status';
import { formatCurrentNode } from '@/lib/nodes';

interface RoundDetailPageProps {
  vm: RoundDetailVm | null;
  breadcrumbs?: ReactNode;
  selection: RoundSelection;
  refreshing: boolean;
  onRefresh: () => void;
  onSelect: (selection: RoundSelection) => void;
}

type RoundTab = 'artifacts' | 'attachments';

const CONTEXT_MENU_DETAIL_CLOSE_DELAY_MS = 150;

export function RoundDetailPage({ vm, breadcrumbs, selection, refreshing, onRefresh, onSelect }: RoundDetailPageProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<RoundTab>('artifacts');
  const [detailOpen, setDetailOpen] = useState(false);
  const [requirementOpen, setRequirementOpen] = useState(false);
  const [detailPinned, setDetailPinned] = useState(false);
  const selectedNodeId = selectedNodeIdFromSelection(selection);
  const pinnedPanelWidth = detailOpen && detailPinned ? 'clamp(360px, 34vw, 520px)' : undefined;
  const streamGroups = useMemo(() => groupStream(vm?.stream ?? []), [vm?.stream]);
  const selectedNodeIds = useMemo(() => {
    if (!selectedNodeId) return new Set<string>();
    const graphNode = vm?.graph.nodes.find((node) => node.id === selectedNodeId || node.nodeId === selectedNodeId);
    return new Set([selectedNodeId, graphNode?.id, graphNode?.nodeId].filter(Boolean) as string[]);
  }, [selectedNodeId, vm?.graph.nodes]);
  const selectedNodeStreamGroups = useMemo(() => filterNodeStreamGroups(streamGroups, selectedNodeIds), [selectedNodeIds, streamGroups]);
  const availableTabs = useMemo(() => {
    const tabs: RoundTab[] = [];
    if (selectedNodeStreamGroups.artifacts.length > 0) tabs.push('artifacts');
    if (selectedNodeStreamGroups.attachments.length > 0) tabs.push('attachments');
    return tabs;
  }, [selectedNodeStreamGroups.artifacts.length, selectedNodeStreamGroups.attachments.length]);

  useEffect(() => {
    if (!availableTabs.includes(activeTab) && availableTabs[0]) setActiveTab(availableTabs[0]);
  }, [activeTab, availableTabs]);

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;

  const requirement = fullRequirementText(streamGroups.requirement?.content, null, t('common.empty'));
  const roundLogItem = streamGroups.events[0] ?? streamGroups.progress[0];
  const roundLogTarget = roundLogSelection(roundLogItem);
  const showNodePanel = availableTabs.length > 0;
  const activeTabItems = tabItems(activeTab, selectedNodeStreamGroups);
  const roundDisplayStatus = vm.round.outcome ?? vm.round.status;
  const currentNode = formatCurrentNode(t, vm.graph, vm.round.currentNode ?? vm.run.currentNode);

  const closeDetail = () => {
    setDetailOpen(false);
    setDetailPinned(false);
  };

  const pinDetail = (pinned: boolean) => {
    setDetailPinned(pinned);
    setDetailOpen(true);
  };

  const openDetail = () => setDetailOpen(true);

  const selectGraphNode = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node) });
    if (detailPinned) setDetailOpen(true);
  };
  const prepareGraphNodeContextMenu = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node) });
    if (detailOpen && !detailPinned) {
      setDetailOpen(false);
      return CONTEXT_MENU_DETAIL_CLOSE_DELAY_MS;
    }
    return 0;
  };
  const openGraphNodeDetail = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node) });
    openDetail();
  };
  const openGraphSession = (node: GraphNodeVm) => {
    if (!node.attemptId) return;
    onSelect({ kind: 'worker-ref', nodeId: canonicalNodeId(node), attemptId: node.attemptId });
    openDetail();
  };
  const openGraphNodeLog = (node: GraphNodeVm) => {
    const nodeId = canonicalNodeId(node);
    const logItem = streamGroups.progress.find((item) => item.nodeId === nodeId || item.nodeId === node.nodeId || item.nodeId === node.id);
    if (!logItem) return;
    onSelect({ kind: 'log', id: logItem.id, nodeId, attemptId: logItem.attemptId ?? undefined });
    openDetail();
  };
  const openStreamDetail = (nextSelection: RoundSelection) => {
    onSelect(withCurrentNodeContext(nextSelection, selection));
    openDetail();
  };
  const openRoundLog = () => {
    if (roundLogTarget) openStreamDetail(roundLogTarget);
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
            <Button variant="outline" disabled={!roundLogTarget} onClick={openRoundLog}>{t('roundDetail.openLog')}</Button>
            <Button variant="outline">{t('roundDetail.exportLog')}</Button>
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
            <Metric label={t('common.currentNode')} value={currentNode} tooltip={currentNode} compact />
            <Metric label={t('common.outcome')} value={<StatusBadge value={roundDisplayStatus} label={displayStatus(t, roundDisplayStatus)} />} compact />
          </MetricsBar>
        )}
      />
      <div className="grid min-h-0 flex-1 overflow-visible" style={{ gridTemplateColumns: pinnedPanelWidth ? `minmax(0, 1fr) ${pinnedPanelWidth}` : 'minmax(0, 1fr)' }}>
        <div className="min-h-0 min-w-0 overflow-visible p-4 xl:p-5">
          <div className={cn('grid h-full min-h-[380px] min-w-0 gap-4', showNodePanel ? 'grid-rows-[minmax(240px,0.9fr)_minmax(200px,1fr)]' : 'grid-rows-[minmax(340px,1fr)]')}>
          <AppCard className="flex min-h-0 min-w-0 flex-col gap-0 overflow-hidden py-0">
            <CardHeader className="border-b px-4 py-2.5">
              <CardTitle>{t('roundDetail.graph')}</CardTitle>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 p-3"><GraphView graph={vm.graph} variant="actual" selectedNodeId={selectedNodeId} onNodeSelect={selectGraphNode} onNodeContextMenuStart={prepareGraphNodeContextMenu} onNodeOpenDetail={openGraphNodeDetail} onNodeOpenSession={openGraphSession} onNodeOpenLog={openGraphNodeLog} /></CardContent>
          </AppCard>
          {showNodePanel ? (
            <AppCard className="flex min-h-0 min-w-0 flex-col gap-0 overflow-hidden py-0">
              <CardHeader className="border-b px-3 py-2 !pb-2">
                <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as RoundTab)}>
                  <div className="flex flex-wrap items-center gap-3">
                    <TabsList className="h-8">
                      {availableTabs.includes('artifacts') ? <TabsTrigger value="artifacts" className="h-6 px-2.5">{t('roundDetail.artifactTab', { count: selectedNodeStreamGroups.artifacts.length })}</TabsTrigger> : null}
                      {availableTabs.includes('attachments') ? <TabsTrigger value="attachments" className="h-6 px-2.5">{t('roundDetail.attachmentTab', { count: selectedNodeStreamGroups.attachments.length })}</TabsTrigger> : null}
                    </TabsList>
                    {selectedNodeId ? <span className="min-w-0 truncate text-xs text-muted-foreground">{t('roundDetail.selectedNode', { node: formatCurrentNode(t, vm.graph, selectedNodeId) })}</span> : null}
                  </div>
                </Tabs>
              </CardHeader>
              <CardContent className="min-h-0 flex-1 px-0 py-0"><ScrollArea className="h-full"><div className="space-y-2 p-1.5">{activeTabItems.map((item) => <StreamItem item={item} key={item.id} onOpenDetail={openStreamDetail} />)}{activeTabItems.length === 0 ? <EmptyState>{t('common.empty')}</EmptyState> : null}</div></ScrollArea></CardContent>
            </AppCard>
          ) : null}
        </div>
        </div>
        <RequirementDetailSheet
          open={requirementOpen}
          title={t('common.fullRequirement')}
          description={t('common.fullRequirementDescription')}
          requirement={requirement}
          closeLabel={t('common.close')}
          onOpenChange={setRequirementOpen}
        />
        {detailOpen && detailPinned ? (
          <aside className="min-h-0 min-w-0 border-l bg-card">
            <RoundDetailPanelContent content={vm.detail} emptyLabel={t('common.empty')} pinned={detailPinned} onClose={closeDetail} onPinnedChange={pinDetail} />
          </aside>
        ) : null}
        {!detailPinned ? (
          <RoundDetailSheet
            content={vm.detail}
            emptyLabel={t('common.empty')}
            open={detailOpen}
            pinned={detailPinned}
            onOpenChange={(open) => { if (open) setDetailOpen(true); else closeDetail(); }}
            onPinnedChange={pinDetail}
          />
        ) : null}
      </div>
    </Page>
  );
}

function RoundDetailSheet({ content, emptyLabel, open, pinned, onOpenChange, onPinnedChange }: { content: RoundDetailVm['detail']; emptyLabel: string; open: boolean; pinned: boolean; onOpenChange: (open: boolean) => void; onPinnedChange: (pinned: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent className="w-[520px] max-w-[calc(100vw-2rem)] gap-0 overflow-hidden border-border bg-card p-0 data-[state=closed]:duration-150 data-[state=open]:duration-200 sm:max-w-[520px]" closeLabel={t('common.close')} showOverlay={false}>
        <RoundDetailPanelContent content={content} emptyLabel={emptyLabel} pinned={pinned} onClose={() => onOpenChange(false)} onPinnedChange={onPinnedChange} />
      </SheetContent>
    </Sheet>
  );
}

function RoundDetailPanelContent({ content, emptyLabel, pinned, onClose, onPinnedChange }: { content: RoundDetailVm['detail']; emptyLabel: string; pinned: boolean; onClose: () => void; onPinnedChange: (pinned: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 space-y-3 border-b px-5 py-4 text-left">
        <div className="flex min-w-0 items-center justify-between gap-3 pr-8">
          <div className="min-w-0">
            <h2 className="truncate text-base font-semibold text-foreground">{t('common.detail')}</h2>
            <p className="sr-only">{t('roundDetail.detailDrawerDescription')}</p>
          </div>
          {content ? <span className="shrink-0 text-xs text-muted-foreground">{content.kind}</span> : null}
        </div>
        <div className="flex flex-wrap gap-2">
          <Button className="w-fit" variant="outline" size="sm" onClick={() => onPinnedChange(!pinned)}>
            {pinned ? <PinOff className="size-4" /> : <Pin className="size-4" />}
            {pinned ? t('roundDetail.unpinDetail') : t('roundDetail.pinDetail')}
          </Button>
          {pinned ? <Button className="w-fit" variant="outline" size="sm" onClick={onClose}><X className="size-4" />{t('common.close')}</Button> : null}
        </div>
      </div>
      <div className="min-h-0 flex-1">
        <DetailViewerContent content={content} emptyLabel={emptyLabel} />
      </div>
    </div>
  );
}

function StreamItem({ item, onOpenDetail }: { item: StreamItemVm; onOpenDetail: (selection: RoundSelection) => void }) {
  const target = streamTarget(item);
  return (
    <Button variant="outline" className={cn('h-auto w-full flex-col items-stretch justify-start gap-2 p-3 text-left', toneSurfaceClass(item.tone), !target && 'opacity-60')} onClick={() => target && onOpenDetail(target)} disabled={!target}>
      <span className="flex items-start justify-between gap-3">
        <strong className="line-clamp-1 text-sm">{item.title}</strong>
        <Badge variant="secondary" className="shrink-0 text-[10px]">{item.kind}</Badge>
      </span>
      <p className="line-clamp-4 whitespace-pre-wrap text-sm leading-6 text-muted-foreground">{item.content}</p>
    </Button>
  );
}

function groupStream(items: StreamItemVm[]) {
  return {
    requirement: items.find((item) => item.kind === 'requirement'),
    round: items.find((item) => item.kind === 'round'),
    events: items.filter((item) => item.kind === 'event'),
    progress: items.filter((item) => item.kind === 'log'),
    artifacts: items.filter((item) => item.kind === 'artifact'),
    attachments: items.filter((item) => item.kind === 'attachment'),
  };
}

function filterNodeStreamGroups(groups: ReturnType<typeof groupStream>, selectedNodeIds: Set<string>) {
  return {
    artifacts: groups.artifacts.filter((item) => item.nodeId ? selectedNodeIds.has(item.nodeId) : false),
    attachments: groups.attachments.filter((item) => item.nodeId ? selectedNodeIds.has(item.nodeId) : false),
  };
}

function tabItems(tab: RoundTab, groups: ReturnType<typeof filterNodeStreamGroups>) {
  if (tab === 'attachments') return groups.attachments;
  return groups.artifacts;
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

function withCurrentNodeContext(nextSelection: RoundSelection, currentSelection: RoundSelection) {
  const contextNodeId = nextSelection.contextNodeId ?? selectedNodeIdFromSelection(nextSelection) ?? selectedNodeIdFromSelection(currentSelection);
  return contextNodeId ? { ...nextSelection, contextNodeId } : nextSelection;
}

function roundLogSelection(item?: StreamItemVm): RoundSelection | null {
  if (!item) return null;
  if (item.kind === 'event') return { kind: 'event', id: item.id };
  if (item.kind === 'log') return { kind: 'log', id: item.id };
  return streamTarget(item);
}

function streamTarget(item: StreamItemVm): RoundSelection | null {
  if (item.kind === 'requirement') return { kind: 'requirement' };
  if (item.kind === 'round') return { kind: 'round' };
  if (item.kind === 'artifact' && item.nodeId && item.attemptId && item.name) return { kind: 'artifact', nodeId: item.nodeId, attemptId: item.attemptId, name: item.name };
  if (item.kind === 'attachment' && item.nodeId && item.attemptId && item.name) return { kind: 'attachment', nodeId: item.nodeId, attemptId: item.attemptId, name: item.name };
  if (item.kind === 'node' && item.nodeId) return { kind: 'node', nodeId: item.nodeId };
  if (item.kind === 'event') return { kind: 'event', id: item.id, nodeId: item.nodeId ?? undefined, attemptId: item.attemptId ?? undefined };
  if (item.kind === 'log') return { kind: 'log', id: item.id, nodeId: item.nodeId ?? undefined, attemptId: item.attemptId ?? undefined };
  return null;
}
