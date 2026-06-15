import { type ChangeEvent, type Dispatch, type ReactNode, type SetStateAction, useEffect, useMemo, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { Check, ChevronDown, Copy, Plus, RefreshCw, Trash2, Upload, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AgentRegistryVm, CreateTaskInput, ProfileListVm, TaskListVm, TaskPage, TaskRowVm, WorkflowDsl, WorkflowTemplate, WorkflowTemplateStore, WorkflowVm } from '../types';
import { displayAppError, displayStatus, displayWorkflowError } from '../i18n';
import { deleteWorkflowTemplate, getAgentRegistry, getProfiles, getWorkflowTemplates, saveWorkflowTemplate, updateWorkflowTemplate } from '../api';
import { StatusBadge } from '../components/StatusBadge';
import { validateWorkflowForSave, WorkflowEditor } from '../components/WorkflowEditor';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { fullRequirementText } from '@/components/RequirementDisclosure';
import { TaskTableSkeleton } from '@/components/LoadingState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Pagination, PaginationContent, PaginationItem, PaginationLink } from '@/components/ui/pagination';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import { cn } from '@/lib/utils';

type TaskListLoading = 'initial' | 'manual' | null;

interface TaskListPageProps {
  vm: TaskListVm | null;
  loading: TaskListLoading;
  breadcrumbs?: ReactNode;
  onNavigate: (page: TaskPage) => void;
  onRefresh: () => void;
  onCreateTask: (input: CreateTaskInput) => Promise<WorkflowVm | undefined>;
  onOpenProfileManagement: () => void;
  createTaskDraft: CreateTaskDraftState;
  onCreateTaskDraftChange: Dispatch<SetStateAction<CreateTaskDraftState>>;
}

export interface CreateTaskDraftState {
  open: boolean;
  selectedTemplateId: string | null;
  lastUsedHintDismissed: boolean;
  baseWorkflow: WorkflowDsl | null;
  saveTemplateName: string;
  title: string;
  description: string;
  requirementFileName: string;
  requirementContent: string;
  workflow: WorkflowDsl | null;
  initialized: boolean;
}

type TaskFilter = 'all' | 'running' | 'completed' | 'resumable' | 'failed' | 'invalid';
type TaskSortKey = 'id' | 'title' | 'status' | 'workflow' | 'latest';
type SortDir = 'asc' | 'desc';

const pageSizes = [10, 20, 50];

export function createInitialCreateTaskDraft(): CreateTaskDraftState {
  return {
    open: false,
    selectedTemplateId: null,
    lastUsedHintDismissed: false,
    baseWorkflow: null,
    saveTemplateName: '',
    title: '',
    description: '',
    requirementFileName: '',
    requirementContent: '',
    workflow: null,
    initialized: false,
  };
}

export function TaskListPage({ vm, loading, breadcrumbs, onNavigate, onRefresh, onCreateTask, onOpenProfileManagement, createTaskDraft, onCreateTaskDraftChange }: TaskListPageProps) {
  const { t } = useTranslation();
  const [previewTaskId, setPreviewTaskId] = useState<string | null>(null);
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);
  const [filter, setFilter] = useState<TaskFilter>('all');
  const [searchTerm, setSearchTerm] = useState('');
  const [sortKey, setSortKey] = useState<TaskSortKey>('id');
  const [sortDir, setSortDir] = useState<SortDir>('desc');
  const [pageIndex, setPageIndex] = useState(0);
  const [pageSize, setPageSize] = useState(10);
  const isInitialLoading = loading === 'initial';
  const isManualRefreshing = loading === 'manual' && vm !== null;

  const filteredTasks = useMemo(() => {
    const tasks = (vm?.tasks ?? []).filter((task) => matchesTaskFilter(task, filter));
    const query = searchTerm.trim().toLowerCase();
    if (!query) return tasks;
    return tasks.filter((task) => taskSearchText(task, t).includes(query));
  }, [filter, searchTerm, t, vm]);

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
    setIsPreviewOpen(false);
  }, [previewTaskId, sortedTasks]);

  const toggleSort = (key: TaskSortKey) => {
    if (sortKey === key) {
      setSortDir((current) => current === 'asc' ? 'desc' : 'asc');
    } else {
      setSortKey(key);
      setSortDir(key === 'id' ? 'desc' : 'asc');
    }
  };

  const updateFilter = (value: TaskFilter) => {
    setFilter(value);
    setPageIndex(0);
  };

  const updateSearchTerm = (value: string) => {
    setSearchTerm(value);
    setPageIndex(0);
  };

  const quickFilterValue = filter === 'running' || filter === 'completed' ? filter : 'all';
  const statusFilterValue = filter === 'resumable' || filter === 'failed' || filter === 'invalid' ? filter : 'all';

  const closePreview = () => {
    setIsPreviewOpen(false);
    setPreviewTaskId(null);
  };

  const openPreview = (taskId: string) => {
    setPreviewTaskId(taskId);
    setIsPreviewOpen(true);
  };

  return (
    <Page flush className="flex flex-col" onContextMenu={(event) => event.preventDefault()}>
      <PageHeader
        breadcrumbs={breadcrumbs}
        title={<span className="text-title">{t('taskList.title')}</span>}
        actions={(
          <>
            <Button variant="outline" disabled={isInitialLoading || isManualRefreshing} onClick={onRefresh}>
              <RefreshCw className={cn(isManualRefreshing && 'animate-spin')} />
              {t('common.refresh')}
            </Button>
            <Button disabled={isInitialLoading} onClick={() => onCreateTaskDraftChange((draft) => ({ ...draft, open: true }))}>{t('taskList.createTask')}</Button>
          </>
        )}
      />

      {isInitialLoading && !vm ? <TaskListSkeletonPage /> : null}
      {vm ? (
        <div className="min-h-0 flex-1 p-5 xl:p-6">
          <ScrollArea className="h-full min-h-0 min-w-0">
            <div className="space-y-5 pr-1">
              <AppCard className="relative overflow-hidden py-0 shadow-none">
                {isManualRefreshing ? <div className="absolute inset-x-0 top-0 z-10 h-px bg-border" /> : null}
                <div className="flex flex-col gap-3 border-b px-4 py-3 lg:flex-row lg:items-center lg:justify-between">
                  <div className="min-w-0">
                    <h2 className="truncate text-base font-semibold text-foreground">{t('taskList.taskList')}</h2>
                  </div>
                  <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2 lg:justify-end">
                    <Tabs value={quickFilterValue} onValueChange={(value) => updateFilter(value as TaskFilter)}>
                      <TabsList>
                        <TabsTrigger value="all">{t('taskList.allTasks')}</TabsTrigger>
                        <TabsTrigger value="running">{t('taskList.runningTasks')}</TabsTrigger>
                        <TabsTrigger value="completed">{t('taskList.completedTasks')}</TabsTrigger>
                      </TabsList>
                    </Tabs>
                    <Select value={statusFilterValue} onValueChange={(value) => updateFilter(value as TaskFilter)}>
                      <SelectTrigger className="w-[148px]" aria-label={t('taskList.statusFilter')}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="all">{t('taskList.allStatuses')}</SelectItem>
                        <SelectItem value="resumable">{displayStatus(t, 'resumable')}</SelectItem>
                        <SelectItem value="failed">{displayStatus(t, 'failed')}</SelectItem>
                        <SelectItem value="invalid">{t('taskList.configIssues')}</SelectItem>
                      </SelectContent>
                    </Select>
                    <Label className="min-w-[220px] flex-1 lg:max-w-sm">
                      <span className="sr-only">{t('taskList.search')}</span>
                      <Input
                        value={searchTerm}
                        onChange={(event) => updateSearchTerm(event.target.value)}
                        placeholder={t('taskList.searchPlaceholder')}
                      />
                    </Label>
                  </div>
                </div>
                <div className="overflow-x-auto">
                  <Table className="w-full min-w-[820px] table-fixed">
                    <colgroup>
                      <col className="w-[12%]" />
                      <col className="w-[40%]" />
                      <col className="w-[13%]" />
                      <col className="w-[12%]" />
                      <col className="w-[13%]" />
                      <col className="w-[10%]" />
                    </colgroup>
                    <TableHeader>
                      <TableRow>
                        <SortableHead label={t('taskList.id')} sortKey="id" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('taskList.taskTitle')} sortKey="title" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('common.status')} sortKey="status" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('common.workflow')} sortKey="workflow" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <SortableHead label={t('taskList.latest')} sortKey="latest" activeKey={sortKey} dir={sortDir} onSort={toggleSort} />
                        <TableHead className="pr-4 text-right">{t('common.action')}</TableHead>
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
                          <TableCell className="truncate text-xs font-medium text-muted-foreground">{task.id}</TableCell>
                          <TableCell className="min-w-0">
                            <strong className="block truncate text-sm">{task.title}</strong>
                            <small className="block truncate text-muted-foreground">{task.requirementPreview || task.description}</small>
                          </TableCell>
                          <TableCell className="truncate"><StatusBadge value={task.displayStatus} label={displayStatus(t, task.displayStatus)} /></TableCell>
                          <TableCell className="truncate"><WorkflowState task={task} /></TableCell>
                          <TableCell className="truncate text-xs font-medium text-muted-foreground">{task.latestRun?.id ?? t('taskList.noRun')}</TableCell>
                          <TableCell className="pr-4 text-right">
                            <Button variant="link" size="sm" className="px-0" onClick={(event) => { event.stopPropagation(); onNavigate({ kind: 'workflow', taskId: task.id }); }}>{t('taskList.enter')}</Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                {sortedTasks.length === 0 ? <div className="p-5"><EmptyState>{vm.tasks.length === 0 ? t('taskList.noTasks') : t('taskList.noMatchingTasks')}</EmptyState></div> : null}
                <div className="flex flex-wrap items-center justify-between gap-3 border-t px-4 py-3 text-sm text-muted-foreground">
                  <span className="shrink-0">{t('common.pageRange', { start: sortedTasks.length ? safePageIndex * pageSize + 1 : 0, end: Math.min(sortedTasks.length, (safePageIndex + 1) * pageSize), total: sortedTasks.length })}</span>
                  <div className="flex flex-wrap items-center gap-2">
                    <span>{t('common.pageSize')}</span>
                    <Select value={String(pageSize)} onValueChange={(value) => { setPageSize(Number(value)); setPageIndex(0); }}>
                      <SelectTrigger className="w-20"><SelectValue /></SelectTrigger>
                      <SelectContent>{pageSizes.map((value) => <SelectItem value={String(value)} key={value}>{value}</SelectItem>)}</SelectContent>
                    </Select>
                    <TaskPagination pageIndex={safePageIndex} pageCount={pageCount} onPageChange={setPageIndex} />
                  </div>
                </div>
              </AppCard>
            </div>
          </ScrollArea>
          <TaskPreviewSheet task={previewTask} open={isPreviewOpen && previewTask !== null} onOpenChange={(open) => { if (open) setIsPreviewOpen(true); else closePreview(); }} onNavigate={onNavigate} />
        </div>
      ) : null}
      <CreateTaskSheet
        draft={createTaskDraft}
        onDraftChange={onCreateTaskDraftChange}
        onOpenProfileManagement={onOpenProfileManagement}
        onCreateTask={async (input) => {
          const created = await onCreateTask(input);
          if (created) {
            onCreateTaskDraftChange(createInitialCreateTaskDraft());
            onNavigate({ kind: 'workflow', taskId: created.task.id });
          }
          return created;
        }}
      />
    </Page>
  );
}

