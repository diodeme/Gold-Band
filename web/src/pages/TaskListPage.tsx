import { type ReactNode, useEffect, useMemo, useState } from 'react';
import { ArrowLeft, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { TaskListVm, TaskPage, TaskRowVm } from '../types';
import { displayStatus } from '../i18n';
import { StatusBadge } from '../components/StatusBadge';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState, ModuleBar, Page } from '@/components/PageScaffold';
import { RequirementTeaser, fullRequirementText } from '@/components/RequirementDisclosure';
import { TaskTableSkeleton } from '@/components/LoadingState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Skeleton } from '@/components/ui/skeleton';
import { cn } from '@/lib/utils';
import { normalizeTone } from '@/lib/status';

type TaskListLoading = 'initial' | 'manual' | null;

interface TaskListPageProps {
  vm: TaskListVm | null;
  loading: TaskListLoading;
  onNavigate: (page: TaskPage) => void;
  onRefresh: () => void;
}

type TaskFilter = 'all' | 'running' | 'completed';
type TaskSortKey = 'id' | 'title' | 'status' | 'workflow' | 'latest' | 'assets';
type SortDir = 'asc' | 'desc';
type PreviewMode = 'summary' | 'requirement';

const pageSizes = [10, 20, 50];

export function TaskListPage({ vm, loading, onNavigate, onRefresh }: TaskListPageProps) {
  const { t } = useTranslation();
  const [previewTaskId, setPreviewTaskId] = useState<string | null>(null);
  const [previewMode, setPreviewMode] = useState<PreviewMode>('summary');
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);
  const [filter, setFilter] = useState<TaskFilter>('all');
  const [sortKey, setSortKey] = useState<TaskSortKey>('id');
  const [sortDir, setSortDir] = useState<SortDir>('asc');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(10);
  const isInitialLoading = loading === 'initial';
  const isManualRefreshing = loading === 'manual' && vm !== null;

  const filteredTasks = useMemo(() => {
    const tasks = vm?.tasks ?? [];
    if (filter === 'running') return tasks.filter((task) => task.displayStatus === 'running' || task.latestRun?.status === 'running');
    if (filter === 'completed') return tasks.filter((task) => task.displayStatus === 'completed' || task.latestRun?.outcome === 'success');
    return tasks;
  }, [filter, vm]);

  const sortedTasks = useMemo(() => {
    return [...filteredTasks].sort((left, right) => compareTasks(left, right, sortKey, sortDir));
  }, [filteredTasks, sortDir, sortKey]);

  const pageCount = Math.max(1, Math.ceil(sortedTasks.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pagedTasks = sortedTasks.slice(safePageIndex * pageSize, safePageIndex * pageSize + pageSize);
  const previewTask = useMemo(() => {
    if (!previewTaskId) return null;
    return sortedTasks.find((task) => task.id === previewTaskId) ?? null;
  }, [previewTaskId, sortedTasks]);

  useEffect(() => {
    if (safePageIndex !== pageIndex) setPageIndex(safePageIndex);
  }, [pageIndex, safePageIndex]);

  useEffect(() => {
    if (!previewTaskId) return;
    if (sortedTasks.some((task) => task.id === previewTaskId)) return;
    setPreviewTaskId(null);
    setPreviewMode('summary');
    setIsPreviewOpen(false);
  }, [previewTaskId, sortedTasks]);

  const toggleSort = (key: TaskSortKey) => {
    if (sortKey === key) {
      setSortDir((current) => current === 'asc' ? 'desc' : 'asc');
    } else {
      setSortKey(key);
      setSortDir('asc');
    }
  };

  const closePreview = () => {
    setIsPreviewOpen(false);
    setPreviewTaskId(null);
    setPreviewMode('summary');
  };

  const openPreview = (taskId: string) => {
    setPreviewTaskId(taskId);
    setPreviewMode('summary');
    setIsPreviewOpen(true);
  };

  return (
    <Page flush className="flex flex-col" onContextMenu={(event) => event.preventDefault()}>
      <ModuleBar
        title={t('taskList.title')}
        tabs={(
          <Tabs value={filter} onValueChange={(value) => { setFilter(value as TaskFilter); setPageIndex(0); }}>
            <TabsList>
              <TabsTrigger value="all">{t('taskList.allTasks')}</TabsTrigger>
              <TabsTrigger value="running">{t('taskList.runningTasks')}</TabsTrigger>
              <TabsTrigger value="completed">{t('taskList.completedTasks')}</TabsTrigger>
            </TabsList>
          </Tabs>
        )}
        actions={(
          <>
            <Button variant="outline" disabled={isInitialLoading || isManualRefreshing} onClick={onRefresh}>
              <RefreshCw className={cn(isManualRefreshing && 'animate-spin')} />
              {t('common.refresh')}
            </Button>
            <Button disabled>{t('taskList.createTask')}</Button>
            <Button variant="outline" disabled>{t('taskList.importRequirements')}</Button>
          </>
        )}
      />

      {isInitialLoading && !vm ? <TaskListSkeletonPage /> : null}
      {vm ? (
        <div className="min-h-0 flex-1 p-5 xl:p-6">
          <ScrollArea className="h-full min-h-0 min-w-0">
            <div className="space-y-5 pr-1">
              <div className="space-y-1">
                <p className="font-mono text-xs uppercase tracking-[0.18em] text-primary">{t('taskList.workspacePath')}</p>
                <p className="max-w-4xl text-sm leading-6 text-muted-foreground">{t('taskList.subtitle')}</p>
              </div>
              <div className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-5">
                {vm.cards.map((card) => <SummaryCard card={card} key={card.key ?? card.label} />)}
              </div>

              <AppCard className="relative overflow-hidden py-0 shadow-none">
                {isManualRefreshing ? <div className="absolute inset-x-0 top-0 z-10 h-px bg-border" /> : null}
                <div className="overflow-x-auto">
                  <Table className="w-full min-w-[820px] table-fixed">
                    <colgroup>
                      <col className="w-[12%]" />
                      <col className="w-[36%]" />
                      <col className="w-[12%]" />
                      <col className="w-[11%]" />
                      <col className="w-[12%]" />
                      <col className="w-[8%]" />
                      <col className="w-[9%]" />
                    </colgroup>
                    <TableHeader>
                      <TableRow>
                        <SortableHead label={t('taskList.id')} sortKey="id" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('taskList.taskTitle')} sortKey="title" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('common.status')} sortKey="status" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('common.workflow')} sortKey="workflow" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('taskList.latest')} sortKey="latest" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('common.assets')} sortKey="assets" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <TableHead className="text-right">{t('common.action')}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {pagedTasks.map((task) => (
                        <TableRow
                          className={cn('cursor-pointer', isPreviewOpen && previewTask?.id === task.id && 'bg-primary/10 hover:bg-primary/15')}
                          data-task-preview-row="true"
                          key={task.id}
                          onClick={() => openPreview(task.id)}
                          onDoubleClick={() => onNavigate({ kind: 'workflow', taskId: task.id })}
                        >
                          <TableCell className="truncate font-mono text-xs text-muted-foreground">{task.id}</TableCell>
                          <TableCell className="min-w-0">
                            <strong className="block truncate text-sm">{task.title}</strong>
                            <small className="block truncate text-muted-foreground">{task.requirementPreview || task.description}</small>
                          </TableCell>
                          <TableCell className="truncate"><StatusBadge value={task.displayStatus} label={displayStatus(t, task.displayStatus)} /></TableCell>
                          <TableCell className="truncate"><WorkflowState task={task} /></TableCell>
                          <TableCell className="truncate font-mono text-xs text-muted-foreground">{task.latestRun?.id ?? t('taskList.noRun')}</TableCell>
                          <TableCell className="truncate text-muted-foreground">A{task.artifactCount} / P{task.attachmentCount}</TableCell>
                          <TableCell className="text-right">
                            <Button variant="link" size="sm" onClick={(event) => { event.stopPropagation(); onNavigate({ kind: 'workflow', taskId: task.id }); }}>{t('taskList.enter')}</Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                {sortedTasks.length === 0 ? <div className="p-5"><EmptyState>{t('taskList.noTasks')}</EmptyState></div> : null}
                <div className="flex flex-wrap items-center justify-between gap-3 border-t px-4 py-3 text-sm text-muted-foreground">
                  <span className="shrink-0">{t('common.pageRange', { start: sortedTasks.length ? safePageIndex * pageSize + 1 : 0, end: Math.min(sortedTasks.length, (safePageIndex + 1) * pageSize), total: sortedTasks.length })}</span>
                  <div className="flex flex-wrap items-center gap-2">
                    <span>{t('common.pageSize')}</span>
                    <Select value={String(pageSize)} onValueChange={(value) => { setPageSize(Number(value)); setPageIndex(0); }}>
                      <SelectTrigger className="w-20"><SelectValue /></SelectTrigger>
                      <SelectContent>{pageSizes.map((value) => <SelectItem value={String(value)} key={value}>{value}</SelectItem>)}</SelectContent>
                    </Select>
                    <Button variant="outline" size="sm" disabled={safePageIndex === 0} onClick={() => setPageIndex((value) => Math.max(0, value - 1))}>{t('common.previousPage')}</Button>
                    <Button variant="outline" size="sm" disabled={safePageIndex >= pageCount - 1} onClick={() => setPageIndex((value) => Math.min(pageCount - 1, value + 1))}>{t('common.nextPage')}</Button>
                  </div>
                </div>
              </AppCard>
            </div>
          </ScrollArea>
          <TaskPreviewSheet task={previewTask} mode={previewMode} open={isPreviewOpen && previewTask !== null} onOpenChange={(open) => { if (open) setIsPreviewOpen(true); else closePreview(); }} onNavigate={onNavigate} onOpenRequirement={() => setPreviewMode('requirement')} onBack={() => setPreviewMode('summary')} />
        </div>
      ) : null}
    </Page>
  );
}

