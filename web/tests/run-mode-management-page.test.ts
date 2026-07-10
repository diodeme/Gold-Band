import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { autoNoticeAutoDismiss, autoSaveTarget, createBlankWorkflowTemplateEditorState, RunModeTabsToolbar, TemplateActionRow } from '@/pages/RunModeManagementPage';

describe('RunModeTabsToolbar', () => {
  it('renders only mode tabs because mode changes are applied immediately', () => {
    const html = renderToStaticMarkup(
      React.createElement(RunModeTabsToolbar, {
        mode: 'auto',
        onModeChange: () => undefined,
        workflowLabel: '工作流模板',
        autoLabel: 'AUTO 设置',
      }),
    );

    expect(html).toContain('data-testid="run-mode-tabs-toolbar"');
    expect(html).toContain('工作流模板');
    expect(html).toContain('AUTO 设置');
    expect(html).not.toContain('保存</button>');
    expect(html).not.toContain('已保存');
  });

  it('creates an editable blank workflow draft for a new workflow template', () => {
    const draft = createBlankWorkflowTemplateEditorState();

    expect(draft.templateId).toBeNull();
    expect(draft.saveName).toBe('');
    expect(draft.workflow).toMatchObject({
      version: '0.1',
      entry: '',
      control: {},
      nodes: [],
      edges: [],
    });
    expect(draft.workflow.id).toMatch(/^workflow-/);
  });

  it('saves AUTO changes to run mode when no template is selected', () => {
    expect(autoSaveTarget('')).toBe('run-mode');
    expect(autoSaveTarget(null)).toBe('run-mode');
    expect(autoSaveTarget('auto-template-1')).toBe('template');
  });

  it('auto-dismisses successful AUTO notices but keeps errors visible', () => {
    expect(autoNoticeAutoDismiss('success')).toBe(true);
    expect(autoNoticeAutoDismiss('error')).toBe(false);
  });

  it('renders the shared template action row in picker-save-name-save-as order', () => {
    const html = renderToStaticMarkup(
      React.createElement(TemplateActionRow, {
        label: 'AUTO 模板',
        picker: React.createElement('button', null, '不使用模板'),
        saving: false,
        saveCurrentLabel: '保存修改',
        savingLabel: '保存中…',
        onSaveCurrent: () => undefined,
        name: '',
        namePlaceholder: '模板名称',
        onNameChange: () => undefined,
        saveAsLabel: '另存模板',
        onSaveAs: () => undefined,
      }),
    );

    expect(html).toContain('data-testid="template-action-row"');
    expect(html.indexOf('不使用模板')).toBeLessThan(html.indexOf('保存修改'));
    expect(html.indexOf('保存修改')).toBeLessThan(html.indexOf('模板名称'));
    expect(html.indexOf('模板名称')).toBeLessThan(html.indexOf('另存模板'));
    expect(html.match(/data-variant="default"/g)?.length).toBe(2);
  });
});
