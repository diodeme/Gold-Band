import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import { isDesktopPageZoomShortcut } from '@/lib/desktop-page-zoom';

describe('desktop page zoom boundary', () => {
  it('enables the Windows WebView2 pinch input before the webview is built', () => {
    const source = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');

    expect(source).toContain('window.zoom_hotkeys_enabled = true;');
  });

  it('prevents page zoom while preserving ordinary keyboard input', () => {
    expect(isDesktopPageZoomShortcut('+', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('=', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('-', false, true)).toBe(true);
    expect(isDesktopPageZoomShortcut('0', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('0', false, false)).toBe(false);
    expect(isDesktopPageZoomShortcut('a', true, false)).toBe(false);
  });

  it('installs one capture guard without stopping image viewport propagation', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/lib/desktop-page-zoom.ts'), 'utf8');

    expect(source).toContain("addEventListener('wheel', preventPageWheelZoom, { capture: true, passive: false })");
    expect(source).toContain('if (event.ctrlKey) event.preventDefault();');
    expect(source).not.toContain('stopPropagation');
  });
});
