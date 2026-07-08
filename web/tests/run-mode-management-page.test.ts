import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { RunModeTabsToolbar } from '@/pages/RunModeManagementPage';

describe('RunModeTabsToolbar', () => {
  it('keeps the page save action beside the mode tabs', () => {
    const html = renderToStaticMarkup(
      React.createElement(RunModeTabsToolbar, {
        mode: 'auto',
        onModeChange: () => undefined,
        onSave: () => undefined,
        saved: true,
        workflowLabel: '工作流模板',
        autoLabel: 'AUTO 设置',
        saveLabel: '保存',
        savedLabel: '已保存',
      }),
    );

    expect(html).toContain('data-testid="run-mode-tabs-toolbar"');
    expect(html).toContain('justify-between');
    expect(html.indexOf('工作流模板')).toBeLessThan(html.indexOf('保存'));
    expect(html.indexOf('AUTO 设置')).toBeLessThan(html.indexOf('保存'));
    expect(html).toContain('已保存');
  });
});
