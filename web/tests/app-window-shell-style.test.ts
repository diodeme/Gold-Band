import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

describe('App window shell style', () => {
  it('keeps a dedicated top outline for frameless Windows windows', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).toContain('--gold-window-outline');
    expect(styles).toContain('--gold-window-top-outline');
    expect(styles).toContain('.app-window-shell::before');
    expect(styles).toContain('border-top-color: var(--gold-window-top-outline)');
    expect(styles).toContain('pointer-events: none');
  });
});
