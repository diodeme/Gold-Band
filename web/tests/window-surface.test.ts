import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { desktopThemeWindowSurface } from '../src/theme';

describe('desktop window surface', () => {
  it('maps every concrete theme to the workspace surface used during native resize', () => {
    expect(desktopThemeWindowSurface('light')).toBe('#f1f2f5');
    expect(desktopThemeWindowSurface('light-warm')).toBe('#f0ede7');
    expect(desktopThemeWindowSurface('dark')).toBe('#0e0e0e');
    expect(desktopThemeWindowSurface('black')).toBe('#060709');
  });

  it('uses the Windows composition resize path and keeps the window hidden until the first themed frame', () => {
    const config = JSON.parse(readFileSync(path.resolve(__dirname, '../../src-tauri/tauri.conf.json'), 'utf8'));
    const mainWindow = config.app.windows[0];

    expect(mainWindow.decorations).toBe(false);
    expect(mainWindow.visible).toBe(false);
    expect(mainWindow.transparent).toBe(false);
    expect(mainWindow.shadow).toBe(true);
    expect(mainWindow.backgroundColor).toBe('#00000000');

    const mainSource = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
    expect(mainSource).toContain('#[cfg(target_os = "windows")]');
    expect(mainSource).toContain('window.transparent = true;');
    expect(mainSource).toContain('window.shadow = true;');
  });

  it('grants host background synchronization and first-frame reveal permissions', () => {
    const capability = JSON.parse(readFileSync(path.resolve(__dirname, '../../src-tauri/capabilities/default.json'), 'utf8'));

    expect(capability.permissions).toContain('core:window:allow-set-background-color');
    expect(capability.permissions).toContain('core:window:allow-show');
    expect(capability.permissions).not.toContain('core:webview:allow-set-webview-background-color');
  });
});
