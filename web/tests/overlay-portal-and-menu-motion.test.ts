import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const readSource = (relativePath: string) => fs.readFileSync(path.resolve(__dirname, relativePath), 'utf8');

describe('overlay portal and positioned menu contracts', () => {
  it('mounts modal overlays in a dedicated unclipped body-level host', () => {
    const index = readSource('../index.html');
    const styles = readSource('../src/styles.css');
    const portalContainer = readSource('../src/lib/portal-container.ts');

    expect(index).toContain('<div id="gold-band-overlay-portal-host" data-overlay-portal-host="true"></div>');
    expect(index.indexOf('id="gold-band-overlay-portal-host"')).toBeGreaterThan(index.indexOf('id="root"'));
    expect(styles).toMatch(/#gold-band-overlay-portal-host\s*\{[^}]*overflow:\s*visible;[^}]*contain:\s*none;[^}]*transform:\s*none;[^}]*filter:\s*none;[^}]*backdrop-filter:\s*none;/su);
    expect(portalContainer).toContain("export const overlayPortalHostId = 'gold-band-overlay-portal-host';");

    for (const file of ['dialog.tsx', 'sheet.tsx', 'alert-dialog.tsx']) {
      const source = readSource(`../src/components/ui/${file}`);
      expect(source).toContain('container = getOverlayPortalHost()');
      expect(source).toContain('container={container}');
    }
  });

  it('keeps Radix menu positioners free of visual animation and clipping', () => {
    const dropdown = readSource('../src/components/ui/dropdown-menu.tsx');
    const context = readSource('../src/components/ui/context-menu.tsx');

    for (const [source, positionerSlot, visualSlot] of [
      [dropdown, 'dropdown-menu-content-positioner', 'dropdown-menu-content'],
      [dropdown, 'dropdown-menu-sub-content-positioner', 'dropdown-menu-sub-content'],
      [context, 'context-menu-content-positioner', 'context-menu-content'],
      [context, 'context-menu-sub-content-positioner', 'context-menu-sub-content'],
    ] as const) {
      const positionerStart = source.indexOf(`data-slot="${positionerSlot}"`);
      const visualStart = source.indexOf(`data-slot="${visualSlot}"`, positionerStart);
      const positioner = source.slice(positionerStart, visualStart);
      const visual = source.slice(visualStart, source.indexOf('>', source.indexOf('className={cn(', visualStart)) + 1);

      expect(positionerStart).toBeGreaterThanOrEqual(0);
      expect(visualStart).toBeGreaterThan(positionerStart);
      expect(positioner).not.toMatch(/animate-|slide-|zoom-|fade-|overflow-|filter|origin-/u);
      expect(visual).toMatch(/animate-in/u);
      expect(visual).toMatch(/slide-in-from/u);
      expect(visual).toMatch(/overflow-(?:hidden|x-hidden)/u);
    }
  });
});
