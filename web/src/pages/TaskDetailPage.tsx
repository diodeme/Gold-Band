import type { TaskDetailVm, TaskPage } from '../types';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState, Metric, MetricsBar, Page, PageHeader } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';

interface TaskDetailPageProps {
  vm: TaskDetailVm | null;
  labels: { openWorkflow: string; startRun: string; continueRun: string; requirement: string; openRun: string };
  busy: boolean;
  onNavigate: (page: TaskPage) => void;
  onStartRun: (taskId: string) => void;
  onContinueRun: (taskId: string, runId: string) => void;
}

export function TaskDetailPage({ vm, labels, busy, onNavigate, onStartRun, onContinueRun }: TaskDetailPageProps) {
  if (!vm) return <Page><EmptyState>Loading…</EmptyState></Page>;
  const resumable = vm.task.resumableRunId;
  return (
    <Page className="space-y-6 p-8">
      <PageHeader
        eyebrow={vm.task.id}
        title={vm.task.title}
        subtitle={vm.task.requirementPreview || vm.task.description}
        actions={(
          <>
            <Button onClick={() => onNavigate({ kind: 'workflow', taskId: vm.task.id })}>{labels.openWorkflow}</Button>
            <Button variant="outline" disabled={busy || !vm.task.workflowValid} onClick={() => onStartRun(vm.task.id)}>{labels.startRun}</Button>
            <Button variant="outline" disabled={busy || !resumable} onClick={() => resumable && onContinueRun(vm.task.id, resumable)}>{labels.continueRun}</Button>
          </>
        )}
      />

      <MetricsBar>
        <Metric label="Task ID" value={vm.task.id} />
        <Metric label="Task Status" value={<StatusBadge value={vm.task.displayStatus} />} />
        <Metric label="Workflow" value={vm.task.workflowValid ? 'Valid' : vm.task.workflowExists ? 'Invalid' : 'Missing'} />
        <Metric label="Active / Latest Run" value={vm.task.latestRun?.id ?? '-'} />
        <Metric label="Artifacts" value={`A${vm.task.artifactCount} / P${vm.task.attachmentCount}`} />
      </MetricsBar>

      <div className="space-y-5">
        <AppCard className="gap-0 py-0">
          <CardHeader className="flex-row items-center justify-between border-b px-5 py-3 !pb-3">
            <CardTitle>{labels.requirement}</CardTitle>
            <span className="text-sm text-muted-foreground">完整 authoring 内容，只读</span>
          </CardHeader>
          <CardContent className="px-4 py-4"><CodeBlock className="font-sans text-sm leading-7">{vm.requirement}</CodeBlock></CardContent>
        </AppCard>

        <AppCard className="gap-0 py-0">
          <CardHeader className="flex-row items-center justify-between border-b px-5 py-3 !pb-3">
            <CardTitle>当前状态</CardTitle>
            {vm.task.workflowError ? <span className="text-sm text-muted-foreground">{vm.task.workflowError}</span> : null}
          </CardHeader>
          <CardContent className="grid grid-cols-4 gap-3 px-4 py-4">
            <Metric label="workflow 校验" value={vm.task.workflowValid ? 'valid' : vm.task.workflowExists ? 'invalid' : 'missing'} compact />
            <Metric label="resumable run" value={resumable ?? '-'} compact />
            <Metric label="latest outcome" value={vm.task.latestRun?.outcome ?? vm.task.latestRun?.status ?? '-'} compact />
            <Metric label="latest updated" value={vm.task.latestRun?.updatedAt ?? '-'} compact />
          </CardContent>
        </AppCard>

        <AppCard className="gap-0 py-0">
          <CardHeader className="border-b px-5 py-3 !pb-3"><CardTitle>最近运行</CardTitle></CardHeader>
          <Table>
            <TableHeader><TableRow><TableHead>Run ID</TableHead><TableHead>Status</TableHead><TableHead>Outcome</TableHead><TableHead>Current Round</TableHead><TableHead>Updated</TableHead><TableHead className="text-right">Action</TableHead></TableRow></TableHeader>
            <TableBody>
              {vm.runs.map((run) => (
                <TableRow className="cursor-pointer" key={run.id} onClick={() => onNavigate({ kind: 'workflow', taskId: vm.task.id })}>
                  <TableCell className="font-semibold">{run.id}</TableCell>
                  <TableCell><StatusBadge value={run.status} /></TableCell>
                  <TableCell><StatusBadge value={run.outcome} /></TableCell>
                  <TableCell className="text-muted-foreground">{run.currentRound ?? '-'}</TableCell>
                  <TableCell className="text-muted-foreground">{run.updatedAt}</TableCell>
                  <TableCell className="text-right"><Button variant="link" size="sm" onClick={(event) => { event.stopPropagation(); onNavigate({ kind: 'workflow', taskId: vm.task.id }); }}>{labels.openRun}</Button></TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </AppCard>
      </div>
    </Page>
  );
}
