import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GraphNodeVm, RoundDetailVm, RoundSelection, StreamItemVm } from '../types';
import { displayStatus } from '../i18n';
import { DetailViewerContent } from '../components/DetailViewer';
import { GraphView } from '../components/GraphView';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, Page } from '@/components/PageScaffold';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { MoreVertical, Pin, PinOff } from 'lucide-react';
import { cn } from '@/lib/utils';
import { toneSurfaceClass } from '@/lib/status';

interface RoundDetailPageProps {
  vm: RoundDetailVm | null;
  selection: RoundSelection;
  onSelect: (selection: RoundSelection) => void;
}

type RoundTab = 'requirement' | 'log' | 'artifacts' | 'attachments';

export function RoundDetailPage({ vm, selection, onSelect }: RoundDetailPageProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<RoundTab>('requirement');
  const [detailOpen, setDetailOpen] = useState(false);
  const [detailPinned, setDetailPinned] = useState(false);
  const selectedNodeId = selectedNodeIdFromSelection(selection);
  const streamGroups = useMemo(() => groupStream(vm?.stream ?? []), [vm?.stream]);
  const availableTabs = useMemo(() => {
    const tabs: RoundTab[] = ['requirement', 'log'];
    if (selectedNodeId && streamGroups.artifacts.length > 0) tabs.push('artifacts');
    if (selectedNodeId && streamGroups.attachments.length > 0) tabs.push('attachments');
    return tabs;
  }, [selectedNodeId, streamGroups.artifacts.length, streamGroups.attachments.length]);

  useEffect(() => {
    if (!availableTabs.includes(activeTab)) setActiveTab('requirement');
  }, [activeTab, availableTabs]);

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;

  const closeDetail = () => {
    setDetailOpen(false);
    setDetailPinned(false);
  };

  const openDetail = () => setDetailOpen(true);

  const selectGraphNode = (node: GraphNodeVm) => {
    onSelect({ kind: 'node', nodeId: canonicalNodeId(node) });
    if (detailPinned) setDetailOpen(true);
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
  const openStreamDetail = (nextSelection: RoundSelection) => {
    onSelect(nextSelection);
    openDetail();
  };

  return (
    <Page flush className="flex flex-col">
      <div className="grid min-h-24 grid-cols-[minmax(220px,1fr)_minmax(420px,560px)_auto] items-center gap-5 border-b bg-background/60 px-8 py-4">
        <div className="min-w-0 space-y-2">
          <p className="truncate font-mono text-xs text-muted-foreground">{vm.run.id} / {displayStatus(t, vm.round.trigger)}</p>
          <div className="flex items-center gap-3">
            <h1 className="truncate text-2xl font-semibold tracking-tight">{vm.round.id}</h1>
            <StatusBadge value={vm.round.status} label={displayStatus(t, vm.round.status)} />
            <StatusBadge value={vm.round.outcome} label={displayStatus(t, vm.round.outcome)} />
          </div>
        </div>
        <div className="grid min-w-0 grid-cols-3 gap-3">
          <Metric label={t('roundDetail.trigger')} value={displayStatus(t, vm.round.trigger)} compact />
          <Metric label={t('roundDetail.repairLoopsUsed')} value={vm.round.repairLoopsUsed} compact />
          <Metric label={t('common.currentNode')} value={vm.round.currentNode ?? vm.run.currentNode ?? '-'} compact />
        </div>
        <div className="flex shrink-0 gap-2">
          <Button variant="outline" onClick={openDetail}>{t('roundDetail.openDetail')}</Button>
          <Button variant="outline">{t('roundDetail.exportLog')}</Button>
          <Button>{t('common.continueRun')}</Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button variant="outline" size="icon"><MoreVertical /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuItem disabled>{t('roundDetail.moreActions')}</DropdownMenuItem></DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-5">
        <div className="grid min-h-[620px] min-w-0 grid-rows-[minmax(300px,0.9fr)_minmax(280px,1fr)] gap-5">
          <AppCard className="flex min-h-0 min-w-0 flex-col overflow-hidden py-0">
            <CardHeader className="flex-row items-center justify-between border-b px-5 py-3">
              <CardTitle>{t('roundDetail.graph')}</CardTitle>
              <div className="flex flex-wrap gap-2 text-xs text-muted-foreground"><Legend tone="success" label={t('roundDetail.success')} /><Legend tone="running" label={t('roundDetail.running')} /><Legend tone="pending" label={t('roundDetail.pending')} /><Legend tone="artifact" label={t('roundDetail.hasArtifacts')} /><Legend tone="attachment" label={t('roundDetail.hasAttachments')} /></div>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 px-4 py-4"><GraphView graph={vm.graph} variant="actual" selectedNodeId={selectedNodeId} onNodeSelect={selectGraphNode} onNodeOpenDetail={openGraphNodeDetail} onNodeOpenSession={openGraphSession} /></CardContent>
          </AppCard>
          <AppCard className="flex min-h-0 min-w-0 flex-col overflow-hidden py-0">
            <CardHeader className="border-b px-4 py-2">
              <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as RoundTab)}>
                <div className="flex flex-wrap items-center gap-3">
                  <TabsList className="h-9">
                    <TabsTrigger value="requirement" className="h-7 px-3">{t('roundDetail.requirement')}</TabsTrigger>
                    <TabsTrigger value="log" className="h-7 px-3">{t('roundDetail.log')}</TabsTrigger>
                    {availableTabs.includes('artifacts') ? <TabsTrigger value="artifacts" className="h-7 px-3">{t('roundDetail.artifactTab', { count: streamGroups.artifacts.length })}</TabsTrigger> : null}
                    {availableTabs.includes('attachments') ? <TabsTrigger value="attachments" className="h-7 px-3">{t('roundDetail.attachmentTab', { count: streamGroups.attachments.length })}</TabsTrigger> : null}
                  </TabsList>
                  <span className="min-w-0 truncate font-mono text-xs text-muted-foreground">{selectedNodeId ? t('roundDetail.selectedNode', { node: selectedNodeId }) : t('roundDetail.roundContext')}</span>
                </div>
                <TabsContent value="requirement" className="m-0 min-h-0" />
                <TabsContent value="log" className="m-0 min-h-0" />
                <TabsContent value="artifacts" className="m-0 min-h-0" />
                <TabsContent value="attachments" className="m-0 min-h-0" />
              </Tabs>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 px-0 py-0"><ScrollArea className="h-full"><div className="space-y-2 p-3">{tabItems(activeTab, streamGroups).map((item) => <StreamItem item={item} key={item.id} onOpenDetail={openStreamDetail} />)}{tabItems(activeTab, streamGroups).length === 0 ? <EmptyState>{t('common.empty')}</EmptyState> : null}</div></ScrollArea></CardContent>
          </AppCard>
        </div>
        <RoundDetailSheet
          content={vm.detail}
          emptyLabel={t('common.empty')}
          open={detailOpen}
          pinned={detailPinned}
          onOpenChange={(open) => { if (open) setDetailOpen(true); else closeDetail(); }}
          onPinnedChange={setDetailPinned}
        />
      </div>
    </Page>
  );
}

