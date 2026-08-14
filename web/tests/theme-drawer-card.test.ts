import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const settingsSource = readFileSync(
  fileURLToPath(new URL('../src/pages/SettingsPage.tsx', import.meta.url)),
  'utf8',
);

describe('theme drawer card contract', () => {
  it('keeps only the active theme in settings and moves the complete visual grid into a sheet', () => {
    const activeSummary = settingsSource.indexOf('<CurrentThemeSummary');
    const sheetContent = settingsSource.indexOf('<SheetContent');
    const completeCatalog = settingsSource.indexOf('themePackageSummaries.map((summary)');

    expect(settingsSource).toContain('<SheetTrigger asChild>');
    expect(settingsSource).toContain("{t('settings.chooseTheme')}");
    expect(settingsSource).toContain('grid-cols-[auto_minmax(0,1fr)_auto]');
    expect(settingsSource).toContain('resizeStorageKey="settings/theme-package-drawer"');
    expect(settingsSource).toContain('grid gap-3 @2xl/theme-drawer:grid-cols-2');
    expect(settingsSource).toContain('group flex min-w-0 flex-col gap-3 rounded-xl');
    expect(activeSummary).toBeGreaterThan(-1);
    expect(completeCatalog).toBeGreaterThan(sheetContent);
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
