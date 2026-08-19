import { useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertTriangle, Ban, Bot, Clock3, Coins, FileCheck2, Gauge, ListChecks,
  RefreshCw, Settings, ShieldCheck, Sparkles, Square, Wrench,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  cancelPersonalAnalytics, cancelPersonalAnalyticsInsights, getPersonalAnalytics,
  queryPersonalAnalyticsReport, startPersonalAnalyticsInsights, subscribePersonalAnalyticsUpdates,
  syncPersonalAnalytics,
} from '@/api';
import type {
  AgentRegistryVm, PersonalAnalyticsRateMetricVm, PersonalAnalyticsReportVm,
  PersonalAnalyticsTaskSummaryVm,
} from '@/types';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { formatTokenCount } from '@/lib/format-token';
import { isPersonalAnalyticsActive, mergePersonalAnalyticsSnapshot } from '@/lib/personal-analytics-state';

interface PersonalAnalyticsPageProps {
  agentRegistry: AgentRegistryVm | null;
  onOpenAgentManagement: () => void;
}

export function PersonalAnalyticsPage({ agentRegistry, onOpenAgentManagement }: PersonalAnalyticsPageProps) {
  const { t, i18n } = useTranslation();
  const [snapshot, setSnapshot] = useState<Awaited<ReturnType<typeof getPersonalAnalytics>> | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedAgentType, setSelectedAgentType] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [rangePreset, setRangePreset] = useState<'all' | 'today' | 'last7' | 'last30' | 'custom'>('all');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [rangeQuerying, setRangeQuerying] = useState(false);
  const [insightSubmitting, setInsightSubmitting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const availableAgents = useMemo(
    () => agentRegistry?.agents.filter((agent) => agent.diagnostic?.available === true) ?? [],
    [agentRegistry],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    getPersonalAnalytics()
      .then((next) => { if (!disposed) setSnapshot((current) => mergePersonalAnalyticsSnapshot(current, next)); })
      .finally(() => { if (!disposed) setLoading(false); });
    void subscribePersonalAnalyticsUpdates((next) => {
      if (!disposed) setSnapshot((current) => mergePersonalAnalyticsSnapshot(current, next));
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  useEffect(() => {
    setSelectedAgentType((current) => (
      availableAgents.some((agent) => agent.agentType === current)
        ? current
        : (availableAgents[0]?.agentType ?? '')
    ));
  }, [availableAgents]);

  const operation = snapshot?.operation ?? null;
  const insightOperation = snapshot?.insightOperation ?? null;
  const report = snapshot?.latestReport ?? null;
  const active = isPersonalAnalyticsActive(snapshot);
  const insightActive = insightOperation?.status === 'queued' || insightOperation?.status === 'analyzing'
    || insightOperation?.status === 'validating-report' || insightOperation?.status === 'cancelling';
  const autoSyncedRef = useRef(false);
  const syncedRangeRef = useRef<string | null>(null);
  const range = useMemo(() => rangeValue(rangePreset, customStart, customEnd), [rangePreset, customStart, customEnd]);
  const rangeKey = JSON.stringify(range.value);
  useEffect(() => {
    if (loading || snapshot === null || range.invalid) return;
    if (active || submitting) {
      autoSyncedRef.current = true;
      syncedRangeRef.current = rangeKey;
      return;
    }
    if (autoSyncedRef.current && syncedRangeRef.current === rangeKey) return;
    autoSyncedRef.current = true;
    syncedRangeRef.current = rangeKey;
    setSubmitting(true);
    void syncPersonalAnalytics()
      .then((next) => setSnapshot((current) => mergePersonalAnalyticsSnapshot(current, next)))
      .catch(() => undefined)
      .finally(() => setSubmitting(false));
  }, [active, loading, range.invalid, rangeKey, snapshot === null, submitting]);

  const progress = operation?.progress.totalUnits
    ? Math.min(100, (operation.progress.processedUnits / operation.progress.totalUnits) * 100)
    : 0;
  const locale = i18n.language.startsWith('zh') ? 'zh-CN' : 'en-US';
  const number = useMemo(() => new Intl.NumberFormat(locale), [locale]);

  useEffect(() => {
    if (loading || snapshot === null || range.invalid) return;
    let disposed = false;
    setRangeQuerying(true);
    void queryPersonalAnalyticsReport(range.value, selectedAgentType || undefined)
      .then((next) => {
        if (!disposed) setSnapshot((current) => ({ ...(current ?? snapshot), latestReport: next }));
      })
      .catch(() => undefined)
      .finally(() => { if (!disposed) setRangeQuerying(false); });
    return () => { disposed = true; };
  }, [loading, range, selectedAgentType, snapshot === null]);

  const activeOperationsRef = useRef({ sync: false, insight: false });
  const operationRevision = operation?.revision ?? null;
  const insightRevision = insightOperation?.revision ?? null;
  useEffect(() => {
    const previous = activeOperationsRef.current;
    activeOperationsRef.current = { sync: active, insight: insightActive };
    if (loading || snapshot === null || range.invalid) return;
    const syncCompleted = previous.sync && !active;
    const insightCompleted = previous.insight && !insightActive;
    if (!syncCompleted && !insightCompleted) return;
    let disposed = false;
    setRangeQuerying(true);
    void queryPersonalAnalyticsReport(range.value, selectedAgentType || undefined)
      .then((next) => {
        if (!disposed) setSnapshot((current) => ({ ...(current ?? snapshot), latestReport: next }));
      })
      .catch(() => undefined)
      .finally(() => { if (!disposed) setRangeQuerying(false); });
    return () => { disposed = true; };
  }, [active, insightActive, insightRevision, loading, operationRevision, range, selectedAgentType, snapshot === null]);
  const start = async () => {
    if (active || submitting) return;
    setSubmitting(true);
    setStartError(null);
    try {
      const next = await syncPersonalAnalytics();
      setSnapshot((current) => mergePersonalAnalyticsSnapshot(current, next));
    } catch (error) {
      setStartError(personalAnalyticsErrorMessage(t, error));
    } finally {
      setSubmitting(false);
    }
  };

  const startInsights = async () => {
    if (!selectedAgentType || !report || insightActive || insightSubmitting || range.invalid) return;
    setInsightSubmitting(true);
    setStartError(null);
    try {
      const next = await startPersonalAnalyticsInsights(selectedAgentType, range.value);
      setSnapshot((current) => ({ ...(current ?? snapshot!), insightOperation: next }));
    } catch (error) {
      setStartError(personalAnalyticsErrorMessage(t, error));
    } finally {
      setInsightSubmitting(false);
    }
  };

  const cancelInsights = async () => {
    if (!insightOperation || !insightActive) return;
    const next = await cancelPersonalAnalyticsInsights(insightOperation.operationId);
    setSnapshot((current) => ({ ...(current ?? snapshot!), insightOperation: next }));
  };

  const cancel = async () => {
    if (!operation || !active) return;
    const next = await cancelPersonalAnalytics(operation.operationId);
    setSnapshot((current) => mergePersonalAnalyticsSnapshot(current, next));
  };

  return (
    <main className="min-h-0 flex-1 bg-background text-foreground" data-personal-analytics-page="true">
      <ScrollArea className="h-full">
        <div className="mx-auto w-full max-w-6xl px-5 py-5 md:px-8 md:py-7">
          <header className="flex flex-wrap items-start justify-between gap-5 border-b border-border/70 pb-5">
            <div className="min-w-0">
              <h1 className="text-xl font-semibold">{t('personalAnalytics.title')}</h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {report ? t('personalAnalytics.generatedAt', { value: formatDate(report.generatedAt, locale) }) : t('personalAnalytics.emptySubtitle')}
              </p>
            </div>
            <div className="w-full min-w-0 md:w-auto md:max-w-[36rem]">
              <div className="flex flex-wrap items-center gap-2" data-personal-analytics-range="true">
                {(['all', 'today', 'last7', 'last30', 'custom'] as const).map((preset) => (
                  <Button key={preset} size="sm" variant={rangePreset === preset ? 'default' : 'outline'} onClick={() => setRangePreset(preset)}>
                    {t(`personalAnalytics.range.${preset}`)}
                  </Button>
                ))}
              </div>
              {rangePreset === 'custom' ? (
                <div className="mt-2 flex flex-wrap gap-2">
                  <input type="date" className="h-9 min-w-0 rounded-md border border-input bg-transparent px-3 text-sm" value={customStart} onChange={(event) => setCustomStart(event.target.value)} aria-label={t('personalAnalytics.range.start')} />
                  <input type="date" className="h-9 min-w-0 rounded-md border border-input bg-transparent px-3 text-sm" value={customEnd} onChange={(event) => setCustomEnd(event.target.value)} aria-label={t('personalAnalytics.range.end')} />
                </div>
              ) : null}
              {range.invalid ? <p role="alert" className="mt-2 text-sm text-destructive">{t('personalAnalytics.range.invalid')}</p> : null}
              <label className="text-sm font-medium" htmlFor="personal-analytics-agent">{t('personalAnalytics.agent')}</label>
              <div className="mt-2 flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center">
                <Select value={selectedAgentType} onValueChange={setSelectedAgentType} disabled={insightActive || insightSubmitting || availableAgents.length === 0}>
                  <SelectTrigger id="personal-analytics-agent" data-personal-analytics-agent="true" className="min-w-0 flex-1 sm:w-64">
                    <SelectValue placeholder={t('personalAnalytics.selectAgent')} />
                  </SelectTrigger>
                  <SelectContent>
                    {availableAgents.map((agent) => (
                      <SelectItem key={agent.agentType} value={agent.agentType}>
                        <span className="flex min-w-0 items-center gap-2"><Bot className="size-4" /><span className="truncate">{agent.displayName}</span></span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button className="sm:shrink-0" data-personal-analytics-sync="true" data-personal-analytics-start="true" onClick={() => void start()} disabled={active || submitting}>
                  {submitting ? <RefreshCw className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
                  {submitting ? t('personalAnalytics.syncing') : t('personalAnalytics.sync')}
                </Button>
                <Button variant="outline" className="sm:shrink-0" data-personal-analytics-insight="true" onClick={() => void startInsights()} disabled={active || !selectedAgentType || !report || insightActive || insightSubmitting || range.invalid || availableAgents.length === 0}>
                  {insightSubmitting || insightActive ? <RefreshCw className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
                  {t('personalAnalytics.generateInsights')}
                </Button>
                {active ? <Button variant="outline" className="sm:shrink-0" onClick={() => void cancel()} disabled={operation?.status === 'cancelling'}><Square className="size-4" />{t('personalAnalytics.cancel')}</Button> : null}
                {insightActive ? <Button variant="outline" className="sm:shrink-0" onClick={() => void cancelInsights()} disabled={insightOperation?.status === 'cancelling'}><Square className="size-4" />{t('personalAnalytics.cancelInsights')}</Button> : null}
              </div>
              {agentRegistry && availableAgents.length === 0 ? (
                <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
                  <p className="text-sm text-muted-foreground">{t('personalAnalytics.noAvailableAgent')}</p>
                  <Button variant="link" size="sm" className="h-auto px-0" onClick={onOpenAgentManagement}><Settings className="size-4" />{t('personalAnalytics.manageAgents')}</Button>
                </div>
              ) : null}
              {startError ? <p role="alert" className="mt-2 text-sm text-destructive">{startError}</p> : null}
            </div>
          </header>

          {operation ? (
            <section className="border-b border-border/70 py-4" aria-live="polite">
              <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
                <span className="flex items-center gap-2 font-medium">
                  {active ? <RefreshCw className="size-4 animate-spin text-primary" /> : statusIcon(operation.status)}
                  {t(`personalAnalytics.status.${operation.status}`)}
                </span>
                {operation.progress.totalUnits > 0 ? <span className="text-muted-foreground">{number.format(operation.progress.processedUnits)} / {number.format(operation.progress.totalUnits)}</span> : null}
              </div>
              {active ? <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted" aria-hidden="true"><div className="h-full bg-primary transition-[width] duration-300" style={{ width: `${operation.status === 'scanning' ? progress : 100}%` }} /></div> : null}
              {operation.error ? <p className="mt-2 text-sm text-destructive">{t(`personalAnalytics.errors.${operation.error.code}`, { defaultValue: t('personalAnalytics.errors.unknown') })}</p> : null}
            </section>
          ) : null}

          {rangeQuerying && report ? <p className="border-b border-border/70 py-3 text-sm text-muted-foreground" role="status">{t('personalAnalytics.querying')}</p> : null}

          {insightOperation ? (
            <section className="border-b border-border/70 py-4" aria-live="polite">
              <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
                <span className="flex items-center gap-2 font-medium">
                  {insightActive ? <RefreshCw className="size-4 animate-spin text-primary" /> : statusIcon(insightOperation.status)}
                  {t(`personalAnalytics.insightStatus.${insightOperation.status}`)}
                </span>
              </div>
              {insightOperation.error ? <p className="mt-2 text-sm text-destructive">{t(`personalAnalytics.errors.${insightOperation.error.code}`, { defaultValue: t('personalAnalytics.errors.unknown') })}</p> : null}
            </section>
          ) : null}

          {loading && !report ? <LoadingState label={t('personalAnalytics.loading')} /> : null}
          {!loading && !report ? <EmptyState /> : null}
          {report ? <ReportContent report={report} number={number} locale={locale} /> : null}
        </div>
      </ScrollArea>
    </main>
  );
}

function ReportContent({ report, number, locale }: { report: PersonalAnalyticsReportVm; number: Intl.NumberFormat; locale: string }) {
  const { t } = useTranslation();
  const sections = [
    ['overview', t('personalAnalytics.overview')],
    ['recent-tasks', t('personalAnalytics.recentTasks')],
    ['reliability', t('personalAnalytics.reliability')],
    ['quality', t('personalAnalytics.quality')],
    ['efficiency', t('personalAnalytics.efficiency')],
    ['token-usage', t('personalAnalytics.tokens')],
    ['context-and-skills', t('personalAnalytics.contextAndSkills')],
    ['coverage', t('personalAnalytics.coverage')],
  ] as const;
  const metrics: Array<[string, PersonalAnalyticsRateMetricVm]> = [
    ['directReplyCompletionRate', report.reliability.directReplyCompletionRate],
    ['workflowRunTerminalSuccessRate', report.reliability.workflowRunTerminalSuccessRate],
    ['autoOuterRunTerminalSuccessRate', report.reliability.autoOuterRunTerminalSuccessRate],
  ];
  return (
    <div data-personal-analytics-report="true" className="grid gap-6 lg:grid-cols-[13rem_minmax(0,1fr)]">
      <SectionNav sections={sections} />
      <div className="min-w-0">
      <ReportSection id="overview" icon={<Gauge className="size-4" />} title={t('personalAnalytics.overview')}>
        <div className="grid grid-cols-2 gap-x-8 gap-y-5 md:grid-cols-3">
          <SummaryValue label={t('personalAnalytics.summary.projects')} value={number.format(report.overview.projectCount)} />
          <SummaryValue label={t('personalAnalytics.summary.tasks')} value={number.format(report.overview.taskCount)} />
          <SummaryValue label={t('personalAnalytics.summary.conversations')} value={number.format(report.overview.conversationCount)} />
          <SummaryValue label={t('personalAnalytics.totalTokens')} value={formatAnalyticsTokenCount(report.tokenUsage.totalTokens)} />
          <SummaryValue label={t('personalAnalytics.averageRunDuration')} value={formatDuration(report.efficiency.averageTerminalRunActiveSeconds)} />
          <SummaryValue label={t('personalAnalytics.summary.historyRange')} value={report.overview.earliestAt && report.overview.latestAt ? `${formatDate(report.overview.earliestAt, locale)} - ${formatDate(report.overview.latestAt, locale)}` : '-'} />
        </div>
        <div className="mt-6 grid gap-3 lg:grid-cols-3">
          {metrics.map(([key, metric]) => <RateMetric key={key} label={t(`personalAnalytics.metrics.${key}`)} metric={metric} number={number} />)}
        </div>
      </ReportSection>

      <ReportSection id="recent-tasks" icon={<ListChecks className="size-4" />} title={t('personalAnalytics.recentTasks')}>
        <TaskTable tasks={report.recentTasks} locale={locale} emptyLabel={t('personalAnalytics.noRecentTasks')} />
      </ReportSection>

      <ReportSection id="reliability" icon={<ShieldCheck className="size-4" />} title={t('personalAnalytics.reliability')}>
        <div className="grid gap-3 lg:grid-cols-3">
          {metrics.map(([key, metric]) => <RateMetric key={key} label={t(`personalAnalytics.metrics.${key}`)} metric={metric} number={number} />)}
        </div>
      </ReportSection>

      <ReportSection id="quality" icon={<ShieldCheck className="size-4" />} title={t('personalAnalytics.quality')}>
        <div className="grid gap-6 lg:grid-cols-[0.9fr_1.1fr]">
          <div>
            <RateMetric label={t('personalAnalytics.retryReentryRate')} metric={report.quality.retryReentryRate} number={number} />
            <DefinitionGrid items={[
              [t('personalAnalytics.recoveredAfterRetry'), number.format(report.quality.recoveredAfterRetryCount)],
              [t('personalAnalytics.failedCountLabel'), number.format(report.reliability.failedCount)],
              [t('personalAnalytics.cancelledCountLabel'), number.format(report.reliability.cancelledCount)],
              [t('personalAnalytics.nonTerminalCountLabel'), number.format(report.reliability.nonTerminalCount)],
            ]} />
          </div>
          <NamedCountList title={t('personalAnalytics.terminalSignals')} items={report.quality.terminalSignals} number={number} emptyLabel={t('personalAnalytics.noVerifiedSignals')} />
        </div>
        <SectionInsights report={report} section="quality" number={number} />
      </ReportSection>

      <ReportSection id="efficiency" icon={<Clock3 className="size-4" />} title={t('personalAnalytics.efficiency')}>
        <DefinitionGrid items={[
          [t('personalAnalytics.averageRunDuration'), formatDuration(report.efficiency.averageTerminalRunActiveSeconds)],
          [t('personalAnalytics.terminalSamples'), number.format(report.efficiency.terminalRunSampleCount)],
          [t('personalAnalytics.durationZeroFilled'), number.format(report.efficiency.activeDurationZeroFilledCount)],
          [t('personalAnalytics.pauses'), number.format(report.efficiency.pauseCount)],
          [t('personalAnalytics.resumes'), number.format(report.efficiency.resumeCount)],
          [t('personalAnalytics.manualContinues'), number.format(report.efficiency.manualContinueCount)],
        ]} />
        <h3 className="mt-7 text-sm font-semibold">{t('personalAnalytics.topDurationTasks')}</h3>
        <TaskTable tasks={report.efficiency.topDurationTasks} locale={locale} emptyLabel={t('personalAnalytics.noRankedTasks')} ranking="duration" />
        <h3 className="mt-7 text-sm font-semibold">{t('personalAnalytics.nodeEfficiency')}</h3>
        <div className="mt-3 hidden min-w-0 overflow-hidden md:block">
          <Table>
            <TableHeader><TableRow><TableHead>{t('personalAnalytics.node')}</TableHead><TableHead className="text-right">{t('personalAnalytics.averageDuration')}</TableHead><TableHead className="text-right">{t('personalAnalytics.calls')}</TableHead><TableHead className="text-right">{t('personalAnalytics.durationShare')}</TableHead></TableRow></TableHeader>
            <TableBody>{report.efficiency.nodeAggregates.map((node) => <TableRow key={node.nodeId}><TableCell className="min-w-0 break-all font-mono text-xs">{node.nodeId}</TableCell><TableCell className="text-right tabular-nums">{formatDuration(node.averageActiveDurationSeconds)}</TableCell><TableCell className="text-right tabular-nums">{number.format(node.callCount)}</TableCell><TableCell className="text-right tabular-nums">{node.activeDurationShare == null ? '-' : `${(node.activeDurationShare * 100).toFixed(1)}%`}</TableCell></TableRow>)}</TableBody>
          </Table>
        </div>
        <div className="mt-3 divide-y divide-border/60 md:hidden">{report.efficiency.nodeAggregates.map((node) => <div key={node.nodeId} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-3 text-sm"><span className="break-all font-mono text-xs">{node.nodeId}</span><span className="tabular-nums">{formatDuration(node.averageActiveDurationSeconds)} · {number.format(node.callCount)}</span></div>)}</div>
        <SectionInsights report={report} section="efficiency" number={number} />
      </ReportSection>

      <ReportSection id="token-usage" icon={<Coins className="size-4" />} title={t('personalAnalytics.tokens')}>
        <DefinitionGrid items={[
          [t('personalAnalytics.totalTokens'), formatAnalyticsTokenCount(report.tokenUsage.totalTokens)],
          [t('personalAnalytics.inputTokens'), formatAnalyticsTokenCount(report.tokenUsage.inputTokens)],
          [t('personalAnalytics.outputTokens'), formatAnalyticsTokenCount(report.tokenUsage.outputTokens)],
          [t('personalAnalytics.cacheReadTokens'), formatAnalyticsTokenCount(report.tokenUsage.cacheReadTokens)],
          [t('personalAnalytics.cacheWriteTokens'), formatAnalyticsTokenCount(report.tokenUsage.cacheWriteTokens)],
          [t('personalAnalytics.observedPrompts'), number.format(report.tokenUsage.observedPromptCount)],
        ]} />
        <h3 className="mt-7 text-sm font-semibold">{t('personalAnalytics.topTokenTasks')}</h3>
        <TaskTable tasks={report.tokenUsage.topTokenTasks} locale={locale} emptyLabel={t('personalAnalytics.noRankedTasks')} ranking="tokens" />
        <SectionInsights report={report} section="token-usage" number={number} />
      </ReportSection>

      <ReportSection id="context-and-skills" icon={<Wrench className="size-4" />} title={t('personalAnalytics.contextAndSkills')}>
        <div className="grid gap-7 lg:grid-cols-3">
          <NamedCountList title={t('personalAnalytics.tools')} items={report.contextAndTools.topTools} number={number} emptyLabel={t('personalAnalytics.noToolActivity')} />
          <NamedCountList title={t('personalAnalytics.agentActivity')} items={report.contextAndTools.topAgents} number={number} emptyLabel={t('personalAnalytics.noAgentActivity')} />
          <NamedCountList title={t('personalAnalytics.skillUsage')} items={report.contextAndTools.topSkills} number={number} emptyLabel={t('personalAnalytics.noVerifiedSkills')} />
        </div>
        <DefinitionGrid items={[
          [t('personalAnalytics.permissionRequests'), number.format(report.contextAndTools.permissionRequestCount)],
          [t('personalAnalytics.elicitationRequests'), number.format(report.contextAndTools.elicitationRequestCount)],
          [t('personalAnalytics.verifiedSkillCalls'), number.format(report.contextAndTools.verifiedSkillCallCount)],
          [t('personalAnalytics.toolCalls'), number.format(report.contextAndTools.toolCallCount)],
        ]} />
        <SectionInsights report={report} section="context-and-skills" number={number} />
      </ReportSection>

      <ReportSection id="coverage" icon={<FileCheck2 className="size-4" />} title={t('personalAnalytics.coverage')} last>
        <DefinitionGrid items={[
          [t('personalAnalytics.parsedFiles'), `${number.format(report.sourceCoverage.parsedFiles)} / ${number.format(report.sourceCoverage.eligibleFiles)}`],
          [t('personalAnalytics.skippedFiles'), number.format(report.sourceCoverage.skippedFiles)],
          [t('personalAnalytics.corruptFiles'), number.format(report.sourceCoverage.corruptFiles)],
          [t('personalAnalytics.unknownVersionFiles'), number.format(report.sourceCoverage.unknownVersionFiles)],
          [t('personalAnalytics.semanticSamples'), `${number.format(report.sourceCoverage.semanticSampledItems)} / ${number.format(report.sourceCoverage.semanticEligibleItems)}`],
          [t('personalAnalytics.durationZeroFilled'), number.format(report.efficiency.activeDurationZeroFilledCount)],
        ]} />
        {report.warnings.length > 0 ? <div className="mt-5 space-y-2">{report.warnings.map((warning, index) => <div key={`${warning.code}-${index}`} className="flex gap-2 text-sm text-muted-foreground"><AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" /><span>{t(`personalAnalytics.warningCodes.${warning.code}`, { defaultValue: warning.code, ...warning.params })}</span></div>)}</div> : null}
      </ReportSection>
      </div>
    </div>
  );
}

function localDate(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function rangeValue(preset: 'all' | 'today' | 'last7' | 'last30' | 'custom', customStart: string, customEnd: string) {
  if (preset === 'all') return { value: { start: null, end: null }, invalid: false };
  if (preset === 'custom') {
    const invalid = !customStart || !customEnd || customStart > customEnd;
    return { value: { start: customStart || null, end: customEnd || null }, invalid };
  }
  const today = new Date();
  const end = localDate(today);
  const start = new Date(today);
  start.setDate(start.getDate() - (preset === 'today' ? 0 : preset === 'last7' ? 6 : 29));
  return { value: { start: localDate(start), end }, invalid: false };
}

function SectionNav({ sections }: { sections: ReadonlyArray<readonly [string, string]> }) {
  const [activeSection, setActiveSection] = useState(sections[0][0]);
  useEffect(() => {
    const observer = new IntersectionObserver((entries) => {
      const visible = entries.filter((entry) => entry.isIntersecting).sort((left, right) => left.boundingClientRect.top - right.boundingClientRect.top)[0];
      if (visible?.target.id) setActiveSection(visible.target.id);
    }, { rootMargin: '-25% 0px -60% 0px' });
    for (const [id] of sections) {
      const element = document.getElementById(id);
      if (element) observer.observe(element);
    }
    return () => observer.disconnect();
  }, [sections]);
  return (
    <nav className="flex gap-2 overflow-x-auto lg:sticky lg:top-0 lg:h-fit lg:flex-col lg:overflow-visible" data-personal-analytics-nav="true">
      {sections.map(([id, title]) => (
        <Button key={id} size="sm" variant="ghost" className={`justify-start whitespace-nowrap ${activeSection === id ? 'bg-accent text-accent-foreground' : ''}`} onClick={() => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })}>
          {title}
        </Button>
      ))}
    </nav>
  );
}

function TaskTable({ tasks, locale, emptyLabel, ranking }: { tasks: PersonalAnalyticsTaskSummaryVm[]; locale: string; emptyLabel: string; ranking?: 'duration' | 'tokens' }) {
  const { t } = useTranslation();
  if (tasks.length === 0) return <p className="mt-3 text-sm text-muted-foreground">{emptyLabel}</p>;
  const tokenRanking = ranking === 'tokens';
  return <>
    <div className="mt-3 hidden min-w-0 overflow-hidden md:block">
      <Table>
        <TableHeader><TableRow>{ranking ? <TableHead className="w-12">#</TableHead> : null}<TableHead>{t('personalAnalytics.task')}</TableHead><TableHead>{t('personalAnalytics.mode')}</TableHead><TableHead>{t('personalAnalytics.outcome')}</TableHead><TableHead className="hidden lg:table-cell">{t('personalAnalytics.agent')}</TableHead>{!tokenRanking ? <TableHead className="text-right">{t('personalAnalytics.duration')}</TableHead> : null}<TableHead className="text-right">{t('personalAnalytics.token')}</TableHead></TableRow></TableHeader>
        <TableBody>{tasks.map((task, index) => <TableRow key={task.taskLocator}>{ranking ? <TableCell>{index + 1}</TableCell> : null}<TableCell className="min-w-0"><div className="break-words font-medium">{task.title}</div><div className="mt-0.5 break-all text-xs text-muted-foreground">{task.taskLocator}</div></TableCell><TableCell><Badge variant="outline">{task.mode.toUpperCase()}</Badge></TableCell><TableCell>{formatOutcome(task.status, task.outcome)}</TableCell><TableCell className="hidden max-w-48 truncate lg:table-cell">{task.agentNames.join(', ') || '-'}</TableCell>{!tokenRanking ? <TableCell className="text-right tabular-nums">{formatDuration(task.activeDurationSeconds)}</TableCell> : null}<TableCell className="text-right tabular-nums">{formatAnalyticsTokenCount(task.totalTokens)}</TableCell></TableRow>)}</TableBody>
      </Table>
    </div>
    <div className="mt-3 divide-y divide-border/60 md:hidden">{tasks.map((task, index) => <article key={task.taskLocator} className="py-3"><div className="flex min-w-0 items-start justify-between gap-3"><div className="min-w-0"><div className="break-words text-sm font-medium">{ranking ? `${index + 1}. ` : ''}{task.title}</div><div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground"><span>{task.mode.toUpperCase()}</span><span>{formatOutcome(task.status, task.outcome)}</span>{task.lastActivityAt ? <span>{formatDate(task.lastActivityAt, locale)}</span> : null}</div></div><div className="flex shrink-0 flex-col items-end gap-0.5 text-sm font-semibold tabular-nums">{!tokenRanking ? <span>{formatDuration(task.activeDurationSeconds)}</span> : null}<span className={!tokenRanking ? 'text-xs font-normal text-muted-foreground' : undefined}>{formatAnalyticsTokenCount(task.totalTokens)}</span></div></div></article>)}</div>
  </>;
}

function NamedCountList({ title, items, number, emptyLabel }: { title: string; items: Array<{ name: string; count: number }>; number: Intl.NumberFormat; emptyLabel: string }) {
  return <div className="min-w-0"><h3 className="text-sm font-semibold">{title}</h3>{items.length === 0 ? <p className="mt-3 text-sm text-muted-foreground">{emptyLabel}</p> : <div className="mt-3 divide-y divide-border/60">{items.map((item) => <div key={item.name} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2.5 text-sm"><span className="break-all font-mono text-xs">{item.name}</span><span className="tabular-nums">{number.format(item.count)}</span></div>)}</div>}</div>;
}

function SectionInsights({ report, section, number }: { report: PersonalAnalyticsReportVm; section: PersonalAnalyticsReportVm['insights'][number]['section']; number: Intl.NumberFormat }) {
  const { t } = useTranslation();
  const insights = report.insights.filter((insight) => insight.section === section);
  if (insights.length === 0) return null;
  return <div className="mt-7 border-t border-border/60 pt-5"><h3 className="flex items-center gap-2 text-sm font-semibold"><Sparkles className="size-4" />{t('personalAnalytics.insights')}</h3><div className="mt-3 divide-y divide-border/60">{insights.map((insight, index) => <article key={`${insight.title}-${index}`} className="py-4 first:pt-0 last:pb-0"><div className="flex flex-wrap items-center gap-2"><h4 className="text-sm font-semibold">{insight.title}</h4><Badge variant="secondary">{t(`personalAnalytics.confidence.${insight.confidence}`)}</Badge><span className="text-xs text-muted-foreground">{t('personalAnalytics.sampleCount', { value: number.format(insight.sampleCount) })}</span></div><p className="mt-2 text-sm leading-6 text-muted-foreground">{insight.summary}</p><p className="mt-2 text-sm leading-6"><span className="font-medium">{t('personalAnalytics.recommendation')}：</span>{insight.recommendation}</p></article>)}</div></div>;
}

function ReportSection({ id, icon, title, children, last = false }: { id?: string; icon: React.ReactNode; title: string; children: React.ReactNode; last?: boolean }) {
  return <section id={id} className={`${last ? '' : 'border-b border-border/70'} scroll-mt-24 py-7`}><h2 className="mb-5 flex items-center gap-2 text-sm font-semibold">{icon}{title}</h2>{children}</section>;
}

function SummaryValue({ label, value }: { label: string; value: string }) {
  return <div className="min-w-0"><div className="text-xs text-muted-foreground">{label}</div><div className="mt-1 break-words text-lg font-semibold tabular-nums">{value}</div></div>;
}

function RateMetric({ label, metric, number }: { label: string; metric: PersonalAnalyticsRateMetricVm; number: Intl.NumberFormat }) {
  const { t } = useTranslation();
  const percent = metric.rate == null ? 0 : Math.max(0, Math.min(100, metric.rate * 100));
  return <div className="rounded-md border border-border/70 p-4"><div className="text-sm font-medium leading-5">{label}</div><div className="mt-3 flex items-end justify-between gap-3"><span className="text-2xl font-semibold tabular-nums">{metric.rate == null ? '-' : `${(metric.rate * 100).toFixed(1)}%`}</span><span className="text-xs text-muted-foreground">{number.format(metric.numerator)} / {number.format(metric.denominator)}</span></div><div className="mt-3 h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full bg-primary" style={{ width: `${percent}%` }} /></div>{metric.unknownCount > 0 ? <div className="mt-2 text-xs text-muted-foreground">{t('personalAnalytics.unknownCount', { value: number.format(metric.unknownCount) })}</div> : null}</div>;
}

function DefinitionGrid({ items }: { items: Array<[string, string]> }) {
  return <dl className="mt-4 grid grid-cols-2 gap-x-5 gap-y-4 md:grid-cols-3">{items.map(([label, value]) => <div key={label} className="min-w-0"><dt className="text-xs text-muted-foreground">{label}</dt><dd className="mt-1 break-words text-base font-semibold tabular-nums">{value}</dd></div>)}</dl>;
}

function EmptyState() {
  const { t } = useTranslation();
  return <div className="flex min-h-80 flex-col items-center justify-center text-center"><Gauge className="size-8 text-muted-foreground" /><h2 className="mt-4 text-base font-semibold">{t('personalAnalytics.emptyTitle')}</h2><p className="mt-1 max-w-sm text-sm text-muted-foreground">{t('personalAnalytics.emptySubtitle')}</p></div>;
}

function LoadingState({ label }: { label: string }) {
  return <div className="flex min-h-80 items-center justify-center gap-2 text-sm text-muted-foreground"><RefreshCw className="size-4 animate-spin" />{label}</div>;
}

function statusIcon(status: string) {
  if (status === 'failed' || status === 'cancelled') return <Ban className="size-4 text-destructive" />;
  return <FileCheck2 className="size-4 text-emerald-500" />;
}

function formatOutcome(status: string, outcome: string | null) {
  return outcome ? `${status} · ${outcome}` : status;
}

function formatDate(value: string, locale: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(date);
}

function formatDuration(seconds: number | null) {
  if (seconds == null) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${(seconds / 3600).toFixed(1)}h`;
}

export function formatAnalyticsTokenCount(tokens: number) {
  if (tokens >= 1_000) return formatTokenCount(tokens);
  if (tokens <= 0) return '0K';
  if (tokens < 100) return '<0.1K';
  return `${(tokens / 1_000).toFixed(1)}K`;
}

function personalAnalyticsErrorMessage(t: ReturnType<typeof useTranslation>['t'], error: unknown) {
  if (error && typeof error === 'object' && typeof (error as { code?: unknown }).code === 'string') {
    const appError = error as { code: string; params?: Record<string, unknown> };
    return t(`personalAnalytics.errors.${appError.code}`, { ...(appError.params ?? {}), defaultValue: t('personalAnalytics.startFailed') });
  }
  return t('personalAnalytics.startFailed');
}