function compareTasks(left: TaskRowVm, right: TaskRowVm, key: TaskSortKey, dir: SortDir) {
  const direction = dir === 'asc' ? 1 : -1;
  const leftValue = taskSortValue(left, key);
  const rightValue = taskSortValue(right, key);
  return leftValue.localeCompare(rightValue, undefined, { numeric: true, sensitivity: 'base' }) * direction;
}

function taskSortValue(task: TaskRowVm, key: TaskSortKey) {
  if (key === 'title') return task.title;
  if (key === 'status') return task.displayStatus;
  if (key === 'workflow') return task.workflowValid ? 'valid' : task.workflowExists ? 'invalid' : 'missing';
  if (key === 'latest') return task.latestRun?.id ?? '';
  if (key === 'assets') return String(task.artifactCount + task.attachmentCount).padStart(8, '0');
  return task.id;
}

function SortableHead({ label, sortKey, activeKey, dir, onSort }: { label: string; sortKey: TaskSortKey; activeKey: TaskSortKey; dir: SortDir; onSort: (key: TaskSortKey) => void }) {
  return (
    <TableHead>
      <Button variant="ghost" size="sm" className="h-auto px-0 font-semibold" onClick={() => onSort(sortKey)}>
        {label}{activeKey === sortKey ? <span className="ml-1 text-xs">{dir === 'asc' ? '↑' : '↓'}</span> : null}
      </Button>
    </TableHead>
  );
}

