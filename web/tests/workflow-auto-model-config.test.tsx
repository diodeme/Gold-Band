import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { optionalWorkerConfigOptions, workerAgentSelectionPatch, WorkflowEditor, WorkflowNodeInspector, workflowEditorSupportedAgents } from '@/components/WorkflowEditor';
import { RunModeManagementPage } from '@/pages/RunModeManagementPage';
import type { AgentRegistryVm, WorkflowDsl } from '@/types';

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

function renderWorkflowNodeInspector(workflow: WorkflowDsl) {
  return renderToStaticMarkup(React.createElement(WorkflowNodeInspector, {
    node: workflow.nodes[0],
    agents: workflowEditorSupportedAgents(agentRegistry),
    profiles: workflow.nodes[0]?.type === 'worker' ? [{ id: 'interview', name: 'Interview' }] : [],
    workflow,
    workflowTemplates: null,
    fieldErrors: {},
    onUpdate: () => undefined,
    t: i18n.t.bind(i18n),
  }));
}

describe('workflow and AUTO model configuration', () => {
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
        provider: 'claude-acp',
        profile: 'interview',
        model: 'sonnet',
        config_options: { reasoning_effort: 'high' },
      }],
      edges: [{ from: 'interview', to: '$end', on: 'success' }],
    };

    const editorHtml = renderToStaticMarkup(React.createElement(WorkflowEditor, {
      value: workflow,
      agentRegistry,
      profiles: [{ id: 'interview', name: 'Interview' }],
      onSave: () => undefined,
      showSaveAction: false,
    }));
    const html = renderWorkflowNodeInspector(workflow);

    expect(editorHtml).not.toContain('Sonnet · High');
    expect(html).toContain('Sonnet · High');
    expect(html).toContain('data-slot="dropdown-menu-trigger"');
  });

  it('replays an AUTO fixed-agent model and thought level through the same selector', () => {
    const html = renderToStaticMarkup(React.createElement(RunModeManagementPage, {
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

    const editorHtml = renderToStaticMarkup(React.createElement(WorkflowEditor, {
      value: workflow,
      agentRegistry,
      profiles: [],
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
    const html = renderToStaticMarkup(React.createElement(RunModeManagementPage, {
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
