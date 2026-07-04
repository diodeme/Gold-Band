import { describe, expect, it } from 'vitest';
import {
  normalizeConversationAutoConfigForSubmit,
  normalizeOptionalRunModeText,
  optionalRunModeText,
} from '../src/lib/conversation-run-mode-config';

describe('conversation run mode config text fields', () => {
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