function WorkflowState({ task }: { task: TaskRowVm }) {
  const { t } = useTranslation();
  if (!task.workflowExists) return <StatusBadge value="missing-workflow" tone="warning" label={displayStatus(t, 'missing-workflow')} />;
  if (!task.workflowValid) return <StatusBadge value="invalid" tone="danger" label={displayStatus(t, 'invalid')} />;
  return <StatusBadge value="valid" tone="success" label={displayStatus(t, 'valid')} />;
}

function TaskListSkeletonPage() {
  return (
    <div className="min-h-0 flex-1 p-5 xl:p-6">
      <div className="space-y-5">
        <Skeleton className="h-16 w-2/3" />
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-5">{Array.from({ length: 5 }, (_, index) => <Skeleton className="h-24" key={index} />)}</div>
        <TaskTableSkeleton />
      </div>
    </div>
  );
}

function SummaryCard({ card }: { card: TaskListVm['cards'][number] }) {
  const { t } = useTranslation();
  const tone = normalizeTone(card.tone);
  return (
    <AppCard className="gap-2 overflow-hidden py-0 shadow-none">
      <CardContent className="relative px-4 py-4">
        <span
          className={cn(
            'absolute inset-y-4 left-0 w-1 rounded-r-full',
            tone === 'running' && 'bg-gold-running',
            tone === 'success' && 'bg-gold-success',
            tone === 'warning' && 'bg-gold-warning',
            tone === 'danger' && 'bg-gold-danger',
            tone === 'neutral' && 'bg-border',
          )}
        />
        <span className="block truncate pl-2 text-xs uppercase tracking-[0.14em] text-muted-foreground">{t(`taskList.cards.${card.key}`, { defaultValue: card.label })}</span>
        <strong className="mt-3 block pl-2 text-2xl font-semibold text-foreground">{card.value}</strong>
      </CardContent>
    </AppCard>
  );
}

