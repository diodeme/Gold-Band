import { describe, expect, it } from 'vitest';
import {
  conversationRunModeOrDefault,
  mergeConversationRunMode,
  normalizeConversationAutoConfigForSubmit,
  normalizeOptionalRunModeText,
  optionalRunModeText,
} from '../src/lib/conversation-run-mode-config';

describe('conversation run mode config text fields', () => {
  it('uses default AUTO when a workspace has no saved run mode', () => {
    expect(conversationRunModeOrDefault(null)).toEqual({ mode: 'auto' });
    expect(conversationRunModeOrDefault(undefined)).toEqual({ mode: 'auto' });
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
