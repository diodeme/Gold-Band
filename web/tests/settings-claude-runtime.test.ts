import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const settingsSource = readFileSync(
  fileURLToPath(new URL('../src/pages/SettingsPage.tsx', import.meta.url)),
  'utf8',
);

describe('settings Claude runtime contract', () => {
  it('hides the local Claude preference and always saves the npm-package runtime choice', () => {
    expect(settingsSource).toContain('const useLocalClaude = false;');
    expect(settingsSource).not.toContain("t('settings.useLocalClaude");
    expect(settingsSource).not.toContain('checkLocalClaude()');
    expect(settingsSource).not.toContain('setUseLocalClaude');
  });
});
