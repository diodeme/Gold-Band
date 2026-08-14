import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const switcherSource = readFileSync(
  fileURLToPath(new URL('../src/components/conversation/ConversationSessionSwitcher.tsx', import.meta.url)),
  'utf8',
);

describe('conversation session switcher theme surface', () => {
  it('delegates transparency and blur to the theme popover recipe', () => {
    expect(switcherSource).toContain('data-theme-role="popover"');
    expect(switcherSource).toContain('bg-popover');
    expect(switcherSource).toContain('text-popover-foreground');
    expect(switcherSource).not.toContain('bg-card/60');
  });
});
