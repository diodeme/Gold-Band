import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

describe('sheet close focus treatment', () => {
  it('shows a keyboard-only focus ring without styling every open sheet as selected', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/components/ui/sheet.tsx'), 'utf8');
    const closeButton = source.match(/<SheetPrimitive\.Close className="([^"]+)"/)?.[1] ?? '';

    expect(closeButton).toContain('focus-visible:ring-2');
    expect(closeButton).not.toContain('focus:ring-2');
    expect(closeButton).not.toContain('data-[state=open]:bg-secondary');
  });

  it('focuses the compact workspace dialog instead of auto-selecting its close button', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/components/workspace/WorkspaceShell.tsx'), 'utf8');

    expect(source).toContain('ref={compactSheetContentRef}');
    expect(source).toContain('onOpenAutoFocus={(event) => {');
    expect(source).toContain('compactSheetContentRef.current?.focus({ preventScroll: true });');
  });
});
