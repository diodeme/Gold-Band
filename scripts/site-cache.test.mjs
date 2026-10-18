import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveConfig } from 'vite';

test('website and demo dev servers own different dependency caches', async () => {
  const site = await resolveConfig({ configFile: 'marketing/site/vite.config.ts' }, 'serve');
  const demo = await resolveConfig({ configFile: 'marketing/demo/vite.config.ts' }, 'serve');
  assert.notEqual(site.cacheDir, demo.cacheDir);
});
