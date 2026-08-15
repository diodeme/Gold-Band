import fs from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { applyPersonalization, desktopTypography, moveFontFamily, normalizeFontFamilies, normalizeTypographySize, toggleFontFamily } from '../src/theme';
import { normalizeFontCatalogFamilies } from '../src/lib/font-families';

describe('desktop typography preferences', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('defines bounded defaults for UI and code text', () => {
    expect(desktopTypography).toEqual({
      ui: { min: 12, max: 18, defaultValue: 14 },
      editor: { min: 10, max: 18, defaultValue: 12 },
    });
    expect(normalizeTypographySize(9, 'ui')).toBe(12);
    expect(normalizeTypographySize(20, 'editor')).toBe(18);
    expect(normalizeTypographySize(Number.NaN, 'ui')).toBe(14);
  });

  it('applies normalized CSS variables at the document root', () => {
    const setProperty = vi.fn();
    vi.stubGlobal('document', { documentElement: { dataset: {}, style: { setProperty } } });

    applyPersonalization({
      schemaVersion: 2,
      typography: {
        ui: { fontStack: { source: 'theme' }, fontSize: { source: 'custom', px: 15.6 } },
        editor: { fontStack: { source: 'custom', families: ['Fira Code', 'Consolas'] }, fontSize: { source: 'custom', px: 30 } },
      },
      avatars: {
        agent: { image: { source: 'theme' }, shape: { source: 'theme' } },
        user: { image: { source: 'theme' }, shape: { source: 'theme' } },
      },
    });

    expect(setProperty).toHaveBeenNthCalledWith(1, '--app-font-sans', 'var(--gb-theme-ui-font-family)');
    expect(setProperty).toHaveBeenNthCalledWith(2, '--app-editor-font-family', '"Fira Code", "Consolas", var(--gb-theme-editor-font-family)');
    expect(setProperty).toHaveBeenNthCalledWith(3, '--app-ui-font-size', '16px');
    expect(setProperty).toHaveBeenNthCalledWith(4, '--app-editor-font-size', '18px');
  });

  it('keeps preview local and persists only committed input values', () => {
    const source = fs.readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    expect(source).toContain("onChange={(value) => previewTypographySize('ui', value)}");
    expect(source).toContain("onCommit={(value) => chooseTypographySize('ui', value)}");
    expect(source).toContain('onBlur={(event) => {');
    expect(source).toContain('onPointerDown={(event) => event.preventDefault()}');
    expect(source).toContain('window.sessionStorage.setItem(typographyDisclosureSessionKey');
    expect(source).not.toContain("localStorage.setItem(typographyDisclosureSessionKey");
  });

  it('uses the MiSans variable axis between Light and Regular and caps emphasis at Semibold', () => {
    const styles = fs.readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const fontDirectory = path.resolve(__dirname, '../public/fonts/misans');
    expect(styles).toContain('src: url("/fonts/misans/MiSans-VF.ttf") format("truetype-variations");');
    expect(styles).toContain('font-weight: 250 520;');
    expect(styles).toContain('--font-weight-normal: 330;');
    expect(styles).toContain('--font-weight-medium: 380;');
    expect(styles).toContain('--font-weight-semibold: 450;');
    expect(styles).toContain('--font-weight-bold: 520;');
    expect(styles).toContain('@apply m-0 overflow-hidden bg-background font-sans font-normal text-foreground;');
    expect(styles).not.toMatch(/MiSans-(?:Light|Regular|Medium|Semibold|Bold)\.woff2/u);
    expect(styles).not.toContain('font-weight: 700;');
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-VF.ttf'))).toBe(true);
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-Light.woff2'))).toBe(false);
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-Regular.woff2'))).toBe(false);
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-Medium.woff2'))).toBe(false);
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-Semibold.woff2'))).toBe(false);
    expect(fs.existsSync(path.join(fontDirectory, 'MiSans-Bold.woff2'))).toBe(false);
  });

  it('bundles Inter Variable ahead of MiSans for coordinated Latin and CJK text', () => {
    const main = fs.readFileSync(path.resolve(__dirname, '../src/main.tsx'), 'utf8');
    const styles = fs.readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const goldBandPreset = fs.readFileSync(path.resolve(__dirname, '../../themes/gold-band/presets.json'), 'utf8');
    const techNeutralPreset = fs.readFileSync(path.resolve(__dirname, '../../themes/tech-neutral/presets.json'), 'utf8');
    expect(main).toContain("import '@fontsource-variable/inter/wght.css';");
    expect(styles).toContain('--app-font-sans: "Inter Variable", "Gold Band MiSans"');
    expect(goldBandPreset).toContain('"families": ["Inter Variable", "Gold Band MiSans"');
    expect(techNeutralPreset).toContain('"families": ["Inter Variable", "Gold Band MiSans"');
  });

  it('keeps the complete sorted font catalog separate from the bounded preference stack', () => {
    const catalog = Array.from({ length: 100 }, (_, index) => `Font ${100 - index}`);
    expect(normalizeFontCatalogFamilies([...catalog, ' font 1 ', 'FONT 2', ''])).toHaveLength(100);
    expect(normalizeFontCatalogFamilies([' Zeta ', 'alpha', 'ALPHA', 'Font 10', 'Font 2'])).toEqual([
      'alpha',
      'Font 2',
      'Font 10',
      'Zeta',
    ]);
    expect(normalizeFontFamilies(catalog)).toHaveLength(16);
  });

  it('normalizes, toggles, and reorders an ordered font stack', () => {
    expect(normalizeFontFamilies([' Segoe UI ', 'segoe ui', 'Gold Band MiSans', 'bad,font'])).toEqual(['Segoe UI', 'Gold Band MiSans']);
    expect(normalizeFontFamilies(['x'.repeat(129), 'Segoe UI'])).toEqual(['Segoe UI']);
    expect(toggleFontFamily(['Segoe UI'], 'Gold Band MiSans')).toEqual(['Segoe UI', 'Gold Band MiSans']);
    expect(toggleFontFamily(['Segoe UI', 'Gold Band MiSans'], 'segoe ui')).toEqual(['Gold Band MiSans']);
    expect(moveFontFamily(['Segoe UI', 'Gold Band MiSans'], 1, -1)).toEqual(['Gold Band MiSans', 'Segoe UI']);
    expect(moveFontFamily(['Segoe UI', 'Gold Band MiSans'], 0, -1)).toEqual(['Segoe UI', 'Gold Band MiSans']);
  });

  it('uses the accessible shadcn font-stack selector and restores the theme for an empty stack', () => {
    const source = fs.readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    const selector = source.slice(source.indexOf('function FontPreferenceSetting'), source.indexOf('function FontStackAction'));
    const stackProjection = source.slice(source.indexOf('function withTypographyFontStack'), source.indexOf('function CurrentThemeSummary'));
    expect(selector).toContain('<Popover open={open} onOpenChange={setOpen}>');
    expect(selector).toContain('<CommandItem');
    expect(selector).toContain('normalizeFontCatalogFamilies([');
    expect(selector).toContain('<FontStackAction');
    expect(selector).not.toContain('<Select');
    expect(selector).toContain('onClick={() => onChange([])}');
    expect(stackProjection).toContain("? { source: 'theme' as const }");
    expect(stackProjection).toContain(": { source: 'custom' as const, families: normalized }");
  });

  it('keeps editor typography isolated from chat inline labels and covers all CodeMirror views', () => {
    const editorTheme = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/editor-extensions.ts'), 'utf8');
    const markdown = fs.readFileSync(path.resolve(__dirname, '../src/components/prompt-kit/markdown.tsx'), 'utf8');
    const fileViewer = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/WorkspaceFileEditor.tsx'), 'utf8');
    const diffViewer = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/TurnFileWorkspacePanel.tsx'), 'utf8');
    expect(editorTheme).toContain("fontFamily: 'var(--app-editor-font-family, ui-monospace)'");
    expect(editorTheme).toContain("fontSize: 'var(--app-editor-font-size, 12px)'");
    expect(fileViewer).toContain('<CodeMirror');
    expect(diffViewer).toContain('unifiedMergeView({');
    expect(markdown).toContain('font-sans text-[1em] font-normal leading-[inherit] tracking-normal');
    expect(markdown).not.toContain('var(--app-ui-code-font-size)');
    expect(markdown).not.toContain('var(--app-editor-font-size)');
  });

  it('uses UI-derived typography tokens in the conversation sidebar', () => {
    const sidebar = fs.readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'), 'utf8');
    expect(sidebar).toContain('truncate text-sm');
    expect(sidebar).not.toContain('text-ui-compact');
    expect(sidebar).toContain('text-ui-caption font-normal leading-4 tabular-nums text-muted-foreground/55');
    expect(sidebar).not.toContain('text-ui-micro tabular-nums text-muted-foreground');
    expect(sidebar).not.toMatch(/text-\[(?:10|12|13|14)px\]/u);
  });

  it('serializes preference saves without clearing unrelated business state', () => {
    const source = fs.readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    const saveBlock = source.slice(source.indexOf('const onSavePreferences'), source.indexOf('const applyAvatarPreferences'));
    expect(saveBlock).toContain('preferenceSaveQueueRef.current');
    expect(saveBlock).toContain('preferenceSaveGenerationRef.current');
    expect(saveBlock).not.toContain('setTaskList(null)');
    expect(saveBlock).not.toContain('setWorkflow(null)');
    expect(saveBlock).not.toContain('setRoundDetail(null)');
  });
});
