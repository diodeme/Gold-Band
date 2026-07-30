import { describe, expect, it } from 'vitest';
import {
  normalizeConfigOptionOverrides,
  validateAutoConfig,
  validateDirectConfig,
  validateWorkflowTemplateForConversationStart,
  validateWorkflowTemplateForConversationStartWithFreshProfiles,
} from '../src/lib/run-mode-validation';
import type { AgentRegistryVm, ProfileVm, WorkflowTemplateStore } from '../src/types';

const t = (key: string, options?: Record<string, unknown>) => {
  const messages: Record<string, string> = {
    'conversation.home.selectWorkflowTemplate': '请选择工作流模板',
    'conversation.validation.workflow.not-found': 'Selected workflow template not found',
    'workflowEditor.validationPermissionModeUnavailable': `${options?.node} 节点的权限模式不属于当前 Agent。`,
    'workflowEditor.validationNodeProfileRequired': `${options?.node} 节点未关联角色。`,
    'workflowEditor.validationNodeProfileVisibilityChanged': `${options?.node} 节点关联的角色不存在或已删除，请重新设置。`,
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
    primaryAgentDir: '.claude',
    compatibleAgentDirs: [],
    externalSessionSyncEnabled: false,
    supported: true,
    supportedModes: [{ id: 'ask', name: 'Ask' }],
    supportedModels: [],
    configOptions: [{
      id: 'thought',
      name: 'Thought',
      description: '',
      category: 'thought_level',
      options: [{ value: 'high', name: 'High' }],
    }],
    diagnostic: { status: 'ok', available: true, reason: null, checkedAt: '' },
  }],
  supportedTypes: [],
};

const profiles: ProfileVm[] = [{
  id: 'profile-1',
  name: '开发',
  summary: '',
  content: '',
  dynamicTemplate: false,
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
  it('normalizes stale config overrides without mutating the input', () => {
    const overrides = { thought: 'high', removed: 'legacy' };
    const snapshot = { ...overrides };
    const normalized = normalizeConfigOptionOverrides(agentRegistry.agents[0], overrides);

    expect(normalized).toEqual({
      configOptions: { thought: 'high' },
      removedOptionIds: ['removed'],
    });
    expect(overrides).toEqual(snapshot);
  });

  it('direct and auto validation tolerate stale overrides without mutation', () => {
    const direct = { agentType: 'claude-acp', configOptions: { removed: 'legacy' } };
    const auto = { agentType: 'claude-acp', configOptions: { removed: 'legacy' } };
    const directSnapshot = structuredClone(direct);
    const autoSnapshot = structuredClone(auto);

    expect(validateDirectConfig(direct, agentRegistry, t)).toEqual([]);
    expect(validateAutoConfig(auto, agentRegistry, null, t)).toEqual([]);
    expect(direct).toEqual(directSnapshot);
    expect(auto).toEqual(autoSnapshot);
  });

  it('allows dynamic agents to use the provider default model', () => {
    const issues = validateAutoConfig({
      agentStrategy: 'dynamic',
      agentType: 'claude-acp',
      bootstrapAgentType: 'claude-acp',
      availableAgents: [{ provider: 'claude-acp' }],
      routingPrompt: '',
    }, agentRegistry, null, t);

    expect(issues).toEqual([]);
  });

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

  it('refreshes profiles before validating a workflow conversation start', async () => {
    const freshProfile: ProfileVm = {
      id: 'fresh-profile',
      name: '新角色',
      summary: '',
      content: '',
      dynamicTemplate: false,
      scope: 'user',
      isBuiltIn: false,
      createdAt: '',
      updatedAt: '',
      path: '',
    };
    const templates: WorkflowTemplateStore = {
      version: '1',
      templates: [{
        id: 'fresh-template',
        name: '新角色工作流',
        createdAt: '',
        updatedAt: '',
        workflow: {
          version: '0.1',
          id: 'fresh-workflow',
          entry: 'dev',
          control: {},
          nodes: [{
            id: 'dev',
            type: 'worker',
            provider: 'claude-acp',
            profile: freshProfile.id,
          }],
          edges: [{ from: 'dev', to: '$end', on: 'success' }],
        },
      }],
      lastUsedTemplateId: 'fresh-template',
    };

    const staleIssues = validateWorkflowTemplateForConversationStart(
      'fresh-template',
      agentRegistry,
      [],
      templates,
      t,
    );
    const freshIssues = await validateWorkflowTemplateForConversationStartWithFreshProfiles(
      'fresh-template',
      agentRegistry,
      [],
      async () => [freshProfile],
      templates,
      t,
    );

    expect(staleIssues).toContain('dev 节点关联的角色不存在或已删除，请重新设置。');
    expect(freshIssues).toEqual([]);
  });
});
