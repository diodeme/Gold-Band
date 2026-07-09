import { describe, expect, it } from 'vitest';

import { agentIconClass, agentIconSrc } from '../src/lib/agent-icons';

describe('agent icon helpers', () => {
  it('keeps known compact icons visually balanced', () => {
    expect(agentIconClass('codex', 'size-4')).toContain('scale-125');
    expect(agentIconClass('gemini', 'size-4')).toContain('scale-110');
    expect(agentIconClass('opencode', 'size-4')).toContain('scale-110');
    expect(agentIconClass('claude', 'size-4')).not.toContain('scale-');
  });

  it('routes the Gold Band icon to the app logo', () => {
    expect(agentIconSrc('gold-band')).toBe('/logo.svg');
    expect(agentIconSrc('claude')).toBe('/agent-icons/claude.svg');
  });
});
