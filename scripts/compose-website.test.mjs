import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { composeWebsite } from './compose-website.mjs';

test('composes independently built applications without replacing the home entry', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gold-band-compose-'));
  try {
    const site = join(root, 'site');
    const demo = join(root, 'demo');
    for (const directory of [site, demo]) await mkdir(join(directory, 'assets'), { recursive: true });
    await writeFile(join(site, 'index.html'), '<main>site</main>');
    await writeFile(join(demo, 'index.html'), '<script src="/assets/demo-hash.js"></script>');
    await writeFile(join(demo, 'assets/demo-hash.js'), 'export const demo = true;');
    await composeWebsite(site, demo);
    assert.equal(await readFile(join(site, 'index.html'), 'utf8'), '<main>site</main>');
    assert.equal(await readFile(join(site, 'demo/index.html'), 'utf8'), await readFile(join(demo, 'index.html'), 'utf8'));
    assert.equal(await readFile(join(site, 'assets/demo-hash.js'), 'utf8'), 'export const demo = true;');
    assert.match(await readFile(join(site, '_redirects'), 'utf8'), /^\/demo\/\* \/demo\/index.html 200/);
    const redirects = await readFile(join(site, '_redirects'), 'utf8');
    assert.match(redirects, /^\/\* \/index.html 404$/m, 'unknown paths must not be successful SPA responses');
    assert.doesNotMatch(redirects, /^\/\* .* 200$/m);
    for (const route of ['/demo', '/zh/', '/en/', '/zh/documentation', '/en/documentation']) {
      assert.ok(redirects.split('\n').some(line => line.startsWith(`${route} `) && line.endsWith(' 200')), route);
    }
    // Hosts without `_redirects` support must still resolve these routes from real files.
    for (const file of ['zh/index.html', 'en/index.html', 'documentation/index.html',
      'zh/documentation/index.html', 'en/documentation/index.html', 'zh/demo/index.html', 'en/demo/index.html']) {
      assert.equal(await readFile(join(site, file), 'utf8'), '<main>site</main>', file);
    }
    assert.equal(await readFile(join(site, 'demo/index.html'), 'utf8'), '<script src="/assets/demo-hash.js"></script>');
    await writeFile(join(demo, 'assets/demo-hash.js'), 'collision');
    await assert.rejects(composeWebsite(site, demo), /asset-collision/);
    assert.equal(await readFile(join(site, 'assets/demo-hash.js'), 'utf8'), 'export const demo = true;');
  } finally { await rm(root, { recursive: true, force: true }); }
});
