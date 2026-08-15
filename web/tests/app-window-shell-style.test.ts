import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('App window shell style', () => {
  it('uses a viewport-safe Win10 inset frame without duplicating Win11 native rounding', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const generatedThemeStyles = readFileSync(
      path.resolve(__dirname, '../src/themes/generated/builtin-themes.css'),
      'utf8',
    );

    expect(styles).toContain('--gold-window-outline');
    expect(styles).toContain('--gold-window-edge-shadow');
    expect(styles).not.toContain('--gold-window-top-outline');
    expect(styles).toContain(".app-window-shell[data-window-frame-style='app-outline']");
    expect(styles).toContain('inset 0 0 0 1px var(--gold-window-outline)');
    expect(styles).toContain('inset 0 0 8px var(--gold-window-edge-shadow)');
    expect(styles).not.toContain('outline-offset: -1px');
    expect(styles).not.toMatch(/^\.app-window-shell \{/mu);
    expect(styles).not.toMatch(/^\.app-window-shell::before/mu);
    expect(styles).toContain('@import "./themes/generated/builtin-themes.css"');
    expect(generatedThemeStyles).toContain(":root[data-theme='builtin.gold-band'] .app-window-shell");
    expect(generatedThemeStyles).toContain(":root[data-theme='builtin.tech-neutral'] .app-window-shell");
    expect(styles).not.toContain('z-index: 60');
  });

  it('binds the host-provided frame policy on both desktop shells', () => {
    const workbenchShell = readFileSync(path.resolve(__dirname, '../src/components/Shell.tsx'), 'utf8');
    const conversationShell = readFileSync(path.resolve(__dirname, '../src/components/workspace/WorkspaceShell.tsx'), 'utf8');

    expect(workbenchShell).toContain('data-window-frame-style={windowFrameStyle}');
    expect(conversationShell).toContain('data-window-frame-style={windowFrameStyle}');
  });

  it('lets the WebView follow the real viewport instead of clipping at desktop minimum dimensions', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).not.toContain('min-w-[1040px]');
    expect(styles).not.toContain('min-h-[680px]');
  });

  it('uses the shared shadcn resizable handles with low-contrast dividers', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const shell = readFileSync(path.resolve(__dirname, '../src/components/workspace/WorkspaceShell.tsx'), 'utf8');
    const workbenchShell = readFileSync(path.resolve(__dirname, '../src/components/Shell.tsx'), 'utf8');

    expect(styles).toContain('--color-workspace-divider: var(--workspace-divider)');
    expect(styles).toContain('--workspace-divider: color-mix(in srgb, var(--sidebar-border) 70%, var(--gold-workspace))');
    expect(shell).toContain('ResizablePanelGroup');
    expect(shell).toContain('data-testid="workspace-left-resize-handle"');
    expect(shell).toContain('data-testid="workspace-right-resize-handle"');
    expect(shell).toContain('border-t border-workspace-divider bg-gold-workspace');
    expect(shell).toContain('bg-workspace-divider hover:bg-primary/30');
    expect(shell).toContain("'border-t border-workspace-divider',");
    expect(shell).not.toContain('bg-sidebar-border/70 hover:bg-primary/30');
    expect(workbenchShell).toContain('border-l border-t border-workspace-divider');
    expect(shell).not.toContain('mousemove');
    expect(shell).not.toContain('mouseup');
  });

  it('elevates the shared main workspace surface with the composer material shadow', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const conversationShell = readFileSync(path.resolve(__dirname, '../src/components/workspace/WorkspaceShell.tsx'), 'utf8');
    const workbenchShell = readFileSync(path.resolve(__dirname, '../src/components/Shell.tsx'), 'utf8');
    const elevation = '[box-shadow:var(--workspace-main-surface-shadow)]';

    expect(conversationShell).toContain(elevation);
    expect(workbenchShell).toContain(elevation);
    expect(conversationShell).toContain('className="min-h-0 flex-1 bg-sidebar !overflow-x-clip !overflow-y-visible"');
    expect(conversationShell).toContain("'relative z-10 min-w-0 [box-shadow:var(--workspace-main-surface-shadow)]'");
    expect(conversationShell).toContain("'relative z-10 border-t border-workspace-divider [box-shadow:var(--workspace-main-surface-shadow)]'");
    expect(conversationShell).toContain("<main className={cn('relative flex h-full");
    expect(conversationShell).not.toContain("<main className={cn('relative z-10");
    expect(conversationShell).not.toContain('bg-gold-workspace [box-shadow:var(--workspace-main-surface-shadow)]');
    expect(conversationShell).not.toContain("showLeft && 'rounded-tl-2xl border-l'");
    expect(workbenchShell).toContain('relative z-10 flex min-w-0');
    expect(styles).toContain('--workspace-main-surface-shadow:');
    expect(styles).toMatch(/--workspace-main-surface-shadow:\s*0 0 16px color-mix\(in srgb, var\(--gold-window-edge-shadow\) 85%, transparent\),\s*var\(--gb-material-shadow\),\s*var\(--gb-material-edge-shadow\);/u);
    expect(styles).not.toMatch(/--workspace-main-surface-shadow:[^;]*\d+px\s+\d+px/u);
  });

  it('lets the panel group grow and shrink the right workspace without imperative resize feedback', () => {
    const shell = readFileSync(path.resolve(__dirname, '../src/components/workspace/WorkspaceShell.tsx'), 'utf8');

    expect(shell).not.toContain('panel.resize(');
    expect(shell).not.toContain('panel.getSize(');
    expect(shell).toContain("groupResizeBehavior={rightPanelOwnsWindowResize ? 'preserve-pixel-size' : 'preserve-relative-size'}");
    expect(shell).toContain("groupResizeBehavior={rightPanelOwnsWindowResize ? 'preserve-relative-size' : 'preserve-pixel-size'}");
    expect(shell).toContain('maxSize={rightPanelMaxWidth}');
    expect(shell).toContain('onResize={trackRightPanelSize}');
    expect(shell).toContain('onPointerDown={beginRightPanelResize}');
    expect(shell).toContain('flushSync(() => setRightPanelResizeActive(true))');
    expect(shell).toContain('onPointerUp={endRightPanelResize}');
    expect(shell).toContain('panelRef={leftPanelRef}');
    expect(shell).toContain('panelRef={rightPanelRef}');
    expect(shell).toContain('collapsedSize={0}');
    expect(shell).toContain('if (panel.isCollapsed()) panel.expand()');
    expect(shell).toContain('panel.collapse()');
  });
});
