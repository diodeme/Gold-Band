import { resolve } from 'node:path';
import { normalizePath } from 'vite';
import { describe, expect, it } from 'vitest';
import config from '../../marketing/demo/vite.config';

describe('demo runtime module identity', () => {
  it('uses the same canonical module ID for desktop, browser and direct imports', () => {
    const plugin = (config as { plugins: { name?: string; resolveId?: (source: string, importer?: string) => string | undefined }[] }).plugins.find((entry) => entry.name === 'demo-runtime')!;
    const importer = resolve('web/src/api/client.ts');
    const canonical = normalizePath(resolve('marketing/demo/runtime.ts'));
    expect(plugin.resolveId!('./browser', importer)).toBe(canonical);
    expect(plugin.resolveId!('./desktop', importer)).toBe(canonical);
    expect(plugin.resolveId!('./browser', resolve('another/client.ts'))).toBeUndefined();
  });
});
