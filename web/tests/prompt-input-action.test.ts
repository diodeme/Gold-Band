import { describe, expect, it } from 'vitest';
import { promptInputActionTitle } from '@/components/prompt-kit/prompt-input';

describe('PromptInputAction', () => {
  it('derives native tooltip titles from simple labels', () => {
    expect(promptInputActionTitle('Send')).toBe('Send');
    expect(promptInputActionTitle(12)).toBe('12');
  });

  it('does not stringify complex tooltip nodes into noisy titles', () => {
    expect(promptInputActionTitle({} as never)).toBeUndefined();
  });
});