function TaskPreviewSheet({ task, mode, open, onOpenChange, onNavigate, onOpenRequirement, onBack }: { task: TaskRowVm | null; mode: PreviewMode; open: boolean; onOpenChange: (open: boolean) => void; onNavigate: (page: TaskPage) => void; onOpenRequirement: () => void; onBack: () => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className={cn('max-w-[calc(100vw-2rem)] gap-0 overflow-hidden p-0', mode === 'requirement' ? 'w-[560px] sm:max-w-[560px]' : 'w-[420px] sm:max-w-[420px]')}
        closeLabel={t('common.close')}
        onInteractOutside={(event) => {
          const target = event.target as HTMLElement | null;
          if (target?.closest('[data-task-preview-row="true"]')) event.preventDefault();
        }}
        showOverlay={false}
      >
        {task && mode === 'requirement' ? <TaskRequirementContent task={task} onBack={onBack} /> : null}
        {task && mode === 'summary' ? <TaskPreviewContent task={task} onNavigate={onNavigate} onOpenRequirement={onOpenRequirement} /> : null}
      </SheetContent>
    </Sheet>
  );
}

function TaskPreviewContent({ task, onNavigate, onOpenRequirement }: { task: TaskRowVm; onNavigate: (page: TaskPage) => void; onOpenRequirement: () => void }) {
  const { t } = useTranslation();
  const requirement = fullRequirementText(task.requirement, task.requirementPreview || task.description, t('common.empty'));
  return (
    <>
      <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
        <p className="font-mono text-xs uppercase tracking-[0.18em] text-primary">{t('taskList.taskPreview')}</p>
        <SheetDescription className="sr-only">{t('taskList.taskPreviewDescription')}</SheetDescription>
        <div className="flex min-w-0 flex-wrap gap-2 pr-8">
          <Badge variant="secondary" className="max-w-full truncate font-mono">{task.id}</Badge>
          <StatusBadge value={task.displayStatus} label={displayStatus(t, task.displayStatus)} />
        </div>
        <SheetTitle className="line-clamp-2 break-words text-xl">{task.title}</SheetTitle>
      </SheetHeader>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-5 p-5">
          <section className="space-y-2 rounded-lg border-l-2 border-primary bg-primary/5 p-4">
            <h3 className="font-semibold">{t('common.requirement')}</h3>
            <RequirementTeaser text={requirement} detailLabel={t('common.viewFullRequirement')} quote onOpenDetail={onOpenRequirement} />
          </section>
          <section className="space-y-3">
            <h3 className="font-semibold">{t('taskList.executionStats')}</h3>
            <dl className="grid gap-2 text-sm">
              <Stat label={t('taskList.latestRun')} value={task.latestRun?.id ?? '-'} mono />
              <Stat label={t('common.artifacts')} value={task.artifactCount} mono />
              <Stat label={t('common.attachments')} value={task.attachmentCount} mono />
              <Stat label={t('common.workflow')} value={task.workflowValid ? displayStatus(t, 'valid') : task.workflowExists ? displayStatus(t, 'invalid') : displayStatus(t, 'missing-workflow')} />
            </dl>
          </section>
          <div className="flex flex-col gap-2">
            {task.resumableRunId ? <Button className="w-full min-w-0"><span className="truncate">{t('taskList.continueExecution', { runId: task.resumableRunId })}</span></Button> : null}
            <Button className="w-full" variant="outline" onClick={() => onNavigate({ kind: 'workflow', taskId: task.id })}>{t('common.workflow')}</Button>
            <Button className="w-full" variant="outline" disabled>{t('taskList.viewArtifacts')}</Button>
          </div>
        </div>
      </ScrollArea>
    </>
  );
}

function TaskRequirementContent({ task, onBack }: { task: TaskRowVm; onBack: () => void }) {
  const { t } = useTranslation();
  const requirement = fullRequirementText(task.requirement, task.requirementPreview || task.description, t('common.empty'));
  return (
    <>
      <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
        <Button variant="ghost" size="sm" className="h-8 w-fit px-2 text-muted-foreground" onClick={onBack}>
          <ArrowLeft className="h-4 w-4" />
          {t('common.back')}
        </Button>
        <SheetDescription className="sr-only">{t('common.fullRequirementDescription')}</SheetDescription>
        <SheetTitle className="break-words text-xl">{task.title} · {t('common.fullRequirement')}</SheetTitle>
      </SheetHeader>
      <ScrollArea className="min-h-0 flex-1">
        <div className="p-5">
          <CodeBlock className="whitespace-pre-wrap text-sm leading-7">{requirement}</CodeBlock>
        </div>
      </ScrollArea>
    </>
  );
}

function Stat({ label, value, mono = false }: { label: string; value: ReactNode; mono?: boolean }) {
  return (
    <div className="min-w-0 rounded-lg border bg-muted/20 px-3 py-2">
      <dt className="truncate text-xs text-muted-foreground">{label}</dt>
      <dd className={cn('mt-1 min-w-0 break-words text-sm text-foreground', mono && 'font-mono')}>{value}</dd>
    </div>
  );
}
