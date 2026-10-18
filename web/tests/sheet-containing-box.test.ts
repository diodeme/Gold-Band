/** @vitest-environment jsdom */
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { sheetContainingBox, sheetResizeWidth, sheetSizeLimit } from '@/components/ui/sheet';

describe('sheet containing box', () => {
  it('uses the demo window when the overlay host is inside it', () => {
    document.body.innerHTML = '<div id="client"><div id="gold-band-overlay-portal-host"></div></div>';
    const client = document.getElementById('client')!;
    client.getBoundingClientRect = () => ({ x: 80, y: 24, width: 640, height: 720, left: 80, right: 720, top: 24, bottom: 744, toJSON() { return {}; } });
    expect(sheetContainingBox(document.getElementById('gold-band-overlay-portal-host'), { left: 0, right: 1200, width: 1200 })).toEqual({ left: 80, right: 720, width: 640 });
  });

  it('uses the browser viewport when the overlay host is on the document body', () => {
    document.body.innerHTML = '<div id="gold-band-overlay-portal-host"></div>';
    const fallback = { left: 0, right: 1280, width: 1280 };
    expect(sheetContainingBox(document.getElementById('gold-band-overlay-portal-host'), fallback)).toEqual(fallback);
  });

  it('falls back when the nested window has not been laid out', () => {
    document.body.innerHTML = '<div id="client"><div id="gold-band-overlay-portal-host"></div></div>';
    const fallback = { left: 0, right: 1200, width: 1200 };
    expect(sheetContainingBox(document.getElementById('gold-band-overlay-portal-host'), fallback)).toEqual(fallback);
  });

  it('keeps a sheet inside a window that is narrower than the browser viewport', () => {
    const box = { left: 80, right: 720, width: 640 };
    expect(sheetSizeLimit(box.width, 1280, 16)).toBe(624);
    expect(sheetSizeLimit(1400, 1280, 16)).toBe(1280);
    expect(sheetResizeWidth('right', 400, box)).toBe(320);
    expect(sheetResizeWidth('left', 400, box)).toBe(320);
  });

  it('does not size embedded sheets from the browser viewport', () => {
    const sheet = readFileSync('web/src/components/ui/sheet.tsx', 'utf8');
    const shell = readFileSync('web/src/components/workspace/WorkspaceShell.tsx', 'utf8');
    const demo = readFileSync('marketing/demo/DemoApp.tsx', 'utf8');
    expect(sheet).toContain('sheetContainingBox');
    expect(sheet).not.toContain('window.innerWidth - clientX');
    expect(sheet).not.toContain('window.innerWidth - viewportMargin');
    expect(shell).toContain('w-[min(92%,44rem)]');
    expect(shell).not.toContain('92vw');
    expect(demo).not.toContain('85vw');
  });
});
