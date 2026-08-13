import fs from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { applyEditorFont, applyTypographyPreferences, desktopTypography, normalizeTypographySize } from '../src/theme';

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

    expect(applyTypographyPreferences(15.6, 30)).toEqual({ uiFontSize: 16, editorFontSize: 18 });
    expect(setProperty).toHaveBeenNthCalledWith(1, '--app-ui-font-size', '16px');
    expect(setProperty).toHaveBeenNthCalledWith(2, '--app-editor-font-size', '18px');
    applyEditorFont('Fira Code');
    expect(setProperty).toHaveBeenNthCalledWith(3, '--app-editor-font-family', '"Fira Code", "JetBrains Mono", "SFMono-Regular", Consolas, monospace');
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

  it('maps global weight semantics one step lighter without flattening hierarchy', () => {
    const styles = fs.readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    expect(styles).toContain('--font-weight-medium: 400;');
    expect(styles).toContain('--font-weight-semibold: 500;');
    expect(styles).toContain('--font-weight-bold: 600;');
  });

  it('keeps editor typography isolated from chat code and covers all CodeMirror views', () => {
    const editorTheme = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/editor-extensions.ts'), 'utf8');
    const markdown = fs.readFileSync(path.resolve(__dirname, '../src/components/prompt-kit/markdown.tsx'), 'utf8');
    const fileViewer = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/WorkspaceFileEditor.tsx'), 'utf8');
    const diffViewer = fs.readFileSync(path.resolve(__dirname, '../src/components/workspace/files/TurnFileWorkspacePanel.tsx'), 'utf8');
    expect(editorTheme).toContain("fontFamily: 'var(--app-editor-font-family, ui-monospace)'");
    expect(editorTheme).toContain("fontSize: 'var(--app-editor-font-size, 12px)'");
    expect(fileViewer).toContain('<CodeMirror');
    expect(diffViewer).toContain('unifiedMergeView({');
    expect(markdown).toContain('var(--app-ui-code-font-size)');
    expect(markdown).not.toContain('var(--app-editor-font-size)');
  });

  it('uses UI-derived typography tokens in the conversation sidebar', () => {
    const sidebar = fs.readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'), 'utf8');
    expect(sidebar).toContain('text-ui-compact');
    expect(sidebar).toContain('text-ui-micro');
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
