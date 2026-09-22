import { describe, expect, it } from 'vitest';

import {
  isWorkflowAgentDoctorReady,
  validateWorkflowForSave,
  workflowAgentIconKeys,
  workflowEditorSupportedAgents,
} from '@/components/WorkflowEditor';
import type { AgentRegistryVm, ManagedAgentVm, WorkflowDsl } from '@/types';
import { readyWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';

const failedAgent = {
  agentType: 'cursor',
  displayName: 'Cursor',
  diagnostic: {
    available: false,
    status: 'unhealthy',
    error: {
      code: 'acp.adapter-exited',
      params: { method: 'initialize' },
    },
  },
} as ManagedAgentVm;

describe('workflow agent health', () => {
  it('uses managed Agent icon metadata for built-in and custom workflow nodes', () => {
    const icons = workflowAgentIconKeys([
      { agentType: 'kimi', iconKey: 'kimi' },
      { agentType: 'custom-acp', iconKey: 'data:image/png;base64,custom' },
      { agentType: 'without-icon', iconKey: '' },
    ] as ManagedAgentVm[]);

    expect(icons.get('kimi')).toBe('kimi');
    expect(icons.get('custom-acp')).toBe('data:image/png;base64,custom');
    expect(icons.get('without-icon')).toBe('gold-band');
    expect(icons.has('not-configured')).toBe(false);
  });

  it('keeps a configured failed agent in editor options but marks it unavailable', () => {
    const registry = {
      agents: [failedAgent],
      catalog: [],
    } as AgentRegistryVm;

    expect(workflowEditorSupportedAgents(registry)).toEqual([failedAgent]);
    expect(isWorkflowAgentDoctorReady(failedAgent)).toBe(false);
  });

  it('still blocks saving a workflow that references the failed agent', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'failed-agent-workflow',
      entry: 'cursor-node',
      control: {},
      nodes: [{
        type: 'worker',
        id: 'cursor-node',
        executionSlotId: 'slot-cursor',
        profile: 'developer',
        goal: 'Implement the change',
      }],
      edges: [{ from: 'cursor-node', to: '$end', on: 'success' }],
    };

    const validation = validateWorkflowForSave(
      workflow,
      readyWorkflowProfileCatalog([{ id: 'developer', name: 'Developer' }]),
      [],
      (key) => key,
      null,
      null,
      null,
      true,
      { definitionRevision: '', bindingRevision: 0, bindings: [{ executionSlotId: 'slot-cursor', agentId: 'cursor' }] },
    );

    expect(validation.valid).toBe(false);
    expect(validation.issues.map((issue) => issue.message)).toContain('workflowEditor.validationNodeProviderUnavailable');
  });

  it('accepts Grok effort and fast when Doctor current table belongs to another model', () => {
    const agent = {
      agentType: 'cursor',
      displayName: 'Cursor',
      diagnostic: { available: true, status: 'healthy' },
      supportedModels: [
        { id: 'grok-4.6', name: 'Cursor Grok 4.6' },
        { id: 'gpt-5.6-luna', name: 'GPT-5.6 Luna' },
      ],
      configOptions: [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }],
        },
        {
          id: 'context',
          category: 'model_config',
          options: [{ value: '1m', name: '1M' }],
        },
      ],
      modelBoundCatalogs: {
        'grok-4.6': [
          {
            id: 'effort',
            category: 'thought_level',
            options: [{ value: 'high', name: 'High' }],
          },
          {
            id: 'fast',
            category: 'model_config',
            options: [{ value: 'false', name: 'Off' }, { value: 'true', name: 'On' }],
          },
        ],
        'gpt-5.6-luna': [
          {
            id: 'reasoning',
            category: 'thought_level',
            options: [{ value: 'high', name: 'High' }],
          },
          {
            id: 'context',
            category: 'model_config',
            options: [{ value: '1m', name: '1M' }],
          },
        ],
      },
    } as ManagedAgentVm;
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'accept-workflow',
      entry: 'accept',
      control: {},
      nodes: [{
        type: 'worker',
        id: 'accept',
        executionSlotId: 'slot-accept',
        profile: 'developer',
        goal: 'Review the change',
      }],
      edges: [{ from: 'accept', to: '$end', on: 'success' }],
    };

    const validation = validateWorkflowForSave(
      workflow,
      readyWorkflowProfileCatalog([{ id: 'developer', name: 'Developer' }]),
      [agent],
      (key, options) => (
        key === 'workflowEditor.validationConfigOptionUnavailable'
          ? `${options?.node} 节点的配置项 ${options?.option} 不属于当前 Agent。`
          : key
      ),
      null,
      null,
      null,
      true,
      {
        definitionRevision: '',
        bindingRevision: 0,
        bindings: [{
          executionSlotId: 'slot-accept',
          agentId: 'cursor',
          modelId: 'grok-4.6',
          configOptions: { effort: 'high', fast: 'false' },
        }],
      },
    );

    expect(validation.issues.map((issue) => issue.message)).toEqual([]);
    expect(validation.valid).toBe(true);
  });

  it('still rejects a listed Grok effort value that is not in the projected catalog', () => {
    const agent = {
      agentType: 'cursor',
      displayName: 'Cursor',
      diagnostic: { available: true, status: 'healthy' },
      supportedModels: [
        { id: 'grok-4.6', name: 'Cursor Grok 4.6' },
        { id: 'gpt-5.6-luna', name: 'GPT-5.6 Luna' },
      ],
      configOptions: [
        {
          id: 'reasoning',
          category: 'thought_level',
          options: [{ value: 'high', name: 'High' }],
        },
      ],
      modelBoundCatalogs: {
        'grok-4.6': [
          {
            id: 'effort',
            category: 'thought_level',
            options: [{ value: 'high', name: 'High' }],
          },
        ],
      },
    } as ManagedAgentVm;
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'accept-workflow',
      entry: 'accept',
      control: {},
      nodes: [{
        type: 'worker',
        id: 'accept',
        executionSlotId: 'slot-accept',
        profile: 'developer',
        goal: 'Review the change',
      }],
      edges: [{ from: 'accept', to: '$end', on: 'success' }],
    };

    const validation = validateWorkflowForSave(
      workflow,
      readyWorkflowProfileCatalog([{ id: 'developer', name: 'Developer' }]),
      [agent],
      (key, options) => (
        key === 'workflowEditor.validationConfigOptionUnavailable'
          ? `${options?.node} 节点的配置项 ${options?.option} 不属于当前 Agent。`
          : key
      ),
      null,
      null,
      null,
      true,
      {
        definitionRevision: '',
        bindingRevision: 0,
        bindings: [{
          executionSlotId: 'slot-accept',
          agentId: 'cursor',
          modelId: 'grok-4.6',
          configOptions: { effort: 'bogus' },
        }],
      },
    );

    expect(validation.issues.map((issue) => issue.message)).toEqual([
      'accept 节点的配置项 effort 不属于当前 Agent。',
    ]);
    expect(validation.valid).toBe(false);
  });
});
