import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('themed tabs surface', () => {
  it('keeps the default tab track distinct from light workspace backgrounds', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/ui/tabs.tsx'),
      'utf8',
    );

    expect(source).toContain(
      'default: "bg-secondary ring-1 ring-inset ring-border/70"',
    );
    expect(source).not.toContain('default: "bg-muted"');
    expect(source).toContain('line: "gap-1 bg-transparent"');
    expect(source).toContain('data-[state=active]:bg-background');
  });
});