function RoundDetailSheet({ content, emptyLabel, open, pinned, onOpenChange, onPinnedChange }: { content: RoundDetailVm['detail']; emptyLabel: string; open: boolean; pinned: boolean; onOpenChange: (open: boolean) => void; onPinnedChange: (pinned: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className="w-[520px] max-w-[calc(100vw-2rem)] gap-0 overflow-hidden p-0 sm:max-w-[520px]"
        closeLabel={t('common.close')}
        onInteractOutside={(event) => {
          if (pinned) event.preventDefault();
        }}
        showOverlay={false}
      >
        <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
          <div className="flex min-w-0 items-center justify-between gap-3 pr-8">
            <div className="min-w-0">
              <SheetTitle className="truncate text-base">{t('common.detail')}</SheetTitle>
              <SheetDescription className="sr-only">{t('roundDetail.detailDrawerDescription')}</SheetDescription>
            </div>
            {content ? <span className="shrink-0 font-mono text-xs text-muted-foreground">{content.kind}</span> : null}
          </div>
          <Button className="w-fit" variant="outline" size="sm" onClick={() => onPinnedChange(!pinned)}>
            {pinned ? <PinOff className="size-4" /> : <Pin className="size-4" />}
            {pinned ? t('roundDetail.unpinDetail') : t('roundDetail.pinDetail')}
          </Button>
        </SheetHeader>
        <div className="min-h-0 flex-1">
          <DetailViewerContent content={content} emptyLabel={emptyLabel} />
        </div>
      </SheetContent>
    </Sheet>
  );
}

function Legend({ tone, label }: { tone: string; label: string }) {
  return <span className="inline-flex items-center gap-1.5"><i className={cn('size-2 rounded-full', tone === 'success' && 'bg-gold-success', tone === 'running' && 'bg-gold-running', tone === 'pending' && 'bg-muted-foreground', tone === 'artifact' && 'bg-gold-warning', tone === 'attachment' && 'bg-slate-400')} />{label}</span>;
}

function StreamItem({ item, onOpenDetail }: { item: StreamItemVm; onOpenDetail: (selection: RoundSelection) => void }) {
  const target = streamTarget(item);
  return (
    <Button variant="outline" className={cn('h-auto w-full flex-col items-stretch justify-start gap-2 p-3 text-left', toneSurfaceClass(item.tone), !target && 'opacity-60')} onClick={() => target && onOpenDetail(target)} disabled={!target}>
      <span className="flex items-start justify-between gap-3">
        <strong className="line-clamp-1 text-sm">{item.title}</strong>
        <Badge variant="secondary" className="shrink-0 font-mono text-[10px]">{item.kind}</Badge>
      </span>
      <p className="line-clamp-4 whitespace-pre-wrap text-sm leading-6 text-muted-foreground">{item.content}</p>
    </Button>
  );
}

function groupStream(items: StreamItemVm[]) {
  return {
    requirement: items.filter((item) => item.kind === 'requirement' || item.id === 'requirement'),
    log: items.filter((item) => item.kind === 'round' || item.kind === 'event' || item.kind === 'log' || item.kind === 'node'),
    artifacts: items.filter((item) => item.kind === 'artifact'),
    attachments: items.filter((item) => item.kind === 'attachment'),
  };
}

function tabItems(tab: RoundTab, groups: ReturnType<typeof groupStream>) {
  if (tab === 'artifacts') return groups.artifacts;
  if (tab === 'attachments') return groups.attachments;
  if (tab === 'log') return groups.log;
  return groups.requirement;
}

function canonicalNodeId(node: GraphNodeVm) {
  return node.nodeId ?? node.id;
}

function selectedNodeIdFromSelection(selection: RoundSelection) {
  if (selection.kind === 'node' || selection.kind === 'artifact' || selection.kind === 'attachment' || selection.kind === 'worker-ref') {
    return selection.nodeId;
  }
  return undefined;
}

function streamTarget(item: StreamItemVm): RoundSelection | null {
  if (item.kind === 'artifact' && item.nodeId && item.attemptId && item.name) return { kind: 'artifact', nodeId: item.nodeId, attemptId: item.attemptId, name: item.name };
  if (item.kind === 'attachment' && item.nodeId && item.attemptId && item.name) return { kind: 'attachment', nodeId: item.nodeId, attemptId: item.attemptId, name: item.name };
  if (item.kind === 'node' && item.nodeId) return { kind: 'node', nodeId: item.nodeId };
  if (item.kind === 'event') return { kind: 'event', id: item.id };
  if (item.kind === 'log') return { kind: 'log', id: item.id };
  return null;
}
