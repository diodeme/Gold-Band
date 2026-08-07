import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ScheduledTaskManagementPage } from '@/pages/ScheduledTaskManagementPage';

describe('ScheduledTaskManagementPage', () => {
  it('renders the global management surface with workspace filtering', () => {
    const html = renderToStaticMarkup(React.createElement(ScheduledTaskManagementPage, { projectId: 'project-a' }));

    expect(html).toContain('定时任务');
    expect(html).toContain('按计划执行并追踪最近一次运行');
    expect(html).toContain('全部工作区');
  });

  it('subscribes to task updates and exposes run-now and edit controls', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('subscribeScheduledTaskUpdates');
    expect(source).toContain('runScheduledTaskNow');
    expect(source).toContain('onOpenDetail');
    expect(source).toContain('编辑任务');
    expect(source).toContain('删除任务');
    expect(source).toContain('刷新定时任务');
    expect(source).toContain('创建定时任务');
    // Detail controls moved to the dedicated detail page
    expect(source).not.toContain('getScheduledTaskDiagnostics');
    expect(source).not.toContain('subscribeScheduledOccurrenceUpdates');
  });
});

describe('ScheduledTaskDetailPage', () => {
  it('loads diagnostics and occurrence history scoped to a single task', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskDetailPage.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('getScheduledTaskDiagnostics');
    expect(source).toContain('listScheduledTaskOccurrences');
    expect(source).toContain('subscribeScheduledTaskUpdates');
    expect(source).toContain('subscribeScheduledOccurrenceUpdates');
    expect(source).toContain('返回定时任务');
    expect(source).toContain('执行历史');
    // Event handlers only react to the matching task id
    expect(source).toContain("event.scheduledTaskId !== scheduledTaskId");
  });
});
