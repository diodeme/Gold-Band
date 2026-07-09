import { describe, expect, it } from 'vitest';
import { validateWorkflowTemplateForConversationStart } from '../src/lib/run-mode-validation';
import type { AgentRegistryVm, ProfileVm, WorkflowTemplateStore } from '../src/types';

const t = (key: string, options?: Record<string, unknown>) => {
  const messages: Record<string, string> = {
    'conversation.home.selectWorkflowTemplate': '请选择工作流模板',
    'conversation.validation.workflow.not-found': 'Selected workflow template not found',
    'workflowEditor.validationPermissionModeUnavailable': `${options?.node} 节点的权限模式不属于当前 Agent。`,
    'workflowEditor.validationNodeProfileRequired': `${options?.node} 节点未关联角色。`,
  };
  return messages[key] ?? key;
};

const agentRegistry: AgentRegistryVm = {
  agents: [{
    agentType: 'claude-acp',
    displayName: 'Claude',
    command: 'claude',
    args: [],
    env: [],
    iconKey: 'claude',
    skillsDirName: '.claude',
    supported: true,
    supportedModes: [{ id: 'ask', name: 'Ask' }],
    supportedModels: [],
    diagnostic: { status: 'ok', available: true, reason: null, checkedAt: '' },
  }],
  supportedTypes: [],
};

const profiles: ProfileVm[] = [{
  id: 'profile-1',
  name: '开发',
  summary: '',
  content: '',
  scope: 'user',
  isBuiltIn: false,
  createdAt: '',
  updatedAt: '',
  path: '',
}];

const workflowTemplates: WorkflowTemplateStore = {
  version: '1',
  templates: [{
    id: 'invalid-template',
    name: '非法工作流',
    createdAt: '',
    updatedAt: '',
    workflow: {
      version: '0.1',
      id: 'invalid-workflow',
      entry: 'ai-dynamic1',
      control: {},
      nodes: [{
        id: 'ai-dynamic1',
        type: 'ai-dynamic',
        agentStrategy: { mode: 'fixed', provider: 'claude-acp' },
        permission_mode: 'full_access',
        allowedProfiles: [],
        allowedWorkflows: [],
        control: {
          maxDynamicNodes: 20,
          maxFanout: 5,
          maxDepth: 6,
          maxParallel: 3,
          maxGroupDepth: 1,
          maxWorkflowInvocations: 10,
          allowNestedDynamic: false,
        },
      }],
      edges: [{ from: 'ai-dynamic1', to: '$end', on: 'success' }],
    },
  }],
  lastUsedTemplateId: 'invalid-template',
};

describe('run mode validation', () => {
  it('blocks invalid workflow templates before starting quick conversation', () => {
    const issues = validateWorkflowTemplateForConversationStart(
      'invalid-template',
      agentRegistry,
      profiles,
      workflowTemplates,
      t,
    );

    expect(issues).toContain('ai-dynamic1 节点的权限模式不属于当前 Agent。');
  });
});