function CreateTaskSheet({ draft, onDraftChange, onCreateTask, onOpenProfileManagement }: { draft: CreateTaskDraftState; onDraftChange: Dispatch<SetStateAction<CreateTaskDraftState>>; onCreateTask: (input: CreateTaskInput) => Promise<WorkflowVm | undefined>; onOpenProfileManagement: () => void }) {
  const { t } = useTranslation();
  const [agentRegistry, setAgentRegistry] = useState<AgentRegistryVm | null>(null);
  const [profileList, setProfileList] = useState<ProfileListVm | null>(null);
  const [templateStore, setTemplateStore] = useState<WorkflowTemplateStore | null>(null);
  const [templatePickerOpen, setTemplatePickerOpen] = useState(false);
  const [deleteTemplateTarget, setDeleteTemplateTarget] = useState<WorkflowTemplate | null>(null);
  const requirementInputRef = useRef<HTMLInputElement | null>(null);
  const [pendingImportFile, setPendingImportFile] = useState<File | null>(null);
  const [confirmRequirementImportOpen, setConfirmRequirementImportOpen] = useState(false);
  const [confirmDiscardOpen, setConfirmDiscardOpen] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [workflowError, setWorkflowError] = useState<string | null>(null);
  const [workflowNotice, setWorkflowNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const {
    open,
    selectedTemplateId,
    lastUsedHintDismissed,
    baseWorkflow,
    saveTemplateName,
    title,
    description,
    requirementFileName,
    requirementContent,
    workflow,
  } = draft;
  const workflowDirty = Boolean(workflow && baseWorkflow && canonicalWorkflow(workflow) !== canonicalWorkflow(baseWorkflow));
  const updateDraft = (patch: Partial<CreateTaskDraftState>) => {
    onDraftChange((current) => ({ ...current, ...patch }));
  };
  const hasEditedDraft = Boolean(
    title.trim()
    || description.trim()
    || requirementFileName.trim()
    || requirementContent.trim()
    || saveTemplateName.trim()
    || workflowDirty
  );

  useEffect(() => {
    if (!workflowNotice) return;
    const timeout = window.setTimeout(() => setWorkflowNotice(null), 3000);
    return () => window.clearTimeout(timeout);
  }, [workflowNotice]);

  useEffect(() => {
    if (!open) return;
    setFormError(null);
    setWorkflowError(null);
    setWorkflowNotice(null);
    Promise.all([getAgentRegistry(), getWorkflowTemplates(), getProfiles()])
      .then(([registry, templates, profiles]) => {
        setAgentRegistry(registry);
        setTemplateStore(templates);
        setProfileList(profiles);
        if (!draft.initialized) {
          const selectedTemplate = templates.templates[0] ?? null;
          const initialWorkflow = selectedTemplate?.workflow ?? templates.lastCreatedWorkflow ?? null;
          onDraftChange((current) => ({
            ...current,
            selectedTemplateId: selectedTemplate?.id ?? null,
            lastUsedHintDismissed: false,
            baseWorkflow: initialWorkflow,
            workflow: initialWorkflow,
            saveTemplateName: '',
            initialized: true,
          }));
          if (!initialWorkflow) setFormError(t('taskList.create.noWorkflowTemplate'));
        } else if (!draft.workflow) {
          setFormError(t('taskList.create.noWorkflowTemplate'));
        }
      })
      .catch((err) => {
        setFormError(displayAppError(t, err));
        setWorkflowError(displayAppError(t, err));
      });
  }, [draft.initialized, onDraftChange, open, t]);

  const applyImportedRequirement = async (file: File) => {
    if (!/\.(txt|md)$/i.test(file.name)) {
      setFormError(t('taskList.create.invalidFile'));
      return;
    }
    const content = await file.text();
    updateDraft({
      requirementFileName: file.name,
      title: file.name.replace(/\.(txt|md)$/i, ''),
      description: '',
      requirementContent: content,
    });
    setFormError(null);
  };

  const clearRequirementFile = () => {
    updateDraft({ requirementFileName: '' });
    setFormError(null);
    if (requirementInputRef.current) requirementInputRef.current.value = '';
  };

  const hasRequirementDraft = Boolean(title.trim() || description.trim() || requirementContent.trim());

  const handleRequirementFileChange = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;
    if (hasRequirementDraft) {
      setPendingImportFile(file);
      setConfirmRequirementImportOpen(true);
      return;
    }
    await applyImportedRequirement(file);
  };

  const confirmRequirementImport = async () => {
    if (!pendingImportFile) return;
    const file = pendingImportFile;
    setConfirmRequirementImportOpen(false);
    setPendingImportFile(null);
    await applyImportedRequirement(file);
  };

  const selectWorkflowTemplate = (templateId: string) => {
    if (!templateStore) return;
    const template = templateStore.templates.find((item) => item.id === templateId);
    if (!template) return;
    updateDraft({
      selectedTemplateId: template.id,
      baseWorkflow: template.workflow,
      workflow: template.workflow,
      lastUsedHintDismissed: template.id === templateStore.lastUsedTemplateId,
      saveTemplateName: '',
      initialized: true,
    });
    setWorkflowError(null);
    setWorkflowNotice(null);
  };

  const startBlankWorkflowTemplate = () => {
    const blankWorkflow = createBlankWorkflowDraft();
    updateDraft({
      selectedTemplateId: null,
      baseWorkflow: blankWorkflow,
      workflow: blankWorkflow,
      lastUsedHintDismissed: false,
      saveTemplateName: '',
      initialized: true,
    });
    setTemplatePickerOpen(false);
    setFormError(null);
    setWorkflowError(null);
    setWorkflowNotice(null);
  };

  const applyDefaultWorkflow = (next: WorkflowDsl) => {
    const matchedTemplate = templateStore?.templates.find((template) => canonicalWorkflow(template.workflow) === canonicalWorkflow(next)) ?? null;
    updateDraft({
      selectedTemplateId: matchedTemplate?.id ?? null,
      baseWorkflow: next,
      workflow: next,
      saveTemplateName: '',
      initialized: true,
    });
    setWorkflowError(null);
    setWorkflowNotice(null);
  };

  const validateTemplateWorkflow = (workflowDraft: WorkflowDsl, validateTemplateDuplicateId = true) => {
    if (!agentRegistry || !profileList) {
      setWorkflowNotice(null);
      setWorkflowError(t('common.loading'));
      return null;
    }
    const validation = validateWorkflowForSave(workflowDraft, profileList.profiles, agentRegistry.agents.filter((agent) => agent.supported && agent.diagnostic?.available === true), t, templateStore ?? null, selectedTemplateId, selectedTemplate?.name ?? null, validateTemplateDuplicateId);
    if (!validation.valid) {
      setWorkflowNotice(null);
      setWorkflowError(validation.issues.map((issue) => issue.message).join('\n'));
      return null;
    }
    return validation.sanitizedWorkflow;
  };

  const saveCurrentAsTemplate = async () => {
    if (!workflow || !saveTemplateName.trim()) return;
    const validatedWorkflow = validateTemplateWorkflow(workflow, false);
    if (!validatedWorkflow) return;
    setSaving(true);
    try {
      const nextStore = await saveWorkflowTemplate(saveTemplateName.trim(), validatedWorkflow);
      const selected = nextStore.templates.at(-1) ?? null;
      const savedWorkflow = selected?.workflow ?? validatedWorkflow;
      setTemplateStore(nextStore);
      updateDraft({
        selectedTemplateId: selected?.id ?? nextStore.lastUsedTemplateId ?? null,
        baseWorkflow: savedWorkflow,
        workflow: savedWorkflow,
        saveTemplateName: '',
        initialized: true,
      });
      setWorkflowError(null);
      setWorkflowNotice(t('taskList.create.workflowTemplateSaved'));
    } catch (err) {
      setWorkflowNotice(null);
      setWorkflowError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  const saveCurrentTemplateChanges = async () => {
    if (!workflow || !selectedTemplateId || selectedTemplateId === 'default') return;
    const validatedWorkflow = validateTemplateWorkflow(workflow);
    if (!validatedWorkflow) return;
    setSaving(true);
    try {
      const nextStore = await updateWorkflowTemplate(selectedTemplateId, validatedWorkflow);
      const selected = nextStore.templates.find((template) => template.id === selectedTemplateId) ?? null;
      const savedWorkflow = selected?.workflow ?? validatedWorkflow;
      setTemplateStore(nextStore);
      updateDraft({
        baseWorkflow: savedWorkflow,
        workflow: savedWorkflow,
        saveTemplateName: '',
        initialized: true,
      });
      setWorkflowError(null);
      setWorkflowNotice(t('taskList.create.workflowTemplateUpdated'));
    } catch (err) {
      setWorkflowNotice(null);
      setWorkflowError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  const confirmDeleteWorkflowTemplate = async () => {
    if (!deleteTemplateTarget || deleteTemplateTarget.id === 'default') return;
    setSaving(true);
    try {
      const deletedTemplateId = deleteTemplateTarget.id;
      const nextStore = await deleteWorkflowTemplate(deletedTemplateId);
      const selected = selectedTemplateId === deletedTemplateId ? nextStore.templates[0] ?? null : nextStore.templates.find((template) => template.id === selectedTemplateId) ?? nextStore.templates[0] ?? null;
      setTemplateStore(nextStore);
      updateDraft({
        selectedTemplateId: selected?.id ?? null,
        baseWorkflow: selected?.workflow ?? null,
        workflow: selected?.workflow ?? null,
        saveTemplateName: '',
        initialized: true,
      });
      setDeleteTemplateTarget(null);
      setWorkflowError(null);
      setWorkflowNotice(t('taskList.create.workflowTemplateDeleted'));
    } catch (err) {
      setWorkflowNotice(null);
      setWorkflowError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  const defaultWorkflow = templateStore?.templates.find((template) => template.id === 'default')?.workflow ?? null;
  const selectedTemplate = templateStore?.templates.find((template) => template.id === selectedTemplateId) ?? null;
  const workflowTemplateLabel = selectedTemplate?.name ?? (workflow ? t('taskList.create.unsavedWorkflowTemplate') : t('taskList.create.workflowTemplatePlaceholder'));
  const canUpdateSelectedTemplate = Boolean(selectedTemplateId && selectedTemplateId !== 'default');
  const lastUsedTemplate = templateStore?.templates.find((template) => template.id === templateStore.lastUsedTemplateId) ?? null;
  const showLastUsedHint = Boolean(lastUsedTemplate && selectedTemplateId !== lastUsedTemplate.id && !lastUsedHintDismissed);

  const submit = async (workflowDraft: WorkflowDsl) => {
    if (!title.trim() || !requirementContent.trim()) {
      setFormError(t('taskList.create.requirementRequired'));
      return;
    }
    setSaving(true);
    try {
      setFormError(null);
      setWorkflowError(null);
      const created = await onCreateTask({
        title: title.trim(),
        description: description.trim() || null,
        requirementFileName: requirementFileName || null,
        requirementContent,
        workflow: workflowDraft,
        workflowTemplateId: selectedTemplateId,
      });
      if (!created) return;
    } catch (err) {
      setWorkflowNotice(null);
      setWorkflowError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  };

  const saveTaskFromHeader = async () => {
    if (!workflow) return;
    const validatedWorkflow = validateTemplateWorkflow(workflow);
    if (!validatedWorkflow) return;
    await submit(validatedWorkflow);
  };

  const requestOpenChange = (nextOpen: boolean) => {
    if (nextOpen) {
      updateDraft({ open: true });
      return;
    }
    if (hasEditedDraft) {
      setConfirmDiscardOpen(true);
      return;
    }
    onDraftChange(createInitialCreateTaskDraft());
  };

  const confirmDiscardDraft = () => {
    setConfirmDiscardOpen(false);
    setFormError(null);
    setWorkflowError(null);
    setWorkflowNotice(null);
    setPendingImportFile(null);
    setConfirmRequirementImportOpen(false);
    setTemplatePickerOpen(false);
    onDraftChange(createInitialCreateTaskDraft());
  };

  return (
    <Sheet open={open} onOpenChange={requestOpenChange}>
      <SheetContent className="gap-0 overflow-hidden p-0" resizeStorageKey="task-list/create-task" defaultSize={1120} minSize={760} maxSize={1440} closeLabel={t('common.close')}>
        <SheetHeader className="border-b px-5 py-4 text-left">
          <div className="flex items-start justify-between gap-3 pr-9">
            <div className="min-w-0 space-y-1">
              <SheetTitle>{t('taskList.create.title')}</SheetTitle>
              <SheetDescription>{t('taskList.create.description')}</SheetDescription>
            </div>
            {workflow ? <Button type="button" size="sm" className="shrink-0" disabled={saving} onClick={() => void saveTaskFromHeader()}>{saving ? t('taskList.create.savingTask') : t('taskList.create.saveTask')}</Button> : null}
          </div>
        </SheetHeader>
        <ScrollArea className="h-[calc(100vh-96px)]">
          <div className="space-y-4 p-5">
            <AppCard className="space-y-1 p-4">
              <div className="flex flex-col gap-1.5 border-b pb-1 sm:flex-row sm:items-start sm:justify-between">
                <div className="space-y-0.5">
                  <h3 className="text-sm font-semibold text-foreground">{t('taskList.create.requirementSectionTitle')}</h3>
                </div>
                <div className="flex flex-wrap items-center gap-2 sm:justify-end">
                  {requirementFileName ? <Badge variant="outline" className="max-w-full truncate">{requirementFileName}</Badge> : null}
                  <Button type="button" variant="outline" size="sm" className="shrink-0" onClick={() => requirementInputRef.current?.click()}>
                    <Upload className="size-4" />
                    {t('taskList.create.importRequirement')}
                  </Button>
                  {requirementFileName ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="size-8"
                      aria-label={t('taskList.create.removeFile')}
                      onClick={(event) => {
                        event.preventDefault();
                        event.stopPropagation();
                        clearRequirementFile();
                      }}
                    >
                      <X className="size-4" />
                    </Button>
                  ) : null}
                </div>
              </div>
              <div className="grid gap-3 pt-0.5 lg:grid-cols-[minmax(280px,0.8fr)_minmax(0,1.2fr)] lg:items-start lg:gap-5">
                <div className="grid gap-2 text-sm">
                  <div className="grid gap-1">
                    <Label className="text-xs font-medium text-muted-foreground">{t('taskList.create.taskTitle')}</Label>
                    <Input className="h-10" value={title} onChange={(event) => updateDraft({ title: event.target.value })} placeholder={t('taskList.create.taskTitlePlaceholder')} />
                  </div>
                  <div className="grid gap-1.5">
                    <Label className="text-xs font-medium text-muted-foreground">{t('taskList.create.taskDescription')}</Label>
                    <Textarea className="min-h-36 resize-none" value={description} onChange={(event) => updateDraft({ description: event.target.value })} placeholder={t('taskList.create.taskDescriptionPlaceholder')} />
                  </div>
                </div>
                <div className="grid gap-1 text-sm">
                  <Label className="text-xs font-medium text-muted-foreground">{t('taskList.create.requirementContent')}</Label>
                  <Input
                    ref={requirementInputRef}
                    className="sr-only"
                    type="file"
                    accept=".txt,.md,text/plain,text/markdown"
                    onChange={(event) => void handleRequirementFileChange(event)}
                  />
                  <Textarea
                    value={requirementContent}
                    onChange={(event) => updateDraft({ requirementContent: event.target.value })}
                    placeholder={t('taskList.create.requirementContentPlaceholder')}
                    className="min-h-[220px] bg-background"
                  />
                </div>
              </div>
            </AppCard>
            {formError ? <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">{formError}</div> : null}
            {workflow ? (
              <div className="space-y-3">
                <AppCard className="flex flex-col gap-3 p-3 lg:flex-row lg:items-center lg:justify-between">
                  <div className="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center">
                    <span className="text-xs font-medium text-muted-foreground">{t('taskList.create.workflowTemplate')}</span>
                    <Popover open={templatePickerOpen} onOpenChange={setTemplatePickerOpen}>
                      <PopoverTrigger asChild>
                        <Button type="button" variant="outline" className="w-full justify-between sm:w-64" aria-label={t('taskList.create.workflowTemplate')}>
                          <span className="truncate">{workflowTemplateLabel}</span>
                          <ChevronDown className="size-4 opacity-60" />
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent align="start" className="w-72 p-1">
                        <div className="space-y-1">
                          <Button type="button" variant="ghost" className="h-9 w-full justify-start gap-2 px-2" onClick={startBlankWorkflowTemplate}>
                            <Plus className="size-4" />
                            {t('taskList.create.newWorkflowTemplate')}
                          </Button>
                          <div className="my-1 border-t" />
                          {templateStore?.templates.map((template) => {
                            const isDefaultTemplate = template.id === 'default';
                            const selected = template.id === selectedTemplateId;
                            return (
                              <div key={template.id} className={cn('flex items-center gap-1 rounded-md p-1', selected && 'bg-accent text-accent-foreground')}>
                                <button
                                  type="button"
                                  className="flex min-h-9 min-w-0 flex-1 items-center justify-between gap-2 rounded-sm px-2 text-left text-sm outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                                  onClick={() => {
                                    selectWorkflowTemplate(template.id);
                                    setTemplatePickerOpen(false);
                                  }}
                                >
                                  <span className="truncate">{template.name}</span>
                                  {selected ? <Check className="size-4 shrink-0" /> : null}
                                </button>
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon"
                                  className="size-8 shrink-0 text-muted-foreground hover:text-destructive"
                                  disabled={isDefaultTemplate || saving}
                                  aria-label={isDefaultTemplate ? t('taskList.create.defaultWorkflowReadonly') : t('taskList.create.deleteWorkflowTemplate', { name: template.name })}
                                  onClick={() => {
                                    if (isDefaultTemplate) return;
                                    setTemplatePickerOpen(false);
                                    setDeleteTemplateTarget(template);
                                  }}
                                >
                                  <Trash2 className="size-4" />
                                </Button>
                              </div>
                            );
                          })}
                        </div>
                      </PopoverContent>
                    </Popover>
                    {showLastUsedHint && lastUsedTemplate ? (
                      <span className="text-xs text-muted-foreground">
                        {t('taskList.create.lastUsedWorkflowPrefix')}
                        <button
                          type="button"
                          className="font-medium text-foreground underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                          aria-label={t('taskList.create.selectLastUsedWorkflow', { name: lastUsedTemplate.name })}
                          onClick={() => selectWorkflowTemplate(lastUsedTemplate.id)}
                        >
                          {lastUsedTemplate.name}
                        </button>
                      </span>
                    ) : null}
                    {workflowDirty ? <Badge variant="outline">{t('taskList.create.workflowDirty')}</Badge> : null}
                  </div>
                  {workflowDirty ? (
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                      {canUpdateSelectedTemplate ? <Button variant="outline" size="sm" disabled={saving} onClick={() => void saveCurrentTemplateChanges()}>{saving ? t('taskList.create.savingWorkflowTemplate') : t('taskList.create.saveCurrentWorkflow')}</Button> : null}
                      <Input className="sm:w-52" value={saveTemplateName} placeholder={t('taskList.create.workflowTemplateName')} onChange={(event) => updateDraft({ saveTemplateName: event.target.value })} />
                      <Button variant="outline" size="sm" disabled={!saveTemplateName.trim() || saving} onClick={() => void saveCurrentAsTemplate()}>{t('taskList.create.saveAsWorkflow')}</Button>
                    </div>
                  ) : null}
                </AppCard>
                {workflowError ? (
                  <div className="flex items-start justify-between gap-3 rounded-lg border border-destructive/25 bg-destructive/5 px-3 py-2 text-sm text-destructive">
                    <span className="min-w-0 whitespace-pre-wrap break-words">{workflowError}</span>
                    <Button type="button" variant="ghost" size="icon" className="-mr-2 -mt-1 size-7 shrink-0 text-destructive/70 hover:bg-destructive/10 hover:text-destructive" aria-label={t('common.close')} onClick={() => setWorkflowError(null)}>
                      <X className="size-4" />
                    </Button>
                  </div>
                ) : null}
                {workflowNotice ? <div className="rounded-lg border border-emerald-500/20 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-700 dark:text-emerald-300">{workflowNotice}</div> : null}
                <WorkflowEditor
                  value={workflow}
                  agentRegistry={agentRegistry}
                  profiles={profileList?.profiles ?? []}
                  onOpenProfileManagement={onOpenProfileManagement}
                  defaultWorkflow={defaultWorkflow}
                  workflowTemplates={templateStore}
                  currentTemplateId={selectedTemplateId}
                  currentTemplateName={selectedTemplate?.name ?? null}
                  allowAiDynamic
                  saving={saving}
                  onChange={(next) => {
                    updateDraft({ workflow: next, initialized: true });
                    setWorkflowError(null);
                    setWorkflowNotice(null);
                  }}
                  onApplyDefaultTemplate={applyDefaultWorkflow}
                  onSave={submit}
                  showSaveAction={false}
                />
              </div>
            ) : <EmptyState>{templateStore ? t('taskList.create.noWorkflowTemplate') : t('common.loading')}</EmptyState>}
          </div>
        </ScrollArea>
      </SheetContent>
      <AlertDialog open={confirmRequirementImportOpen} onOpenChange={(open) => {
        setConfirmRequirementImportOpen(open);
        if (!open) setPendingImportFile(null);
      }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('taskList.create.confirmImportTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('taskList.create.confirmImportDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => void confirmRequirementImport()}>{t('taskList.create.confirmImportAction')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={confirmDiscardOpen} onOpenChange={setConfirmDiscardOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('taskList.create.confirmDiscardTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('taskList.create.confirmDiscardDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDiscardDraft}>{t('taskList.create.confirmDiscardAction')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog open={Boolean(deleteTemplateTarget)} onOpenChange={(open) => !open && setDeleteTemplateTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('taskList.create.deleteWorkflowTemplateTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('taskList.create.deleteWorkflowTemplateDescription', { name: deleteTemplateTarget?.name ?? '' })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction disabled={saving} onClick={() => void confirmDeleteWorkflowTemplate()}>{t('taskList.create.deleteWorkflowTemplateAction')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Sheet>
  );
}

function TaskPagination({ pageIndex, pageCount, onPageChange }: { pageIndex: number; pageCount: number; onPageChange: (value: number) => void }) {
  const { t } = useTranslation();
  const previousDisabled = pageIndex === 0;
  const nextDisabled = pageIndex >= pageCount - 1;
  return (
    <Pagination className="w-auto">
      <PaginationContent>
        <PaginationItem>
          <PaginationLink
            href="#"
            size="default"
            aria-disabled={previousDisabled}
            className={cn('px-3', previousDisabled && 'pointer-events-none opacity-50')}
            onClick={(event) => { event.preventDefault(); if (!previousDisabled) onPageChange(Math.max(0, pageIndex - 1)); }}
          >
            {t('common.previousPage')}
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink href="#" isActive aria-label={`Page ${pageIndex + 1}`}>
            {pageIndex + 1}
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink
            href="#"
            size="default"
            aria-disabled={nextDisabled}
            className={cn('px-3', nextDisabled && 'pointer-events-none opacity-50')}
            onClick={(event) => { event.preventDefault(); if (!nextDisabled) onPageChange(Math.min(pageCount - 1, pageIndex + 1)); }}
          >
            {t('common.nextPage')}
          </PaginationLink>
        </PaginationItem>
      </PaginationContent>
    </Pagination>
  );
}

function canonicalWorkflow(workflow: WorkflowDsl) {
  return JSON.stringify(workflow);
}

function createBlankWorkflowDraft(): WorkflowDsl {
  return {
    version: '0.1',
    id: `workflow-${Date.now().toString(36)}`,
    entry: '',
    control: {},
    nodes: [],
    edges: [],
  };
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
  if (key === 'workflow') return workflowStatusValue(task);
  if (key === 'latest') return task.latestRun?.id ?? '';
  return task.id;
}

function matchesTaskFilter(task: TaskRowVm, filter: TaskFilter) {
  const display = normalizeStatusValue(task.displayStatus);
  const runStatus = normalizeStatusValue(task.latestRun?.status);
  const runOutcome = normalizeStatusValue(task.latestRun?.outcome);
  if (filter === 'running') return display === 'running' || runStatus === 'running';
  if (filter === 'completed') return ['completed', 'complete', 'success', 'succeeded'].includes(display) || ['success', 'succeeded'].includes(runOutcome);
  if (filter === 'resumable') return display === 'resumable' || Boolean(task.resumableRunId) || Boolean(task.latestRun?.resumable);
  if (filter === 'failed') return ['failed', 'failure'].includes(display) || ['failed', 'failure'].includes(runOutcome);
  if (filter === 'invalid') return ['invalid', 'missing-workflow', 'missing'].includes(display) || !task.workflowExists || !task.workflowValid;
  return true;
}

function taskSearchText(task: TaskRowVm, t: TFunction) {
  const workflowStatus = workflowStatusValue(task);
  return [
    task.id,
    task.title,
    task.description,
    task.requirementPreview,
    task.requirement,
    task.displayStatus,
    displayStatus(t, task.displayStatus),
    displayWorkflowError(t, task.workflowError),
    workflowStatus,
    displayStatus(t, workflowStatus),
    task.latestRun?.id,
    task.latestRun?.status,
    task.latestRun?.outcome,
    task.resumableRunId,
  ]
    .filter(Boolean)
    .join('\n')
    .toLowerCase();
}

function workflowStatusValue(task: TaskRowVm) {
  if (!task.workflowExists) return 'missing-workflow';
  if (!task.workflowValid) return 'invalid';
  return 'valid';
}

function normalizeStatusValue(value?: string | null) {
  return value?.toLowerCase() ?? '';
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
        <TaskTableSkeleton />
      </div>
    </div>
  );
}

function TaskPreviewSheet({ task, open, onOpenChange, onNavigate }: { task: TaskRowVm | null; open: boolean; onOpenChange: (open: boolean) => void; onNavigate: (page: TaskPage) => void }) {
  const { t } = useTranslation();
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent
        className="gap-0 overflow-hidden p-0"
        resizeStorageKey="task-list/task-preview"
        defaultSize={440}
        minSize={360}
        maxSize={720}
        closeLabel={t('common.close')}
        onInteractOutside={(event) => {
          const target = event.target as HTMLElement | null;
          if (target?.closest('[data-task-preview-row="true"]')) event.preventDefault();
        }}
        showOverlay={false}
      >
        {task ? <TaskPreviewContent task={task} onNavigate={onNavigate} /> : null}
      </SheetContent>
    </Sheet>
  );
}

function TaskPreviewContent({ task, onNavigate }: { task: TaskRowVm; onNavigate: (page: TaskPage) => void }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const requirement = fullRequirementText(task.requirement, task.requirementPreview || task.description, t('common.empty'));

  const copyRequirement = async () => {
    await navigator.clipboard.writeText(requirement);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <>
      <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
        <p className="text-xs font-semibold uppercase tracking-[0.18em] text-primary">{t('taskList.taskPreview')}</p>
        <SheetDescription className="sr-only">{t('taskList.taskPreviewDescription')}</SheetDescription>
        <div className="flex min-w-0 flex-wrap gap-2 pr-8">
          <Badge variant="secondary" className="max-w-full truncate">{task.id}</Badge>
          <StatusBadge value={task.displayStatus} label={displayStatus(t, task.displayStatus)} />
        </div>
        <SheetTitle className="line-clamp-2 break-words text-xl">{task.title}</SheetTitle>
      </SheetHeader>
      <div className="flex min-h-0 flex-1 flex-col p-5">
        <div className="flex shrink-0 items-center justify-between gap-3 pb-4">
          <h3 className="font-semibold text-foreground">{t('common.fullRequirement')}</h3>
          <Button variant="ghost" size="icon" className="h-8 w-8 text-muted-foreground hover:text-foreground" aria-label={t('common.copy')} onClick={copyRequirement}>
            {copied ? <Check className="h-4 w-4 text-primary" /> : <Copy className="h-4 w-4" />}
          </Button>
        </div>
        <ScrollArea className="min-h-0 flex-1">
          <CodeBlock className="whitespace-pre-wrap font-sans text-sm leading-7">{requirement}</CodeBlock>
        </ScrollArea>
      </div>
      <SheetFooter className="shrink-0 border-t bg-background/95 p-5">
        <Button className="w-full shadow-sm" onClick={() => onNavigate({ kind: 'workflow', taskId: task.id })}>{t('common.workflow')}</Button>
      </SheetFooter>
    </>
  );
}
