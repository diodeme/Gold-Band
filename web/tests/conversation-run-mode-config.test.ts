import { describe, expect, it } from 'vitest';
import {
  canOpenRunModeManagement,
  CONVERSATION_RUN_MODE_ORDER,
  conversationRunModeForWorkspace,
  conversationRunModeOrDefault,
  directConfigForAgent,
  includeInterviewForSubmit,
  isDefaultWorkflowTemplate,
  mergeConversationRunMode,
  normalizeConversationAutoConfigForSubmit,
  normalizeConversationDirectConfigForSubmit,
  normalizeOptionalRunModeText,
  optionalRunModeText,
  shouldShowInterviewToggle,
  setConversationRunModeForWorkspace,
} from '../src/lib/conversation-run-mode-config';

describe('conversation run mode config text fields', () => {
  it('keeps quick-session tabs ordered Direct, Workflow, AUTO', () => {
    expect(CONVERSATION_RUN_MODE_ORDER).toEqual(['direct', 'workflow', 'auto']);
  });

  it('keeps Direct configuration inside the quick-session composer', () => {
    expect(canOpenRunModeManagement('direct')).toBe(false);
    expect(canOpenRunModeManagement('workflow')).toBe(true);
    expect(canOpenRunModeManagement('auto')).toBe(true);
  });
  it('uses default Direct when a workspace has no saved run mode', () => {
    expect(conversationRunModeOrDefault(null)).toEqual({ mode: 'direct' });
    expect(conversationRunModeOrDefault(undefined)).toEqual({ mode: 'direct' });
  });

  it('preserves in-progress spaces while editing session config', () => {
    expect(optionalRunModeText('alpha ')).toBe('alpha ');
    expect(optionalRunModeText('alpha  beta')).toBe('alpha  beta');
    expect(optionalRunModeText(' ')).toBe(' ');
  });

  it('preserves non-blank optional text at submit boundaries', () => {
    expect(normalizeOptionalRunModeText(' alpha beta ')).toBe(' alpha beta ');
    expect(normalizeOptionalRunModeText('   ')).toBeUndefined();
    expect(normalizeOptionalRunModeText(null)).toBeUndefined();
  });

  it('keeps AUTO global goal spaces available to the controlled input loop', () => {
    const typedValue = 'build the ';
    const runModeGlobalGoal = optionalRunModeText(typedValue);
    const remountedInputValue = runModeGlobalGoal ?? '';

    expect(remountedInputValue).toBe('build the ');
  });

  it('preserves AUTO global goal text when creating a conversation', () => {
    expect(normalizeConversationAutoConfigForSubmit({
      agentStrategy: 'fixed',
      agentType: 'claude',
      globalGoal: ' ship the MVP ',
    })).toEqual({
      agentStrategy: 'fixed',
      agentType: 'claude',
      globalGoal: ' ship the MVP ',
    });
  });

  it('preserves AUTO config when switching to workflow mode', () => {
    expect(mergeConversationRunMode(
      {
        mode: 'auto',
        workflowTemplateId: 'workflow-default',
        autoConfig: {
          agentStrategy: 'fixed',
          agentType: 'claude-acp',
          modelId: 'sonnet',
        },
      },
      {
        mode: 'workflow',
        workflowTemplateId: 'workflow-review',
      },
    )).toEqual({
      mode: 'workflow',
      workflowTemplateId: 'workflow-review',
      autoConfig: {
        agentStrategy: 'fixed',
        agentType: 'claude-acp',
        modelId: 'sonnet',
      },
    });
  });

  it('preserves the AUTO thought-level override at the submit boundary', () => {
    expect(normalizeConversationAutoConfigForSubmit({
      agentStrategy: 'fixed',
      agentType: 'claude-acp',
      modelId: 'sonnet',
      configOptions: { reasoning_effort: 'high', blank: '   ' },
    })).toEqual({
      agentStrategy: 'fixed',
      agentType: 'claude-acp',
      modelId: 'sonnet',
      configOptions: { reasoning_effort: 'high' },
    });
  });

  it('normalizes role-scoped dynamic AUTO thought-level overrides', () => {
    expect(normalizeConversationAutoConfigForSubmit({
      agentStrategy: 'dynamic',
      agentType: 'claude-acp',
      bootstrapAgentType: 'claude-acp',
      bootstrapConfigOptions: { reasoning_effort: 'high', blank: ' ' },
      acceptanceConfigOptions: { reasoning_effort: 'medium' },
      availableAgents: [{
        provider: 'claude-acp',
        model: 'sonnet',
        configOptions: { reasoning_effort: 'low', blank: '' },
      }],
    })).toMatchObject({
      bootstrapConfigOptions: { reasoning_effort: 'high' },
      acceptanceConfigOptions: { reasoning_effort: 'medium' },
      availableAgents: [{
        provider: 'claude-acp',
        model: 'sonnet',
        configOptions: { reasoning_effort: 'low' },
      }],
    });
  });

  it('normalizes Direct config without adding runtime prompt fields', () => {
    expect(normalizeConversationDirectConfigForSubmit({
      agentType: ' codex-acp ',
      modelId: 'gpt-direct',
      permissionMode: 'ask',
    })).toEqual({
      agentType: 'codex-acp',
      modelId: 'gpt-direct',
      permissionMode: 'ask',
    });
    expect(normalizeConversationDirectConfigForSubmit({ agentType: '  ' })).toBeUndefined();
  });

  it('restores Direct model and permission per Agent inside a workspace', () => {
    const mode = {
      mode: 'direct' as const,
      directConfig: { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'ask' },
      directPreferences: {
        'claude-acp': { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'ask' },
        'codex-acp': { agentType: 'codex-acp', modelId: 'gpt-direct', permissionMode: 'full-access' },
      },
    };

    expect(directConfigForAgent(mode, 'codex-acp')).toEqual({
      agentType: 'codex-acp',
      modelId: 'gpt-direct',
      permissionMode: 'full-access',
    });
    expect(directConfigForAgent(mode, 'gemini')).toEqual({ agentType: 'gemini' });
  });

  it('isolates Direct Agent preferences by workspace', () => {
    const workspaceA = {
      mode: 'direct' as const,
      directConfig: { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'ask' },
      directPreferences: {
        'claude-acp': { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'ask' },
      },
    };
    const workspaceB = {
      mode: 'direct' as const,
      directConfig: { agentType: 'claude-acp', modelId: 'opus', permissionMode: 'bypassPermissions' },
      directPreferences: {
        'claude-acp': { agentType: 'claude-acp', modelId: 'opus', permissionMode: 'bypassPermissions' },
      },
    };
    const modes = setConversationRunModeForWorkspace(
      setConversationRunModeForWorkspace({}, 'workspace-a', workspaceA),
      'workspace-b',
      workspaceB,
    );

    expect(directConfigForAgent(
      conversationRunModeForWorkspace(modes, 'workspace-a'),
      'claude-acp',
    )).toEqual(workspaceA.directConfig);
    expect(directConfigForAgent(
      conversationRunModeForWorkspace(modes, 'workspace-b'),
      'claude-acp',
    )).toEqual(workspaceB.directConfig);
    expect(conversationRunModeForWorkspace(modes, 'workspace-missing')).toEqual({ mode: 'direct' });
  });

  it('keeps existing Workflow and AUTO memories isolated when another workspace uses Direct', () => {
    const workflow = {
      mode: 'workflow' as const,
      workflowTemplateId: 'workflow-review',
      includeInterview: false,
    };
    const auto = {
      mode: 'auto' as const,
      autoConfig: {
        agentStrategy: 'fixed',
        agentType: 'codex-acp',
        modelId: 'gpt-5',
        permissionMode: 'full-access',
        globalGoal: 'review the repository',
      },
    };
    let modes = setConversationRunModeForWorkspace({}, 'workspace-workflow', workflow);
    modes = setConversationRunModeForWorkspace(modes, 'workspace-auto', auto);
    modes = setConversationRunModeForWorkspace(modes, 'workspace-direct', {
      mode: 'direct',
      directConfig: { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'ask' },
    });

    expect(conversationRunModeForWorkspace(modes, 'workspace-workflow')).toEqual(workflow);
    expect(conversationRunModeForWorkspace(modes, 'workspace-auto')).toEqual(auto);
  });

  it('preserves Direct preferences while switching through Workflow and AUTO', () => {
    const current = {
      mode: 'direct' as const,
      directConfig: { agentType: 'codex-acp', modelId: 'gpt-direct' },
      directPreferences: {
        'codex-acp': { agentType: 'codex-acp', modelId: 'gpt-direct' },
      },
    };
    const workflow = mergeConversationRunMode(current, { mode: 'workflow', workflowTemplateId: 'default' });
    const auto = mergeConversationRunMode(workflow, { mode: 'auto', autoConfig: { agentType: 'claude-acp' } });

    expect(auto.directConfig).toEqual(current.directConfig);
    expect(auto.directPreferences).toEqual(current.directPreferences);
  });

  it('preserves workflow template when switching back to AUTO mode', () => {
    expect(mergeConversationRunMode(
      {
        mode: 'workflow',
        workflowTemplateId: 'workflow-review',
        autoConfig: {
          agentStrategy: 'fixed',
          agentType: 'claude-acp',
        },
      },
      {
        mode: 'auto',
        autoConfig: {
          agentStrategy: 'fixed',
          agentType: 'codex-acp',
        },
      },
    )).toEqual({
      mode: 'auto',
      workflowTemplateId: 'workflow-review',
      autoConfig: {
        agentStrategy: 'fixed',
        agentType: 'codex-acp',
      },
    });
  });

  it('persists the workspace interview preference across mode and template changes', () => {
    expect(mergeConversationRunMode(
      {
        mode: 'workflow',
        workflowTemplateId: 'default',
        includeInterview: false,
      },
      {
        mode: 'workflow',
        workflowTemplateId: 'workflow-review',
      },
    )).toEqual({
      mode: 'workflow',
      workflowTemplateId: 'workflow-review',
      includeInterview: false,
      autoConfig: undefined,
    });
  });

  it('only exposes and submits the interview preference for the built-in default workflow', () => {
    const mode = {
      mode: 'workflow' as const,
      workflowTemplateId: 'default',
      includeInterview: false,
    };

    expect(isDefaultWorkflowTemplate('default')).toBe(true);
    expect(isDefaultWorkflowTemplate('custom')).toBe(false);
    expect(shouldShowInterviewToggle('workflow', 'default')).toBe(true);
    expect(shouldShowInterviewToggle('workflow', 'custom')).toBe(false);
    expect(shouldShowInterviewToggle('auto', 'default')).toBe(false);
    expect(includeInterviewForSubmit(mode, 'default')).toBe(false);
    expect(includeInterviewForSubmit(mode, 'custom')).toBeUndefined();
  });

  it('defaults the default workflow interview preference to enabled', () => {
    expect(includeInterviewForSubmit({ mode: 'workflow' }, 'default')).toBe(true);
  });

  it('preserves special characters, markdown, JSON-like text, emoji, and newlines', () => {
    const globalGoal = [
      '# Goal: keep symbols @#$%^&*()[]{}<>/\\|`~',
      'Markdown **bold** `code` [link](https://example.com?a=1&b=two)',
      'JSON-ish {"quote":"\\"","slash":"\\\\","array":[1,2,3]}',
      '中文标点，emoji 🚀，quotes \'single\' "double"',
    ].join('\n');

    expect(normalizeConversationAutoConfigForSubmit({
      agentStrategy: 'dynamic',
      agentType: 'claude',
      globalGoal,
    })?.globalGoal).toBe(globalGoal);
  });
});
