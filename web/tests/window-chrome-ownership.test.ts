import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('desktop window chrome ownership', () => {
  it('keeps native decoration policy in Rust while the frontend only reveals the prepared window', () => {
    const appSource = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    const rustMainSource = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');

    expect(rustMainSource).toContain('#[cfg(target_os = "macos")]');
    expect(rustMainSource).toContain('window.decorations = true;');
    expect(rustMainSource).toContain('window.title_bar_style = tauri::TitleBarStyle::Overlay;');
    expect(rustMainSource).toContain('window.hidden_title = true;');
    expect(appSource).toContain('syncDesktopWindowSurface(resolveThemePreference(preferences.theme))');
    expect(appSource).toContain('appWindow.show()');
    expect(appSource).not.toContain('.setDecorations(');
    expect(appSource).not.toContain('.setTitleBarStyle(');
  });

  it('does not grant the WebView permission to change native decorations', () => {
    const capability = JSON.parse(
      readFileSync(path.resolve(__dirname, '../../src-tauri/capabilities/default.json'), 'utf8'),
    ) as { permissions: string[] };

    expect(capability.permissions).not.toContain('core:window:allow-set-decorations');
    expect(capability.permissions).toContain('core:window:allow-set-background-color');
    expect(capability.permissions).toContain('core:window:allow-show');
  });
});
