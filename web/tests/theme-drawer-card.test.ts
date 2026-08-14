import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const settingsSource = readFileSync(
  fileURLToPath(new URL('../src/pages/SettingsPage.tsx', import.meta.url)),
  'utf8',
);

describe('theme drawer card contract', () => {
  it('uses a compact visual grid instead of stretching name-only themes across the drawer', () => {
    expect(settingsSource).toContain('grid gap-3 @4xl/settings-content:grid-cols-3');
    expect(settingsSource).toContain('group flex min-w-0 flex-col gap-3 rounded-xl');
    expect(settingsSource).not.toContain('grid-cols-[72px_minmax(0,1fr)]');
    expect(settingsSource).not.toContain('min-h-32');
  });

  it('shows the effective theme with a semantic check badge', () => {
    expect(settingsSource).toContain('aria-pressed={selected}');
    expect(settingsSource).toContain("selected && 'border-primary/45");
    expect(settingsSource).toContain('<Check className="size-3"');
    expect(settingsSource).toContain('bg-primary px-2 py-0.5 text-ui-micro font-medium text-primary-foreground');
  });
});
