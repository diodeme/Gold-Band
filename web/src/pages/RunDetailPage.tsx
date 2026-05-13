import type { RunDetailVm, TaskPage } from '../types';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { isRunStoppable } from '@/lib/status';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';

interface RunDetailPageProps {
  vm: RunDetailVm | null;
  labels: { continueRun: string; retryRun: string; stopRun: string; openRound: string };
  busy: boolean;
  taskId: string;
  onNavigate: (page: TaskPage) => void;
  onContinueRun: (taskId: string, runId: string) => void;
  onRetryRun: (taskId: string, runId: string) => void;
  onKillRun: (taskId: string, runId: string) => void;
}

export function RunDetailPage({ vm, labels, busy, taskId, onNavigate, onContinueRun, onRetryRun, onKillRun }: RunDetailPageProps) {
  if (!vm) return <Page><EmptyState>Loading…</EmptyState></Page>;
  const canStopRun = isRunStoppable(vm.run.status);

  return (
    <Page className="space-y-6 p-8">
      <PageHeader
        eyebrow="Run"
        title={vm.run.id}
        subtitle={<span className="flex gap-2"><StatusBadge value={vm.run.status} /><StatusBadge value={vm.run.outcome} /></span>}
        actions={(
          <>
            <Button variant="outline" disabled={busy || !vm.run.resumable} onClick={() => onContinueRun(taskId, vm.run.id)}>{labels.continueRun}</Button>
            <Button variant="outline" disabled={busy} onClick={() => onRetryRun(taskId, vm.run.id)}>{labels.retryRun}</Button>
            <Button variant="destructive" disabled={busy || !canStopRun} onClick={() => onKillRun(taskId, vm.run.id)}>{labels.stopRun}</Button>
          </>
        )}
      />
      <div className="grid min-h-[520px] grid-cols-[420px_minmax(0,1fr)] gap-6">
        <AppCard className="gap-0 py-0">
          <CardHeader className="border-b px-5 py-3 !pb-3"><CardTitle>Rounds</CardTitle></CardHeader>
          <CardContent className="px-0 py-0">
            <ScrollArea className="h-[520px]">
              <div className="space-y-2 p-3">
                {vm.rounds.map((round) => (
                  <Button className="h-auto w-full justify-between p-4" variant="outline" key={round.id} onClick={() => onNavigate({ kind: 'round-detail', taskId, runId: vm.run.id, roundId: round.id })}>
                    <span className="min-w-0 text-left"><strong className="block truncate">{round.id}</strong><small className="text-muted-foreground">{labels.openRound}</small></span>
                    <span className="flex gap-2"><StatusBadge value={round.status} /><StatusBadge value={round.outcome} /></span>
                  </Button>
                ))}
              </div>
            </ScrollArea>
          </CardContent>
        </AppCard>
        <AppCard className="gap-0 py-0">
          <CardHeader className="border-b px-5 py-3 !pb-3"><CardTitle>Events / Progress</CardTitle></CardHeader>
          <CardContent className="px-4 py-4"><CodeBlock className="min-h-[420px]">{vm.events ?? JSON.stringify(vm.progress ?? {}, null, 2)}</CodeBlock></CardContent>
        </AppCard>
      </div>
    </Page>
  );
}
