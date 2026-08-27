import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ScheduledTaskManagementPage, scheduledWorkspaceFilterTriggerClassName } from '@/pages/ScheduledTaskManagementPage';
import { TooltipProvider } from '@/components/ui/tooltip';
import i18n from '@/i18n';

describe('ScheduledTaskManagementPage', () => {
  it('renders the global management surface with workspace filtering', async () => {
    await i18n.changeLanguage('zh-CN');
    const html = renderToStaticMarkup(React.createElement(
      TooltipProvider,
      null,
      React.createElement(ScheduledTaskManagementPage, { projectId: 'project-a' }),
    ));

    expect(html).toContain('定时任务');
    expect(html).not.toContain('按计划执行并追踪最近一次运行');
    expect(html).toContain('全部工作区');
  });

  it('keeps the workspace filter width stable while workspace options load', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');

    expect(scheduledWorkspaceFilterTriggerClassName).toContain('w-28');
    expect(source).toContain('<Select value={workspaceFilter} onValueChange={setWorkspaceFilter}>');
    expect(source).toContain('className={scheduledWorkspaceFilterTriggerClassName}');
    expect(source).toContain("workspaceFilter === 'all' ? t('scheduled.management.allWorkspaces') : workspaceFilter");
    expect(source).not.toContain('<select');
  });

  it('uses theme foreground tokens for task row icons', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');

    expect(source).toContain('bg-foreground/10 text-foreground');
    expect(source).not.toContain('bg-primary/10 text-primary"><AlarmClock');
  });

  it('subscribes to task updates and exposes run-now and edit controls', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('subscribeScheduledTaskUpdates');
    expect(source).toContain('runScheduledTaskNow');
    expect(source).toContain('onOpenDetail');
    expect(source).toContain("t('scheduled.management.edit')");
    expect(source).toContain("t('scheduled.management.delete')");
    expect(source).toContain("t('scheduled.management.refresh')");
    expect(source).toContain("t('scheduled.management.create')");
    // Detail controls moved to the dedicated detail page
    expect(source).not.toContain('getScheduledTaskDiagnostics');
    expect(source).not.toContain('subscribeScheduledOccurrenceUpdates');
    expect(source).toContain('<Sheet open={Boolean(editing)}');
    expect(source).toContain('resizeStorageKey="scheduled-task-management/edit"');
    expect(source).toContain('presentation="workspace"');
  });

  it('keeps existing rows on refresh errors and exposes per-task pending state', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');

    expect(source).toContain('setLoadError(true)');
    expect(source).not.toContain('.catch(() => setTasks([]))');
    expect(source).toContain('pendingTaskActions');
    expect(source).toContain('pendingTaskActionsRef');
    expect(source).toContain('taskListRequestIdRef');
    expect(source).toContain('taskMutationGenerationRef');
    expect(source).toContain("filter === 'enabled'");
    expect(source).not.toContain("filter === 'running'");
  });

  it('uses the full workspace width and a responsive row contract', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('<Page flush className="flex flex-col">');
    expect(source).toContain('<PageHeader');
    expect(source).toContain('variant="integrated"');
    expect(source).toContain('min-h-0 flex-1 overflow-y-auto px-6 pb-6 pt-4');
    expect(source).toContain('md:grid-cols-[minmax(0,1.35fr)');
    expect(source).toContain('md:hidden');
    expect(source).not.toContain('max-w-6xl');
    expect(source).not.toContain('min-w-[980px]');
  });
});

describe('ScheduledTaskDetailPage', () => {
  it('loads Run history with a complete locator and local deletion state', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskDetailPage.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('listScheduledExecutionHistory');
    expect(source).toContain('deleteScheduledExecutionHistory');
    expect(source).toContain("t('scheduled.detail.back')");
    expect(source).toContain("t('scheduled.detail.history')");
    expect(source).toContain('scheduledHistoryTarget');
    expect(source).toContain('ScheduledExecutionHistoryVm');
    expect(source).toContain("t('scheduled.detail.deleted')");
    expect(source).toContain('const [selectedRuns, setSelectedRuns]');
    expect(source).toContain("result.status === 'completed'");
    expect(source).toContain('code: result.code');
    expect(source).toContain('displayAppError(t, { code: state.code, params: state.params ?? {} })');
    expect(source).toContain('setSelectedRuns(new Set());');
    expect(source).not.toContain('listScheduledTaskOccurrences');
    expect(source).not.toContain('occurrenceRetentionDays');
    expect(source).toContain('<Sheet open={Boolean(editing)}');
    expect(source).toContain('resizeStorageKey="scheduled-task-detail/edit"');
    expect(source).toContain('<ScheduledTaskDialog');
    expect(source).toContain('await updateScheduledTask({');
    expect(source).toContain('setEditing(null);');
    expect(source).toContain('getScheduledTaskDiagnostics');
    expect(source).toContain('subscribeScheduledTaskUpdates');
    expect(source).toContain('subscribeScheduledOccurrenceUpdates');
    expect(source).toContain("getScheduledTaskDiagnostics(request.projectId, request.scheduledTaskId).catch(() => null)");
    expect(source).not.toContain('Promise.allSettled');
    expect(source).toContain('void listScheduledTasks(pid)');
    expect(source).toContain("initialLocation.kind === 'anchor'");
    expect(source).toContain('event.projectId !== (task?.projectId ?? projectId)');
    expect(source).toContain('if (taskId && runId) return;');
    expect(source).toContain('historyDeleteInFlightRef.current');
    expect(source).toContain('generation !== historyMutationGenerationRef.current');
    expect(source).toContain('executionHistoryKey(item)');
    expect(source).toContain('displayAppError(t, item.error)');
    expect(source).toContain('<RunHistorySection readOnly');
    expect(source).toContain("aria-label={t('scheduled.detail.history')}");
    expect(source).toContain("aria-label={t('scheduled.detail.selectAllRuns')}");
    expect(source).toContain('executionHistoryStatusLabel(t, item)');
    expect(source).toContain('const effectiveProjectId = task?.projectId ?? projectId;');
    expect(source).toContain("type HistoryPageLocation =");
    expect(source).not.toContain("pid || 'default'");
  });

  it('localizes execution history availability and selection controls', async () => {
    await i18n.changeLanguage('zh-CN');
    expect(i18n.t('scheduled.detail.historyAvailability.available')).toBe('可打开');
    expect(i18n.t('scheduled.detail.historyAvailability.unavailable')).toBe('不可用');
    expect(i18n.t('scheduled.detail.selectRun', { summary: '每日摘要' })).toBe('选择运行：每日摘要');

    await i18n.changeLanguage('en');
    expect(i18n.t('scheduled.detail.historyAvailability.available')).toBe('Available');
    expect(i18n.t('scheduled.detail.selectAllRuns')).toBe('Select all runs on this page');
  });

  it('shows structured elicitation submission failures instead of silently reopening the question', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/components/acp/ACPChatDialog.tsx', import.meta.url)), 'utf8');

    expect(source).toContain('setSendError(displayAppError(t, error))');
  });
});
