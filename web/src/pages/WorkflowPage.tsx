import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { ChevronDown, ChevronRight, RefreshCw } from 'lucide-react';
import type { GraphVm, RoundSummaryVm, RunGroupVm, TaskPage, TaskRowVm, WorkflowVm } from '../types';
import { displayPolicy, displayStatus } from '../i18n';
import { GraphView } from '../components/GraphView';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, MetricsBar, Page, PageHeader } from '@/components/PageScaffold';
import { RequirementDetailSheet, RequirementTeaser, fullRequirementText } from '@/components/RequirementDisclosure';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { normalizeTone } from '@/lib/status';
import { formatCurrentNode } from '@/lib/nodes';

interface WorkflowPageProps {
  vm: WorkflowVm | null;
  busy: boolean;
  refreshing: boolean;
  breadcrumbs?: ReactNode;
  onNavigate: (page: TaskPage) => void;
  onRefresh: () => void;
  onStartRun: (taskId: string) => void;
  onContinueRun: (taskId: string, runId: string) => void;
  onKillRun: (taskId: string, runId: string) => void;
}

type StatusFilter = 'all' | 'running' | 'paused' | 'completed' | 'failed' | 'resumable';
type SortDir = 'asc' | 'desc';
type WorkflowDrawerMode = 'view' | 'create' | 'edit' | 'repair';

type WorkflowLifecycle = {
  status: 'valid' | 'invalid' | 'missing-workflow';
  primaryMode: WorkflowDrawerMode;
  primaryLabelKey: string;
};

const pageSizes = [5, 10, 20];
const collapsedRunRowMinHeight = 64;
const historyRowGridClass = 'grid gap-3 md:grid-cols-[minmax(180px,0.9fr)_minmax(112px,0.36fr)_minmax(220px,1fr)_minmax(180px,0.72fr)_minmax(72px,auto)] md:items-center';

function historyBodyMinHeightFor(pageSize: number) {
  return Math.max(320, pageSize * collapsedRunRowMinHeight);
}

