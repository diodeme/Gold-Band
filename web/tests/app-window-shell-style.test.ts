import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('App window shell style', () => {
  it('uses a viewport-safe Win10 inset frame without duplicating Win11 native rounding', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).toContain('--gold-window-outline');
    expect(styles).toContain('--gold-window-edge-shadow');
    expect(styles).not.toContain('--gold-window-top-outline');
    expect(styles).toContain(".app-window-shell[data-window-frame-style='app-outline']");
    expect(styles).toContain('inset 0 0 0 1px var(--gold-window-outline)');
    expect(styles).toContain('inset 0 0 8px var(--gold-window-edge-shadow)');
    expect(styles).not.toContain('outline-offset: -1px');
    expect(styles).not.toContain('.app-window-shell {');
    expect(styles).not.toContain('.app-window-shell::before');
    expect(styles).not.toContain('z-index: 60');
  });

  it('binds the host-provided frame policy on both desktop shells', () => {
    const workbenchShell = readFileSync(path.resolve(__dirname, '../src/components/Shell.tsx'), 'utf8');
    const conversationShell = readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationShell.tsx'), 'utf8');

    expect(workbenchShell).toContain('data-window-frame-style={windowFrameStyle}');
    expect(conversationShell).toContain('data-window-frame-style={windowFrameStyle}');
  });

  it('lets the WebView follow the real viewport instead of clipping at desktop minimum dimensions', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).not.toContain('min-w-[1040px]');
    expect(styles).not.toContain('min-h-[680px]');
  });

  it('keeps the conversation sidebar resize target wide without painting a thick accent divider', () => {
    const shell = readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationShell.tsx'), 'utf8');

    expect(shell).toContain("'absolute right-0 top-0 bottom-0 z-20 w-2 cursor-col-resize bg-transparent'");
    expect(shell).toContain('data-testid="conversation-sidebar-resize-handle"');
    expect(shell).toContain('border-l border-t border-sidebar-border/70 rounded-tl-2xl');
    expect(shell).not.toContain('hover:bg-primary/40');
    expect(shell).not.toContain('active:bg-primary/60');
  });
});
