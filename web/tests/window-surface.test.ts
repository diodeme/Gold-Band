import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { getThemePackage } from '../src/theme';

describe('desktop window surface', () => {
  it('maps every concrete theme to the workspace surface used during native resize', () => {
    expect(getThemePackage('builtin.gold-band').schemes.light.windowSurface).toBe('#ffffff');
    expect(getThemePackage('builtin.tech-neutral').schemes.light.windowSurface).toBe('#ffffff');
    expect(getThemePackage('builtin.gold-band').schemes.dark.windowSurface).toBe('#0f0f0f');
    expect(getThemePackage('builtin.tech-neutral').schemes.dark.windowSurface).toBe('#111111');
    expect(getThemePackage('builtin.glass').schemes.light.windowSurface).toBe('#c7c7ca');
    expect(getThemePackage('builtin.glass').schemes.dark.windowSurface).toBe('#111113');
    expect(getThemePackage('builtin.neo-brutalist').schemes.light.windowSurface).toBe('#e8e6df');
    expect(getThemePackage('builtin.neo-brutalist').schemes.dark.windowSurface).toBe('#111111');
  });

  it('uses the Windows composition resize path and keeps the window hidden until the first themed frame', () => {
    const config = JSON.parse(readFileSync(path.resolve(__dirname, '../../src-tauri/tauri.conf.json'), 'utf8'));
    const mainWindow = config.app.windows[0];

    expect(mainWindow.decorations).toBe(false);
    expect(mainWindow.visible).toBe(false);
    expect(mainWindow.transparent).toBe(false);
    expect(mainWindow.shadow).toBe(true);
    expect(mainWindow.backgroundColor).toBe('#00000000');
    expect(mainWindow.minWidth).toBeUndefined();
    expect(mainWindow.minHeight).toBeUndefined();

    const mainSource = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
    expect(mainSource).toContain('#[cfg(target_os = "windows")]');
    expect(mainSource).toContain('window.transparent = true;');
    expect(mainSource).toContain('let desktop_window_chrome = window_chrome::desktop_window_chrome_vm();');
    expect(mainSource).toContain('window.shadow = desktop_window_chrome.native_shadow;');
  });

  it('selects the Win10 outline fallback from the real Windows build number', () => {
    const cargo = readFileSync(path.resolve(__dirname, '../../src-tauri/Cargo.toml'), 'utf8');
    const chromeSource = readFileSync(path.resolve(__dirname, '../../src-tauri/src/window_chrome.rs'), 'utf8');

    expect(cargo).toContain('windows-version = "0.1.7"');
    expect(chromeSource).toContain('windows_version::OsVersion::current()');
    expect(chromeSource).toContain('const WINDOWS_11_MINIMUM_BUILD: u32 = 22_000;');
    expect(chromeSource).toContain('DesktopWindowFrameStyle::AppOutline');
    expect(chromeSource).toContain('DesktopWindowFrameStyle::NativeCompositor');
    expect(chromeSource).toContain('native_shadow: false');
    expect(chromeSource).toContain('native_shadow: true');
  });

  it('grants host background synchronization and first-frame reveal permissions', () => {
    const capability = JSON.parse(readFileSync(path.resolve(__dirname, '../../src-tauri/capabilities/default.json'), 'utf8'));

    expect(capability.permissions).toContain('core:window:allow-set-background-color');
    expect(capability.permissions).toContain('core:window:allow-inner-size');
    expect(capability.permissions).toContain('core:window:allow-scale-factor');
    expect(capability.permissions).toContain('core:window:allow-set-min-size');
    expect(capability.permissions).toContain('core:window:allow-set-size');
    expect(capability.permissions).toContain('core:window:allow-show');
    expect(capability.permissions).not.toContain('core:webview:allow-set-webview-background-color');
  });

  it('applies the page minimum before revealing the initially hidden native window', () => {
    const appSource = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    const minimumIndex = appSource.indexOf('await syncDesktopWindowMinimum(');
    const showIndex = appSource.indexOf('await appWindow.show()');

    expect(minimumIndex).toBeGreaterThan(-1);
    expect(showIndex).toBeGreaterThan(minimumIndex);
  });
});
