import { afterEach, describe, expect, it, vi } from 'vitest';

import { AGENT_ICON_ACCEPT, MAX_AGENT_ICON_BYTES, agentIconClass, agentIconSrc, readAgentIconFile } from '../src/lib/agent-icons';

afterEach(() => vi.unstubAllGlobals());

describe('agent icon helpers', () => {
  it('keeps known compact icons visually balanced', () => {
    expect(agentIconClass('codex', 'size-4')).toContain('scale-125');
    expect(agentIconClass('gemini', 'size-4')).toContain('scale-110');
    expect(agentIconClass('opencode', 'size-4')).toContain('scale-110');
    expect(agentIconClass('claude', 'size-4')).not.toContain('scale-');
  });

  it('inverts bundled monochrome icons only in dark mode', () => {
    expect(agentIconClass('claude', 'size-4')).toContain('dark:invert');
    expect(agentIconClass(' amp-acp ', 'size-4')).toContain('dark:invert');
    expect(agentIconClass('agent', 'size-4')).not.toContain('dark:invert');
    expect(agentIconClass('https://example.com/icon.svg', 'size-4')).not.toContain('dark:invert');
  });

  it('routes the Gold Band icon to the app logo', () => {
    expect(agentIconSrc('gold-band')).toBe('/logo.svg');
    expect(agentIconSrc('claude')).toBe('/agent-icons/claude.svg');
  });

  it('imports supported local images as stable data URIs', async () => {
    class StubFileReader {
      result: string | ArrayBuffer | null = null;
      onload: null | (() => void) = null;
      onerror: null | (() => void) = null;

      readAsDataURL() {
        this.result = 'data:image/png;base64,aWNvbg==';
        this.onload?.();
      }
    }
    vi.stubGlobal('FileReader', StubFileReader);

    const result = await readAgentIconFile({ type: 'image/png', size: 4 } as File);

    expect(result).toBe('data:image/png;base64,aWNvbg==');
    expect(AGENT_ICON_ACCEPT).toContain('image/svg+xml');
  });

  it('rejects unsupported or oversized local icons', async () => {
    await expect(readAgentIconFile({ type: 'image/gif', size: 4 } as File))
      .rejects.toThrow('agent-icon.unsupported-image-type');
    await expect(readAgentIconFile({ type: 'image/png', size: MAX_AGENT_ICON_BYTES + 1 } as File))
      .rejects.toThrow('agent-icon.image-too-large');
  });
});
