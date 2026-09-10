import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { verifyWebsiteDeployment } from './verify-website-deployment.mjs';

test('deployment acceptance rejects SPA asset fallback, stale HTML, uncached and uncompressed assets', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'website-deployment-'));
  try {
    await mkdir(join(directory, 'assets'));
    await mkdir(join(directory, 'demo'));
    const site = '<script src="/assets/site-hash.js"></script>';
    const demo = '<script src="/assets/demo-hash.js"></script>';
    const files = new Map([['/assets/site-hash.js', 'site();'], ['/assets/demo-hash.js', 'demo();']]);
    await writeFile(join(directory, 'index.html'), site);
    await writeFile(join(directory, 'demo/index.html'), demo);
    for (const [path, body] of files) await writeFile(join(directory, path), body);
    let broken = false;
    const fetchImpl = async url => {
      const asset = files.has(url.pathname);
      if (url.pathname.includes('missing')) return new Response(broken ? site : null, { status: broken ? 200 : 404 });
      return new Response(asset ? files.get(url.pathname) : broken ? 'outdated' : url.pathname.startsWith('/demo/') ? demo : site, {
        headers: {
          'Content-Type': asset ? 'text/javascript' : 'text/html',
          'Cache-Control': asset && !broken ? 'public, max-age=31536000, immutable' : 'no-cache',
          ...(asset && !broken ? { 'Content-Encoding': 'br' } : {}),
        },
      });
    };
    assert.equal((await verifyWebsiteDeployment({ origin: 'https://example.test', directory, fetchImpl })).passed, true);
    broken = true;
    const report = await verifyWebsiteDeployment({ origin: 'https://example.test', directory, fetchImpl });
    assert.equal(report.passed, false);
    assert.deepEqual(report.checks.find(check => check.path === '/zh/').failures, ['content']);
    assert.deepEqual(report.checks.find(check => check.path === '/assets/site-hash.js').failures, ['cache', 'compression']);
    assert.deepEqual(report.checks.at(-2).failures, ['missing-route-status']);
    assert.deepEqual(report.checks.at(-1).failures, ['missing-route-status']);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
