import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { optionalWorkerConfigOptions, workerAgentSelectionPatch, WorkflowEditor, WorkflowNodeInspector, workflowEditorSupportedAgents } from '@/components/WorkflowEditor';
import { RunModeManagementPage } from '@/pages/RunModeManagementPage';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AgentRegistryVm, WorkflowDsl, WorkflowModelBindings } from '@/types';
import { readyWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';

const agentRegistry = {
  agents: [{
    agentType: 'claude-acp',
    displayName: 'Claude',
    diagnostic: { available: true },
    supportedModels: [{ id: 'sonnet', name: 'Sonnet' }],
    supportedModes: [{ id: 'acceptEdits', name: 'Accept Edits' }],
    configOptions: [{
      id: 'reasoning_effort',
      category: 'thought_level',
      options: [{ value: 'high', name: 'High' }],
    }],
  }],
  catalog: [],
} as AgentRegistryVm;

function renderWithTooltip(element: React.ReactElement) {
  return renderToStaticMarkup(React.createElement(TooltipProvider, null, element));
}

function renderWorkflowNodeInspector(
  workflow: WorkflowDsl,
  modelBindings: WorkflowModelBindings = { definitionRevision: '', bindingRevision: 0, bindings: [] },
) {
  const node = workflow.nodes[0];
  return renderWithTooltip(React.createElement(WorkflowNodeInspector, {
    node,
    binding: node?.type === 'worker'
      ? modelBindings.bindings.find((binding) => binding.executionSlotId === node.executionSlotId) ?? null
      : null,
    modelBindings,
    agents: workflowEditorSupportedAgents(agentRegistry),
    profiles: node?.type === 'worker' ? [{ id: 'interview', name: 'Interview' }] : [],
    workflow,
    workflowTemplates: null,
    fieldErrors: {},
    onUpdate: () => undefined,
    onBindingUpdate: () => undefined,
    onBindingSync: () => undefined,
    workflowControl: React.createElement('div', { 'data-slot': 'workflow-control-config' }),
    t: i18n.t.bind(i18n),
  }));
}

describe('workflow and AUTO model configuration', () => {
  it('renders workflow controls directly when the canvas has no selected node', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'workflow-empty-selection',
      entry: 'interview',
      control: { max_attempts: 10, max_rounds: 3 },
      nodes: [{
        type: 'worker',
        id: 'interview',
        executionSlotId: 'slot-interview',
        profile: 'interview',
      }],
      edges: [{ from: 'interview', to: '$end', on: 'success' }],
    };

    const html = renderWithTooltip(React.createElement(WorkflowEditor, {
      value: workflow,
      agentRegistry,
      profileCatalog: readyWorkflowProfileCatalog([{ id: 'interview', name: 'Interview' }]),
      onSave: () => undefined,
      showSaveAction: false,
    }));

    expect(html.match(/data-slot="workflow-control-config"/g)).toHaveLength(1);
    expect(html).toContain(i18n.t('workflowEditor.workflowControls'));
    expect(html).not.toContain(i18n.t('workflowEditor.workflowSettings'));
  });

  it('restores omitted worker Agent options after switching back to the original Agent', () => {
    const original = {
      type: 'worker' as const,
      id: 'interview',
      provider: 'claude-acp',
      profile: 'interview',
      permission_mode: 'bypassPermissions',
    };
    const switched = { ...original, ...workerAgentSelectionPatch('cursor') };
    const restored = {
      ...switched,
      ...workerAgentSelectionPatch('claude-acp'),
      permission_mode: 'bypassPermissions',
    };

    expect(JSON.stringify(restored)).toBe(JSON.stringify(original));
    expect(optionalWorkerConfigOptions({})).toBeUndefined();
    expect(optionalWorkerConfigOptions({ reasoning_effort: 'high' })).toEqual({ reasoning_effort: 'high' });
  });

  it('replays a worker node model and thought level through the shared composite selector', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'workflow-model-config',
      entry: 'interview',
      control: {},
      nodes: [{
        type: 'worker',
        id: 'interview',
        executionSlotId: 'slot-interview',
        profile: 'interview',
      }],
      edges: [{ from: 'interview', to: '$end', on: 'success' }],
    };

    const modelBindings = { definitionRevision: '', bindingRevision: 0, bindings: [{ executionSlotId: 'slot-interview', agentId: 'claude-acp', modelId: 'sonnet', configOptions: { reasoning_effort: 'high' } }] };
    const editorHtml = renderWithTooltip(React.createElement(WorkflowEditor, {
      value: workflow,
      modelBindings,
      agentRegistry,
      profileCatalog: readyWorkflowProfileCatalog([{ id: 'interview', name: 'Interview' }]),
      onSave: () => undefined,
      showSaveAction: false,
    }));
    const html = renderWorkflowNodeInspector(workflow, modelBindings);

    expect(editorHtml).not.toContain('Sonnet · High');
    expect(html).toContain('Sonnet · High');
    expect(html).toContain('data-slot="dropdown-menu-trigger"');
    const modelConfigIndex = html.indexOf('data-slot="worker-model-config"');
    const workflowControlIndex = html.indexOf('data-slot="workflow-control-config"');
    const nodeConfigIndex = html.indexOf('data-slot="worker-node-config"');
    expect(modelConfigIndex).toBeGreaterThan(-1);
    expect(workflowControlIndex).toBeGreaterThan(modelConfigIndex);
    expect(nodeConfigIndex).toBeGreaterThan(workflowControlIndex);
    expect(html).toContain('data-slot="worker-inspector"');
  });

  it('states that model synchronization is limited to the current workflow', () => {
    expect(i18n.getResource('zh-CN', 'translation', 'workflowEditor.syncDialogDescription')).toContain('仅在当前工作流内');
    expect(i18n.getResource('en', 'translation', 'workflowEditor.syncDialogDescription')).toContain('current workflow only');
  });

  it('replays an AUTO fixed-agent model and thought level through the same selector', () => {
    const html = renderWithTooltip(React.createElement(RunModeManagementPage, {
      projectId: 'project-a',
      workspaceName: 'Project A',
      workspaces: [{ projectId: 'project-a', name: 'Project A', workspacePath: 'D:/project-a' }],
      runMode: {
        mode: 'auto',
        autoConfig: {
          agentStrategy: 'fixed',
          agentType: 'claude-acp',
          modelId: 'sonnet',
          configOptions: { reasoning_effort: 'high' },
        },
      },
      agentRegistry,
      workflowTemplates: { version: '1', templates: [] },
      onProjectChange: () => undefined,
      onSave: () => undefined,
      onBack: () => undefined,
    }));

    expect(html).toContain('Sonnet · High');
    expect(html).toContain('data-slot="dropdown-menu-trigger"');
  });

  it('replays every dynamic workflow model role with its own thought-level override', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'workflow-dynamic-model-config',
      entry: 'route',
      control: {},
      nodes: [{
        type: 'ai-dynamic',
        id: 'route',
        agentStrategy: {
          mode: 'dynamic',
          bootstrapProvider: 'claude-acp',
          bootstrapModel: 'sonnet',
          permissionMode: 'acceptEdits',
          bootstrapConfigOptions: { reasoning_effort: 'high' },
          acceptanceModel: 'sonnet',
          acceptanceConfigOptions: { reasoning_effort: 'high' },
          routingPrompt: '',
          availableAgents: [{
            provider: 'claude-acp',
            model: 'sonnet',
            permissionMode: 'acceptEdits',
            configOptions: { reasoning_effort: 'high' },
          }],
        },
        control: {
          maxDynamicNodes: 20,
          maxFanout: 5,
          maxDepth: 6,
          maxParallel: 3,
          maxGroupDepth: 1,
          maxWorkflowInvocations: 10,
          allowNestedDynamic: false,
        },
        allowedWorkflows: [],
      }],
      edges: [{ from: 'route', to: '$end', on: 'success' }],
    };

    const editorHtml = renderWithTooltip(React.createElement(WorkflowEditor, {
      value: workflow,
      agentRegistry,
      profileCatalog: readyWorkflowProfileCatalog([]),
      onSave: () => undefined,
      showSaveAction: false,
      allowAiDynamic: true,
    }));
    const html = renderWorkflowNodeInspector(workflow);

    expect(editorHtml).not.toContain('Sonnet · High');
    expect(html.match(/Sonnet · High/g)?.length).toBe(3);
    expect(html.match(/Accept Edits/g)?.length).toBe(2);
  });

  it('replays every dynamic AUTO model role with its own thought-level override', () => {
    const html = renderWithTooltip(React.createElement(RunModeManagementPage, {
      projectId: 'project-a',
      workspaceName: 'Project A',
      workspaces: [{ projectId: 'project-a', name: 'Project A', workspacePath: 'D:/project-a' }],
      runMode: {
        mode: 'auto',
        autoConfig: {
          agentStrategy: 'dynamic',
          agentType: 'claude-acp',
          bootstrapAgentType: 'claude-acp',
          bootstrapModelId: 'sonnet',
          permissionMode: 'acceptEdits',
          bootstrapConfigOptions: { reasoning_effort: 'high' },
          acceptanceModelId: 'sonnet',
          acceptanceConfigOptions: { reasoning_effort: 'high' },
          availableAgents: [{
            provider: 'claude-acp',
            model: 'sonnet',
            permissionMode: 'acceptEdits',
            configOptions: { reasoning_effort: 'high' },
          }],
          routingPrompt: '',
        },
      },
      agentRegistry,
      workflowTemplates: { version: '1', templates: [] },
      onProjectChange: () => undefined,
      onSave: () => undefined,
      onBack: () => undefined,
    }));

    expect(html.match(/Sonnet · High/g)?.length).toBe(3);
    expect(html.match(/Accept Edits/g)?.length).toBe(2);
  });
});