export function WorkflowPage({ vm, busy, refreshing, breadcrumbs, onNavigate, onRefresh, onStartRun }: WorkflowPageProps) {
  const { t } = useTranslation();
  const [requirementOpen, setRequirementOpen] = useState(false);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [sortDir, setSortDir] = useState<SortDir>('desc');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(5);
  const [workflowDrawerMode, setWorkflowDrawerMode] = useState<WorkflowDrawerMode | null>(null);
  const [runExpansion, setRunExpansion] = useState<Record<string, boolean>>({});

  const toggleRun = (runId: string, expanded: boolean) => {
    setRunExpansion((current) => ({ ...current, [runId]: !expanded }));
  };

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;
  const latestRun = vm.runs[0]?.run;
  const filteredRuns = vm.runs.filter((group) => matchesRunFilter(group, statusFilter));
  const sortedRuns = [...filteredRuns].sort((left, right) => left.run.id.localeCompare(right.run.id, undefined, { numeric: true }) * (sortDir === 'asc' ? 1 : -1));
  const pageCount = Math.max(1, Math.ceil(sortedRuns.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pagedRuns = sortedRuns.slice(safePageIndex * pageSize, safePageIndex * pageSize + pageSize);
  const emptyMessage = vm.runs.length === 0 ? t('workflow.noRuns') : t('workflow.noRunsForFilter');
  const requirement = fullRequirementText(vm.task.requirement, vm.task.requirementPreview || vm.task.description, t('common.empty'));
  const workflowLifecycle = workflowLifecycleFor(vm.task);
  const workflowDrawerOpen = workflowDrawerMode !== null;
  const historyBodyMinHeight = historyBodyMinHeightFor(pageSize);

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        className="px-5 py-4 xl:px-6"
        breadcrumbs={breadcrumbs}
        title={vm.task.title}
        subtitle={(
          <div className="flex min-w-0 items-center gap-2 overflow-hidden text-xs">
            <span className="shrink-0 font-medium text-foreground">{t('common.requirement')}</span>
            <RequirementTeaser compact className="flex-1" text={requirement} detailLabel={t('common.viewFullRequirement')} onOpenDetail={() => setRequirementOpen(true)} />
          </div>
        )}
        actions={(
          <Button variant="outline" disabled={busy || refreshing} onClick={onRefresh}>
            <RefreshCw className={cn(refreshing && 'animate-spin')} />
            {t('common.refresh')}
          </Button>
        )}
        metrics={(
          <MetricsBar className="lg:grid-cols-4 xl:grid-cols-4">
            <Metric label={t('workflow.taskId')} value={vm.task.id} compact />
            <WorkflowMetricCard lifecycle={workflowLifecycle} onOpen={setWorkflowDrawerMode} t={t} />
            <Metric label={t('taskList.latestRun')} value={latestRun?.id ?? '-'} compact />
            <Metric label={t('common.outcome')} value={<StatusBadge value={vm.task.displayStatus} label={displayStatus(t, vm.task.displayStatus)} />} compact />
          </MetricsBar>
        )}
      />
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-3 p-3 xl:p-4">
          <AppCard className="flex min-h-0 flex-col gap-0 py-0">
            <CardHeader className="flex flex-row items-center justify-between gap-3 border-b bg-muted/10 px-4 py-2.5 !pb-2.5">
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-3">
                <CardTitle className="shrink-0">{t('workflow.historyTitle')}</CardTitle>
                <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-muted-foreground">
                  <span className="shrink-0">{t('common.filterByStatus')}</span>
                  <Select value={statusFilter} onValueChange={(value) => { setStatusFilter(value as StatusFilter); setPageIndex(0); }}>
                    <SelectTrigger className="h-9 w-32"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {(['all', 'running', 'paused', 'completed', 'failed', 'resumable'] as StatusFilter[]).map((value) => <SelectItem value={value} key={value}>{value === 'all' ? t('common.all') : displayStatus(t, value)}</SelectItem>)}
                    </SelectContent>
                  </Select>
                  <Button variant="outline" size="sm" onClick={() => setSortDir((value) => value === 'asc' ? 'desc' : 'asc')}>{t('common.sort')} {sortDir === 'asc' ? '↑' : '↓'}</Button>
                </div>
              </div>
              <Button className="shrink-0" disabled={busy || !vm.task.workflowValid} onClick={() => onStartRun(vm.task.id)}>{t('common.startRun')}</Button>
            </CardHeader>
            <CardContent className="flex min-h-0 flex-1 flex-col px-3 py-2">
              <div className="min-h-0 flex-1" style={{ minHeight: historyBodyMinHeight }}>
                {pagedRuns.length ? (
                  <div className="overflow-hidden rounded-xl border bg-card/55 shadow-sm shadow-background/10">
                    <div className={cn(historyRowGridClass, 'border-b bg-muted/20 px-4 py-2 text-[11px] font-semibold uppercase tracking-[0.16em] text-muted-foreground')}>
                      <span>{t('workflow.idGroup')}</span>
                      <span>{t('common.status')}</span>
                      <span>{t('workflow.historyProgress')}</span>
                      <span>{t('workflow.historyContext')}</span>
                      <span className="text-right">{t('common.action')}</span>
                    </div>
                    <div className="divide-y divide-border/80">
                      {pagedRuns.map((group) => {
                        const expanded = runExpansion[group.run.id] ?? false;
                        return (
                          <RunGroupRow
                            key={group.run.id}
                            group={group}
                            graph={vm.graph}
                            expanded={expanded}
                            onToggle={() => toggleRun(group.run.id, expanded)}
                            onOpenRound={(roundId) => onNavigate({ kind: 'round-detail', taskId: vm.task.id, runId: group.run.id, roundId })}
                            t={t}
                          />
                        );
                      })}
                    </div>
                  </div>
                ) : <EmptyState className="h-full min-h-full">{emptyMessage}</EmptyState>}
              </div>
              <div className="mt-2 flex flex-wrap items-center justify-between gap-3 text-sm text-muted-foreground">
                <span>{t('workflow.groupsRange', { start: sortedRuns.length ? safePageIndex * pageSize + 1 : 0, end: Math.min(sortedRuns.length, (safePageIndex + 1) * pageSize), total: sortedRuns.length })}</span>
                <div className="flex items-center gap-2">
                  <span>{t('common.pageSize')}</span>
                  <Select value={String(pageSize)} onValueChange={(value) => { setPageSize(Number(value)); setPageIndex(0); }}>
                    <SelectTrigger className="w-20"><SelectValue /></SelectTrigger>
                    <SelectContent position="popper" side="top" align="end" sideOffset={6}>
                      {pageSizes.map((value) => <SelectItem value={String(value)} key={value}>{value}</SelectItem>)}
                    </SelectContent>
                  </Select>
                  <Button variant="outline" size="sm" disabled={safePageIndex === 0} onClick={() => setPageIndex((value) => Math.max(0, value - 1))}>{t('common.previousPage')}</Button>
                  <Button variant="outline" size="sm" disabled={safePageIndex >= pageCount - 1} onClick={() => setPageIndex((value) => Math.min(pageCount - 1, value + 1))}>{t('common.nextPage')}</Button>
                </div>
              </div>
            </CardContent>
          </AppCard>
        </div>
      </ScrollArea>
      <RequirementDetailSheet
        open={requirementOpen}
        title={t('common.fullRequirement')}
        description={t('common.fullRequirementDescription')}
        requirement={requirement}
        closeLabel={t('common.close')}
        onOpenChange={setRequirementOpen}
      />
      <Sheet modal={false} open={workflowDrawerOpen} onOpenChange={(open) => !open && setWorkflowDrawerMode(null)}>
        <SheetContent className="w-[720px] max-w-[calc(100vw-2rem)] gap-0 overflow-hidden p-0 sm:max-w-[720px]" closeLabel={t('common.close')} showOverlay={false}>
          <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
            <SheetDescription className="sr-only">{t('workflow.drawerDescription')}</SheetDescription>
            <div className="flex min-w-0 flex-wrap items-center gap-3 pr-8">
              <SheetTitle className="break-words text-xl">{workflowDrawerMode ? t(`workflow.${workflowDrawerMode}WorkflowTitle`) : t('common.workflow')}</SheetTitle>
              <StatusBadge value={workflowLifecycle.status} label={displayStatus(t, workflowLifecycle.status)} />
            </div>
            {vm.task.workflowError ? <p className="text-sm text-muted-foreground">{vm.task.workflowError}</p> : null}
          </SheetHeader>
          <ScrollArea className="min-h-0 flex-1">
            <div className="space-y-4 p-5">
              {vm.control && vm.task.workflowExists ? (
                <div className="flex flex-wrap items-center gap-2 rounded-xl border border-primary/20 bg-muted/20 p-2">
                  <ControlPill label={t('workflow.maxRepairLoops')} value={vm.control.maxRepairLoops} />
                  <ControlPill label={t('workflow.maxAcceptanceLoops')} value={vm.control.maxAcceptanceLoops} />
                  <ControlPill label={t('workflow.onAcceptanceFailure')} value={displayPolicy(t, vm.control.onAcceptanceFailure)} />
                </div>
              ) : null}
              {vm.task.workflowExists ? <GraphView graph={vm.graph} variant="workflow" /> : <EmptyState>{t('workflow.noWorkflow')}</EmptyState>}
            </div>
          </ScrollArea>
        </SheetContent>
      </Sheet>
    </Page>
  );
}

function WorkflowMetricCard({ lifecycle, onOpen, t }: { lifecycle: WorkflowLifecycle; onOpen: (mode: WorkflowDrawerMode) => void; t: TFunction }) {
  return (
    <AppCard className="h-full gap-2 border-border/45 bg-card/45 py-3 shadow-none">
      <CardContent className="flex h-full flex-col justify-between gap-1 px-3">
        <span className="block text-xs uppercase tracking-[0.16em] text-muted-foreground">{t('common.workflow')}</span>
        <div className="flex min-h-8 items-center justify-between gap-3">
          <StatusBadge value={lifecycle.status} label={displayStatus(t, lifecycle.status)} />
          <Button size="sm" className="h-8 px-3" onClick={() => onOpen(lifecycle.primaryMode)}>{t(lifecycle.primaryLabelKey)}</Button>
        </div>
      </CardContent>
    </AppCard>
  );
}

function workflowLifecycleFor(task: TaskRowVm): WorkflowLifecycle {
  if (!task.workflowExists) {
    return { status: 'missing-workflow', primaryMode: 'create', primaryLabelKey: 'workflow.createWorkflow' };
  }
  if (!task.workflowValid) {
    return { status: 'invalid', primaryMode: 'repair', primaryLabelKey: 'workflow.repairWorkflow' };
  }
  return { status: 'valid', primaryMode: 'view', primaryLabelKey: 'workflow.viewWorkflow' };
}

function ControlPill({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex min-h-9 min-w-[176px] flex-1 items-center justify-between gap-3 rounded-lg border bg-card/55 px-3 py-1.5">
      <span className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">{label}</span>
      <strong className="shrink-0 text-sm text-foreground">{value}</strong>
    </div>
  );
}

function RunGroupRow({ group, graph, expanded, onToggle, onOpenRound, t }: {
  group: RunGroupVm;
  graph: GraphVm;
  expanded: boolean;
  onToggle: () => void;
  onOpenRound: (roundId: string) => void;
  t: TFunction;
}) {
  const rounds = useMemo(() => [...group.rounds].sort((left, right) => right.index - left.index), [group.rounds]);
  const regionId = `run-rounds-${group.run.id}`;
  const currentNode = formatCurrentNode(t, graph, group.run.currentNode);
  const pauseReason = group.run.pauseReason ? displayStatus(t, group.run.pauseReason) : null;

  return (
    <section className={cn('bg-background/20 transition-colors', expanded && 'bg-muted/15')}>
      <div
        className={cn(historyRowGridClass, 'min-h-16 cursor-pointer border-l-2 border-transparent px-4 py-2.5 transition-colors hover:bg-muted/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50', expanded && 'border-l-border bg-card/65')}
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={t(expanded ? 'workflow.collapseRun' : 'workflow.expandRun', { runId: group.run.id })}
        onClick={onToggle}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            onToggle();
          }
        }}
      >
        <div className="flex min-w-0 items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="h-8 w-8 shrink-0 rounded-lg border-border/70 bg-background/70 text-muted-foreground shadow-sm"
            aria-hidden="true"
            tabIndex={-1}
            onClick={(event) => {
              event.stopPropagation();
              onToggle();
            }}
          >
            {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          </Button>
          <strong className="min-w-0 truncate text-base text-foreground">{group.run.id}</strong>
        </div>
        <div className="min-w-0"><StatusBadge value={summaryStatusValue(group.run.status, group.run.outcome)} label={displayStatus(t, summaryStatusValue(group.run.status, group.run.outcome))} /></div>
        <HistoryCell label={t('workflow.currentRound')} value={group.run.currentRound ?? '-'} />
        <HistoryCell label={pauseReason ? t('workflow.pauseReason') : t('workflow.currentNode')} value={pauseReason ?? currentNode} title={pauseReason ?? currentNode} />
        <span className="hidden text-right text-sm text-muted-foreground md:block">—</span>
      </div>
      {expanded ? <RoundList id={regionId} runId={group.run.id} graph={graph} rounds={rounds} onOpenRound={onOpenRound} t={t} /> : null}
    </section>
  );
}

