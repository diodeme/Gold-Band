import React from 'react';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import type { WorkflowDsl } from '@/types';
import { autoNoticeAutoDismiss, autoNoticeDismissDelay, autoSaveTarget, createBlankAutoTemplateEditorState, createBlankWorkflowTemplateEditorState, findSavedWorkflowTemplate, isNoAutoTemplateSelected, RunModeManagementPage, RunModeProjectSelector, RunModeTabsToolbar, templatePickerSavedListClass, TemplateActionRow } from '@/pages/RunModeManagementPage';
import { pruneMissingAutoAllowedProfileIds, pruneMissingAutoAllowedWorkflowIds, pruneMissingAutoConfigReferences } from '@/lib/run-mode-validation';

const pageSource = readFileSync(fileURLToPath(new URL('../src/pages/RunModeManagementPage.tsx', import.meta.url)), 'utf8');

describe('RunModeTabsToolbar', () => {
  it('renders a title-only page header, without a mode description or duplicate back action', () => {
    const html = renderToStaticMarkup(
      React.createElement(RunModeManagementPage, {
        projectId: 'project-a',
        workspaceName: 'Project A',
        workspaces: [{ projectId: 'project-a', name: 'Project A', workspacePath: 'D:/a' }],
        runMode: { mode: 'auto', workflowTemplateId: null, autoConfig: null },
        agentRegistry: null,
        workflowTemplates: null,
        onProjectChange: () => undefined,
        onSave: () => undefined,
      }),
    );
    const header = html.match(/<header[\s\S]*?<\/header>/)?.[0] ?? '';

    expect(header).toContain('<h1');
    expect(header).not.toContain('text-muted-foreground');
    expect(header).not.toContain('<button');
  });

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
    expect(html).not.toContain('Direct');
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

  it('keeps workflow profile validation and template saves pending until profiles load', () => {
    expect(pageSource).toContain('profileCatalogReady={profilesLoaded}');
    expect(pageSource).toContain('saveCurrentDisabled={!profilesLoaded || wfSaveCurrentDisabled}');
    expect(pageSource).toContain('saveAsDisabled={!profilesLoaded}');
  });

  it('creates an independent empty AUTO draft for a new template', () => {
    expect(createBlankAutoTemplateEditorState()).toMatchObject({
      agentStrategy: 'fixed',
      agentType: '',
      allowedWorkflows: [],
      allowedProfiles: [],
      activeTemplateId: '',
      activeTemplateName: '',
      control: { maxDynamicNodes: 20, maxWorkflowInvocations: 10 },
    });
  });

  it('resolves a newly saved workflow by its name instead of template array position', () => {
    const templates = {
      version: '0.1',
      lastUsedTemplateId: 'default',
      lastCreatedWorkflow: null,
      templates: [
        { id: 'new-template', name: '新增工作流', workflow: { id: 'workflow-new' } as WorkflowDsl, createdAt: '', updatedAt: '' },
        { id: 'default', name: '默认完整工作流', isBuiltIn: true, workflow: { id: 'workflow-default' } as WorkflowDsl, createdAt: '', updatedAt: '' },
      ],
    };

    expect(findSavedWorkflowTemplate(templates, '新增工作流')?.id).toBe('new-template');
  });

  it('renders the current project as the selected run mode scope', () => {
    const html = renderToStaticMarkup(
      React.createElement(RunModeProjectSelector, {
        projectId: 'project-b',
        workspaceName: 'Project B',
        label: '项目',
        workspaces: [
          { projectId: 'project-a', name: 'Project A', workspacePath: 'D:/a' },
          { projectId: 'project-b', name: 'Project B', workspacePath: 'D:/b' },
        ],
        onProjectChange: () => undefined,
      }),
    );

    expect(html).toContain('data-testid="run-mode-project-selector"');
    expect(html).toContain('项目');
    expect(html).toContain('Project B');
  });

  it('saves AUTO changes to run mode when no template is selected', () => {
    expect(autoSaveTarget('')).toBe('run-mode');
    expect(autoSaveTarget(null)).toBe('run-mode');
    expect(autoSaveTarget('auto-template-1')).toBe('template');
  });

  it('does not select “no template” while a new AUTO template draft is active', () => {
    expect(isNoAutoTemplateSelected('', true)).toBe(false);
    expect(isNoAutoTemplateSelected('', false)).toBe(true);
    expect(isNoAutoTemplateSelected('saved-template', false)).toBe(false);
  });

  it('limits both template pickers to a scrollable saved-template list', () => {
    expect(templatePickerSavedListClass).toContain('max-h-64');
    expect(templatePickerSavedListClass).toContain('overflow-auto');
  });

  it('auto-dismisses successful and warning AUTO notices but keeps errors visible', () => {
    expect(autoNoticeAutoDismiss('success')).toBe(true);
    expect(autoNoticeAutoDismiss('error')).toBe(false);
    expect(autoNoticeAutoDismiss('warning')).toBe(true);
    expect(autoNoticeDismissDelay('success')).toBe(3000);
    expect(autoNoticeDismissDelay('warning')).toBe(5000);
  });

  it('removes only AUTO workflow references that no longer resolve', () => {
    const result = pruneMissingAutoAllowedWorkflowIds(
      ['task-workflow', 'missing-workflow', '  custom-workflow  '],
      {
        version: '0.1',
        lastUsedTemplateId: 'default',
        lastCreatedWorkflow: null,
        templates: [
          { id: 'default', name: '默认完整工作流', isBuiltIn: true, workflow: { id: 'task-workflow' } as WorkflowDsl, createdAt: '', updatedAt: '' },
          { id: 'custom', name: '我的工作流', workflow: { id: 'custom-workflow' } as WorkflowDsl, createdAt: '', updatedAt: '' },
        ],
      },
    );

    expect(result).toEqual({
      workflowIds: ['task-workflow', 'custom-workflow'],
      removedWorkflowIds: ['missing-workflow'],
    });
  });

  it('removes only AUTO profile references that no longer resolve', () => {
    const result = pruneMissingAutoAllowedProfileIds(
      ['profile-plan', 'missing-profile', ' profile-dev '],
      [
        { id: 'profile-plan', name: '规划', content: '', isBuiltIn: false },
        { id: 'profile-dev', name: '开发', content: '', isBuiltIn: false },
      ],
    );

    expect(result).toEqual({
      profileIds: ['profile-plan', 'profile-dev'],
      removedProfileIds: ['missing-profile'],
    });
  });

  it('returns a cleaned AUTO template config when its workflow or profile reference is missing', () => {
    const result = pruneMissingAutoConfigReferences(
      {
        agentType: 'claude-acp',
        allowedWorkflows: [{ workflowId: 'missing-workflow' }, { workflowId: 'task-workflow' }],
        allowedProfiles: ['missing-profile', 'profile-plan'],
      },
      {
        version: '0.1',
        lastUsedTemplateId: 'default',
        lastCreatedWorkflow: null,
        templates: [{ id: 'default', name: '默认完整工作流', isBuiltIn: true, workflow: { id: 'task-workflow' } as WorkflowDsl, createdAt: '', updatedAt: '' }],
      },
      [{ id: 'profile-plan', name: '规划', content: '', isBuiltIn: false }],
    );

    expect(result.removedWorkflowIds).toEqual(['missing-workflow']);
    expect(result.removedProfileIds).toEqual(['missing-profile']);
    expect(result.config.allowedWorkflows).toEqual([{ workflowId: 'task-workflow' }]);
    expect(result.config.allowedProfiles).toEqual(['profile-plan']);
  });

  it('renders the shared template action row in picker-save-name-save-as order', () => {
    const html = renderToStaticMarkup(
      React.createElement(TemplateActionRow, {
        label: 'AUTO 模板',
        picker: React.createElement('button', null, '不使用模板'),
        notice: '默认工作流不可覆盖，请另存为新的工作流。',
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
    expect(html).toContain('data-testid="template-action-notice"');
    expect(html).toContain('role="status"');
    expect(html).toContain('默认工作流不可覆盖，请另存为新的工作流。');
    expect(html.indexOf('不使用模板')).toBeLessThan(html.indexOf('保存修改'));
    expect(html.indexOf('保存修改')).toBeLessThan(html.indexOf('模板名称'));
    expect(html.indexOf('模板名称')).toBeLessThan(html.indexOf('另存模板'));
    expect(html.match(/data-variant="default"/g)?.length).toBe(2);
  });
});
