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

  it('does not subscribe to scheduler events and exposes manual CRUD controls', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const source = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');
    expect(source).not.toContain('subscribeScheduledTaskUpdates');
    expect(source).toContain('编辑任务');
    expect(source).toContain('删除任务');
    expect(source).toContain('刷新定时任务');
    expect(source).toContain('创建定时任务');
  });
});
