import { useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { RoundSummaryVm, RunGroupVm, TaskPage, WorkflowVm } from '../types';
import { displayPolicy, displayStatus } from '../i18n';
import { GraphView } from '../components/GraphView';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Metric, MetricsBar, ModuleBar, Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { normalizeTone } from '@/lib/status';

interface WorkflowPageProps {
  vm: WorkflowVm | null;
  busy: boolean;
  onNavigate: (page: TaskPage) => void;
  onStartRun: (taskId: string) => void;
  onContinueRun: (taskId: string, runId: string) => void;
  onKillRun: (taskId: string, runId: string) => void;
}

type StatusFilter = 'all' | 'running' | 'paused' | 'completed' | 'failed' | 'resumable';
type SortDir = 'asc' | 'desc';
const pageSizes = [5, 10, 20];

export function WorkflowPage({ vm, busy, onNavigate, onStartRun, onContinueRun, onKillRun }: WorkflowPageProps) {
  const { t } = useTranslation();
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [sortDir, setSortDir] = useState<SortDir>('desc');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(5);
  const [expandedRunIds, setExpandedRunIds] = useState<Set<string>>(new Set());
  const [collapsedRunIds, setCollapsedRunIds] = useState<Set<string>>(new Set());

  const toggleRun = (runId: string, expanded: boolean) => {
    setExpandedRunIds((current) => {
      const next = new Set(current);
      if (expanded) next.delete(runId);
      else next.add(runId);
      return next;
    });
    setCollapsedRunIds((current) => {
      const next = new Set(current);
      if (expanded) next.add(runId);
      else next.delete(runId);
      return next;
    });
  };

  if (!vm) return <Page><EmptyState>{t('common.loading')}</EmptyState></Page>;
  const activeRun = vm.runs.find((group) => group.run.status === 'running' || group.run.status === 'paused')?.run ?? vm.runs[0]?.run;
  const latestRunId = vm.runs[0]?.run.id;
  const filteredRuns = vm.runs.filter((group) => matchesRunFilter(group, statusFilter));
  const sortedRuns = [...filteredRuns].sort((left, right) => left.run.id.localeCompare(right.run.id, undefined, { numeric: true }) * (sortDir === 'asc' ? 1 : -1));
  const pageCount = Math.max(1, Math.ceil(sortedRuns.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pagedRuns = sortedRuns.slice(safePageIndex * pageSize, safePageIndex * pageSize + pageSize);
  const emptyMessage = vm.runs.length === 0 ? t('workflow.noRuns') : t('workflow.noRunsForFilter');

  return (
    <Page flush className="flex flex-col">
      <ModuleBar
        title={t('workflow.moduleTitle')}
        tabs={<Tabs value="runs"><TabsList><TabsTrigger value="overview">{t('workflow.overview')}</TabsTrigger><TabsTrigger value="runs">{t('workflow.runs')}</TabsTrigger><TabsTrigger value="nodes">{t('workflow.nodes')}</TabsTrigger><TabsTrigger value="artifacts">{t('workflow.artifacts')}</TabsTrigger></TabsList></Tabs>}
        actions={<><Button disabled={busy || !vm.task.workflowValid} onClick={() => onStartRun(vm.task.id)}>{t('common.startRun')}</Button><Button variant="outline" disabled={busy || !activeRun?.resumable} onClick={() => activeRun && onContinueRun(vm.task.id, activeRun.id)}>{t('common.continueRun')}</Button></>}
      />
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-5 p-6">
          <PageHeader
            eyebrow={vm.task.id}
            title={vm.task.title}
            subtitle={<>{t('workflow.requirementSummary', { summary: vm.task.requirementPreview || vm.task.description || '-' })}{activeRun?.currentNode ? <span className="ml-2 text-primary">{t('workflow.currentStatus', { node: activeRun.currentNode })}</span> : null}</>}
            actions={<><Button variant="outline" disabled>{t('workflow.viewRequirement')}</Button>{activeRun && (activeRun.status === 'running' || activeRun.status === 'paused') ? <Button variant="destructive" disabled={busy} onClick={() => onKillRun(vm.task.id, activeRun.id)}>{t('common.stopRun')}</Button> : null}</>}
          />
          <MetricsBar>
            <Metric label={t('workflow.taskId')} value={vm.task.id} />
            <Metric label={t('workflow.workflowStatus')} value={vm.task.workflowValid ? displayStatus(t, 'valid') : vm.task.workflowExists ? displayStatus(t, 'invalid') : displayStatus(t, 'missing-workflow')} />
            <Metric label={t('workflow.activeRun')} value={activeRun?.id ?? '-'} />
            <Metric label={t('common.outcome')} value={displayStatus(t, activeRun?.outcome ?? activeRun?.status ?? vm.task.displayStatus)} />
            <Metric label={t('common.artifacts')} value={vm.task.artifactCount} />
          </MetricsBar>
          <AppCard className="gap-0 py-0">
            <CardHeader className="border-b px-5 py-4"><CardTitle>{t('workflow.blueprintTitle')}</CardTitle></CardHeader>
            <CardContent className="space-y-3 p-4">
              {vm.control ? (
                <div className="flex flex-wrap items-center gap-2 rounded-xl border border-primary/20 bg-muted/20 p-2">
                  <ControlPill label={t('workflow.maxRepairLoops')} value={vm.control.maxRepairLoops} />
                  <ControlPill label={t('workflow.maxAcceptanceLoops')} value={vm.control.maxAcceptanceLoops} />
                  <ControlPill label={t('workflow.onAcceptanceFailure')} value={displayPolicy(t, vm.control.onAcceptanceFailure)} />
                </div>
              ) : null}
              <GraphView graph={vm.graph} variant="workflow" />
            </CardContent>
          </AppCard>
          <AppCard className="py-0">
            <CardHeader className="flex-row items-center justify-between gap-3 border-b py-5">
              <CardTitle>{t('workflow.historyTitle')}</CardTitle>
              <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
                <span>{t('common.filterByStatus')}</span>
                <Select value={statusFilter} onValueChange={(value) => { setStatusFilter(value as StatusFilter); setPageIndex(0); }}>
                  <SelectTrigger className="w-36"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {(['all', 'running', 'paused', 'completed', 'failed', 'resumable'] as StatusFilter[]).map((value) => <SelectItem value={value} key={value}>{value === 'all' ? t('common.all') : displayStatus(t, value)}</SelectItem>)}
                  </SelectContent>
                </Select>
                <Button variant="outline" size="sm" onClick={() => setSortDir((value) => value === 'asc' ? 'desc' : 'asc')}>{t('common.sort')} {sortDir === 'asc' ? '↑' : '↓'}</Button>
              </div>
            </CardHeader>
            <CardContent className="px-5 py-5">
              {pagedRuns.length ? (
                <div className="overflow-hidden rounded-xl border bg-card/35">
                  <div className="divide-y divide-border/80">
                    {pagedRuns.map((group) => {
                      const defaultExpanded = group.run.id === latestRunId || group.run.resumable || group.run.status === 'running' || group.run.status === 'paused';
                      const expanded = expandedRunIds.has(group.run.id) || (defaultExpanded && !collapsedRunIds.has(group.run.id));
                      return (
                        <RunGroupRow
                          key={group.run.id}
                          group={group}
                          busy={busy}
                          expanded={expanded}
                          highlighted={defaultExpanded}
                          onToggle={() => toggleRun(group.run.id, expanded)}
                          onContinue={() => onContinueRun(vm.task.id, group.run.id)}
                          onKill={() => onKillRun(vm.task.id, group.run.id)}
                          onOpenRound={(roundId) => onNavigate({ kind: 'round-detail', taskId: vm.task.id, runId: group.run.id, roundId })}
                          t={t}
                        />
                      );
                    })}
                  </div>
                </div>
              ) : <EmptyState>{emptyMessage}</EmptyState>}
              <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm text-muted-foreground">
                <span>{t('workflow.groupsRange', { start: sortedRuns.length ? safePageIndex * pageSize + 1 : 0, end: Math.min(sortedRuns.length, (safePageIndex + 1) * pageSize), total: sortedRuns.length })}</span>
                <div className="flex items-center gap-2">
                  <span>{t('common.pageSize')}</span>
                  <Select value={String(pageSize)} onValueChange={(value) => { setPageSize(Number(value)); setPageIndex(0); }}>
                    <SelectTrigger className="w-20"><SelectValue /></SelectTrigger>
                    <SelectContent>{pageSizes.map((value) => <SelectItem value={String(value)} key={value}>{value}</SelectItem>)}</SelectContent>
                  </Select>
                  <Button variant="outline" size="sm" disabled={safePageIndex === 0} onClick={() => setPageIndex((value) => Math.max(0, value - 1))}>{t('common.previousPage')}</Button>
                  <Button variant="outline" size="sm" disabled={safePageIndex >= pageCount - 1} onClick={() => setPageIndex((value) => Math.min(pageCount - 1, value + 1))}>{t('common.nextPage')}</Button>
                </div>
              </div>
            </CardContent>
          </AppCard>
        </div>
      </ScrollArea>
    </Page>
  );
}

function ControlPill({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex min-h-9 min-w-[176px] flex-1 items-center justify-between gap-3 rounded-lg border bg-card/55 px-3 py-1.5">
      <span className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">{label}</span>
      <strong className="shrink-0 text-sm text-foreground">{value}</strong>
    </div>
  );
}

function RunGroupRow({ group, busy, expanded, highlighted, onToggle, onContinue, onKill, onOpenRound, t }: {
  group: RunGroupVm;
  busy: boolean;
  expanded: boolean;
  highlighted: boolean;
  onToggle: () => void;
  onContinue: () => void;
  onKill: () => void;
  onOpenRound: (roundId: string) => void;
  t: TFunction;
}) {
  const rounds = useMemo(() => [...group.rounds].sort((left, right) => left.index - right.index), [group.rounds]);
  const regionId = `run-rounds-${group.run.id}`;

  return (
    <section className={cn('bg-background/20', highlighted && 'bg-primary/[0.035]')}>
      <div className={cn('grid gap-3 px-4 py-3 xl:grid-cols-[minmax(220px,0.9fr)_minmax(260px,0.8fr)_auto]', highlighted && 'border-l-2 border-l-primary/60 pl-[14px]')}>
        <div className="flex min-w-0 items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0 text-muted-foreground"
            aria-expanded={expanded}
            aria-controls={regionId}
            aria-label={t(expanded ? 'workflow.collapseRun' : 'workflow.expandRun', { runId: group.run.id })}
            onClick={onToggle}
          >
            {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          </Button>
          <strong className="truncate font-mono text-base text-foreground">{group.run.id}</strong>
          <StatusBadge value={group.run.status} label={displayStatus(t, group.run.status)} />
          <StatusBadge value={group.run.outcome} label={displayStatus(t, group.run.outcome)} />
          {group.run.resumable ? <StatusBadge value="resumable" label={t('workflow.resumable')} /> : null}
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-x-5 gap-y-1 text-sm text-muted-foreground">
          <InlineMeta label={t('workflow.currentRound')} value={group.run.currentRound ?? '-'} />
          {group.run.pauseReason ? <InlineMeta label={t('workflow.pauseReason')} value={displayStatus(t, group.run.pauseReason)} /> : null}
        </div>
        <div className="flex shrink-0 items-center justify-end gap-2">
          {group.run.resumable ? <Button variant="outline" size="sm" disabled={busy} onClick={onContinue}>{t('common.continueRun')}</Button> : null}
          {group.run.status === 'running' || group.run.status === 'paused' ? <Button variant="destructive" size="sm" disabled={busy} onClick={onKill}>{t('common.stopRun')}</Button> : null}
        </div>
      </div>
      {expanded ? <RoundList id={regionId} runId={group.run.id} rounds={rounds} onOpenRound={onOpenRound} t={t} /> : null}
    </section>
  );
}

function InlineMeta({ label, value }: { label: ReactNode; value: ReactNode }) {
  return <span className="min-w-0"><span className="text-muted-foreground/70">{label}</span><span className="mx-1 text-muted-foreground/40">/</span><strong className="font-medium text-foreground">{value}</strong></span>;
}

function RoundList({ id, runId, rounds, onOpenRound, t }: {
  id: string;
  runId: string;
  rounds: RoundSummaryVm[];
  onOpenRound: (roundId: string) => void;
  t: TFunction;
}) {
  if (!rounds.length) return <EmptyState className="mx-4 mb-4">{t('common.empty')}</EmptyState>;
  return (
    <div id={id} className="border-t bg-muted/[0.08] px-4 py-3">
      <div className="space-y-2 border-l border-border pl-4">
        {rounds.map((round) => <RoundRow key={round.id} runId={runId} round={round} onOpen={() => onOpenRound(round.id)} t={t} />)}
      </div>
    </div>
  );
}

function RoundRow({ runId, round, onOpen, t }: { runId: string; round: RoundSummaryVm; onOpen: () => void; t: TFunction }) {
  return (
    <div className="relative grid items-center gap-3 rounded-lg px-3 py-2 hover:bg-muted/25 xl:grid-cols-[minmax(220px,0.85fr)_minmax(180px,0.6fr)_auto]">
      <span className={cn('absolute -left-[21px] top-1/2 h-2.5 w-2.5 -translate-y-1/2 rounded-full border', timelineDotClass(round.outcome ?? round.status))} />
      <div className="flex min-w-0 items-center gap-2">
        <strong className="truncate font-mono text-sm text-foreground">{round.id}</strong>
        <Badge variant="secondary" className="font-mono text-[11px]">#{round.index}</Badge>
        <StatusBadge value={round.status} label={displayStatus(t, round.status)} />
        <StatusBadge value={round.outcome} label={displayStatus(t, round.outcome)} />
      </div>
      <div className="flex min-w-0 flex-wrap gap-x-5 gap-y-1 text-sm text-muted-foreground">
        <InlineMeta label={t('workflow.currentNode')} value={round.currentNode ?? '-'} />
      </div>
      <Button variant="outline" size="sm" className="justify-self-end" onClick={onOpen} aria-label={t('workflow.openRoundA11y', { runId, roundId: round.id })}>{t('workflow.openRound')}</Button>
    </div>
  );
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