function HistoryCell({ label, value, title, className }: { label: ReactNode; value: ReactNode; title?: string | null; className?: string }) {
  return (
    <div className={cn('min-w-0 space-y-0.5', className)} title={title ?? undefined}>
      <span className="block truncate text-[11px] font-medium text-muted-foreground/70">{label}</span>
      <strong className="block min-w-0 truncate text-sm font-medium text-foreground">{value}</strong>
    </div>
  );
}

function RoundList({ id, runId, graph, rounds, onOpenRound, t }: {
  id: string;
  runId: string;
  graph: GraphVm;
  rounds: RoundSummaryVm[];
  onOpenRound: (roundId: string) => void;
  t: TFunction;
}) {
  if (!rounds.length) return <EmptyState className="mx-4 mb-4">{t('common.empty')}</EmptyState>;
  return (
    <div id={id} className="border-t bg-muted/20 px-4 py-3">
      <div className="ml-8 space-y-2 border-l-2 border-border/70 pl-5">
        {rounds.map((round) => <RoundRow key={round.id} runId={runId} graph={graph} round={round} onOpen={() => onOpenRound(round.id)} t={t} />)}
      </div>
    </div>
  );
}

function RoundRow({ runId, graph, round, onOpen, t }: { runId: string; graph: GraphVm; round: RoundSummaryVm; onOpen: () => void; t: TFunction }) {
  const currentNode = formatCurrentNode(t, graph, round.currentNode);

  return (
    <div className={cn(historyRowGridClass, 'relative min-h-[58px] rounded-lg border border-border/55 bg-background/55 px-3 py-2.5 shadow-sm transition-colors hover:bg-card/75')}>
      <span className={cn('absolute -left-[27px] top-1/2 h-3 w-3 -translate-y-1/2 rounded-full border-2 ring-4 ring-muted/20', timelineDotClass(round.outcome ?? round.status))} />
      <div className="flex min-w-0 items-center gap-2 pl-1">
        <strong className="truncate text-sm text-foreground">{round.id}</strong>
        <Badge variant="secondary" className="text-[11px]">#{round.index}</Badge>
      </div>
      <div className="min-w-0"><StatusBadge value={summaryStatusValue(round.status, round.outcome)} label={displayStatus(t, summaryStatusValue(round.status, round.outcome))} /></div>
      <HistoryCell label={t('workflow.currentNode')} value={currentNode} title={currentNode} />
      <HistoryCell label={t('workflow.currentRound')} value={`#${round.index}`} />
      <Button variant="outline" size="sm" className="justify-self-start md:justify-self-end" onClick={onOpen} aria-label={t('workflow.openRoundA11y', { runId, roundId: round.id })}>{t('workflow.openRound')}</Button>
    </div>
  );
}

function summaryStatusValue(status?: string | null, outcome?: string | null) {
  return outcome ?? status ?? null;
}

function timelineDotClass(value?: string | null) {
  const tone = normalizeTone(value);
  if (tone === 'running') return 'border-gold-running bg-gold-running';
  if (tone === 'success') return 'border-gold-success bg-gold-success';
  if (tone === 'warning') return 'border-gold-warning bg-gold-warning';
  if (tone === 'danger') return 'border-gold-danger bg-gold-danger';
  return 'border-border bg-muted-foreground';
}

function matchesRunFilter(group: RunGroupVm, filter: StatusFilter) {
  if (filter === 'all') return true;
  if (filter === 'failed') return group.run.outcome === 'failure' || group.rounds.some((round) => round.outcome === 'failure');
  if (filter === 'resumable') return group.run.resumable;
  return group.run.status === filter || group.rounds.some((round) => round.status === filter);
}
