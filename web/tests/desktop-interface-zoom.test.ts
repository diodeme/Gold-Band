import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  INTERFACE_ZOOM_DEFAULT,
  INTERFACE_ZOOM_MAX,
  INTERFACE_ZOOM_MIN,
  clampInterfaceZoomPercent,
  normalizeInterfaceZoomPercent,
  parseInterfaceZoomInput,
  withInterfaceZoomPercent,
} from '@/lib/desktop-zoom';

describe('interface zoom preference', () => {
  it('clamps and rounds zoom percents to the configured step', () => {
    expect(normalizeInterfaceZoomPercent(100)).toBe(100);
    expect(normalizeInterfaceZoomPercent(133)).toBe(135);
    expect(normalizeInterfaceZoomPercent(12)).toBe(INTERFACE_ZOOM_MIN);
    expect(normalizeInterfaceZoomPercent(400)).toBe(INTERFACE_ZOOM_MAX);
    expect(normalizeInterfaceZoomPercent(Number.NaN)).toBe(INTERFACE_ZOOM_DEFAULT);
  });

  it('bounds the manual range to 50-250', () => {
    expect(INTERFACE_ZOOM_MIN).toBe(50);
    expect(INTERFACE_ZOOM_MAX).toBe(250);
    // Manual input clamps to the nearest bound without step snapping.
    expect(clampInterfaceZoomPercent(12)).toBe(50);
    expect(clampInterfaceZoomPercent(400)).toBe(250);
    expect(clampInterfaceZoomPercent(73)).toBe(73);
  });

  it('parses manual input: invalid text is rejected, numbers clamp', () => {
    expect(parseInterfaceZoomInput('125')).toEqual({ kind: 'valid', percent: 125 });
    expect(parseInterfaceZoomInput(' 118 ')).toEqual({ kind: 'valid', percent: 118 });
    expect(parseInterfaceZoomInput('12')).toEqual({ kind: 'valid', percent: 50 });
    expect(parseInterfaceZoomInput('999')).toEqual({ kind: 'valid', percent: 250 });
    expect(parseInterfaceZoomInput('abc')).toEqual({ kind: 'invalid' });
    expect(parseInterfaceZoomInput('')).toEqual({ kind: 'invalid' });
    expect(parseInterfaceZoomInput('12x5')).toEqual({ kind: 'invalid' });
  });

  it('stages the zoom percent into the personalization preference', () => {
    const personalization = withInterfaceZoomPercent({
      schemaVersion: 5,
      typography: {
        ui: { fontStack: { source: 'theme' }, fontSize: { source: 'theme' } },
        editor: { fontStack: { source: 'theme' }, fontSize: { source: 'theme' } },
      },
      wallpaper: {
        byColorScheme: {
          light: { image: { source: 'theme' }, opacityPercent: 60 },
          dark: { image: { source: 'theme' }, opacityPercent: 60 },
        },
      },
      avatars: { agent: { image: { source: 'theme' }, shape: { source: 'theme' } }, user: { image: { source: 'theme' }, shape: { source: 'theme' } } },
      interfaceZoomPercent: 100,
      workflowGraphDirection: 'auto',
    }, 153);
    // Manual input keeps the exact integer instead of snapping to the slider step.
    expect(personalization.interfaceZoomPercent).toBe(153);
    expect(personalization.workflowGraphDirection).toBe('auto');
  });
});

describe('interface zoom wiring contract', () => {
  it('exposes the zoom row in settings with a commit-only slider', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    expect(source).toContain('data-interface-zoom-row="true"');
    expect(source).toContain('data-interface-zoom-invalid="true"');
    expect(source).toContain('parseInterfaceZoomInput');
    expect(source).toContain('chooseZoomPercent');
    expect(source).toContain('onValueCommit');
    // Dragging must not re-apply the webview zoom mid-gesture (it re-layouts
    // the page and kicks the slider away from the pointer); only the local
    // draft follows the thumb and the commit applies + persists the zoom.
    expect(source).not.toContain('previewZoomPercent');
  });

  it('applies the persisted zoom at startup and persists through the shared save path', () => {
    const mainRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
    expect(mainRs).toContain('commands::apply_interface_zoom_to_app_webviews');

    const commandsRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/commands.rs'), 'utf8');
    expect(commandsRs).toContain('pub fn apply_desktop_zoom');
    // The full preference save path applies the persisted zoom after saving.
    expect(commandsRs.indexOf('apply_interface_zoom_to_app_webviews(&app_handle, personalization.interface_zoom_percent)')).toBeGreaterThan(0);
  });

  it('persists shortcut zoom steps through the single-field zoom save', () => {
    // Startup defaults can still be live while the bootstrap IPC is in flight;
    // a shortcut step must never round-trip a full preference object built from
    // them. The backend changes exactly one field of the canonical fact.
    const mainRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
    expect(mainRs).toContain('save_interface_zoom_percent,');

    const app = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(app).toContain('saveInterfaceZoomPercent(percent)');
  });

  it('installs the shortcut driver bridge in the app shell', () => {
    const app = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(app).toContain('useDesktopZoomShortcuts(');
  });

  it('keeps native child webviews glued to their DOM slots at any zoom', () => {
    // The in-app browser pages and the address suggestion list live in native
    // child webviews placed in physical pixels; the frontend must convert CSS
    // bounds with devicePixelRatio (system scale × interface zoom) and the
    // backend must consume them as physical units.
    const host = readFileSync(path.resolve(__dirname, '../src/components/workspace/browser/browser-webview-host.ts'), 'utf8');
    expect(host).toContain('export function toPhysicalBrowserBounds');
    expect(host).toContain('toPhysicalBrowserBounds(roundBounds(bounds))');
    expect(host).toContain('toPhysicalBrowserBounds(target)');

    // The conversion lives in exactly one place; the overlay reuses it instead
    // of keeping a drifting inline copy.
    const overlay = readFileSync(path.resolve(__dirname, '../src/components/workspace/browser/browser-address-overlay.ts'), 'utf8');
    expect(overlay).toContain('bounds: toPhysicalBrowserBounds(overlayBounds)');
    expect(overlay).not.toContain('window.devicePixelRatio');

    const browserRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/browser.rs'), 'utf8');
    expect(browserRs).toContain('PhysicalPosition::new');
    expect(browserRs).toContain('PhysicalSize::new');
    expect(browserRs).not.toContain('LogicalPosition');
  });

  it('mirrors the interface zoom onto the address-suggestion overlay webview', () => {
    // The suggestion overlay renders app-styled rows at fixed CSS sizes inside
    // dpr-scaled bounds; without the same page zoom its box and content diverge
    // by exactly the zoom factor. Browser pages serve arbitrary remote content
    // and are out of scope.
    const browserRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/browser.rs'), 'utf8');
    expect(browserRs).toContain('pub(crate) fn apply_interface_zoom_to_address_suggestions');
    // Creation reads the persisted zoom once so a first-show overlay matches.
    expect(browserRs).toContain('apply_interface_zoom_to_address_suggestions(app, persisted_interface_zoom_percent(app));');

    // Every later zoom change (startup, settings commit, live preview) reaches
    // the live overlay through the shared webview zoom applier.
    const commandsRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/commands.rs'), 'utf8');
    expect(commandsRs).toContain('crate::browser::apply_interface_zoom_to_address_suggestions(app_handle, percent);');
  });
});
