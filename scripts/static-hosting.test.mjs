import assert from 'node:assert/strict';
import test from 'node:test';
import { get } from 'node:http';
import { get as getHttps } from 'node:https';
import { gunzipSync } from 'node:zlib';

const origin = process.env.STATIC_PREVIEW_URL;
const site = process.env.SITE_MOUNT || '/product';
const demo = process.env.DEMO_MOUNT || '/nested/demo';
function request(path, headers = {}) {
  const url = new URL(path, origin);
  return new Promise((resolve, reject) => {
    const req = (url.protocol === 'https:' ? getHttps : get)(url, { headers }, response => {
      const chunks = [];
      response.on('data', chunk => chunks.push(chunk));
      response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks) }));
      response.on('error', reject);
    });
    req.on('error', reject);
    req.setTimeout(15000, () => req.destroy(new Error('HTTP timeout')));
  });
}
test('static host deployment HTTP contract', { skip: !origin }, async t => {
  for (const mount of [site, demo]) {
    await t.test(`${mount}: redirect, SPA, missing resources and cache`, async () => {
      const redirect = await request(mount);
      assert.equal(redirect.status, 308);
      assert.equal(redirect.headers.location, mount + '/');
      const entry = await request(mount + '/');
      assert.equal(entry.status, 200);
      assert.match(entry.headers['content-type'], /text\/html/);
      assert.equal(entry.headers['cache-control'], 'no-cache');
      const deep = await request(mount + '/en/documentation');
      assert.equal(deep.status, 200);
      assert.deepEqual(deep.body, entry.body);
      for (const path of ['/assets/absent.js', '/data/absent.json', '/media/absent', '/agent-icons/absent.svg']) {
        assert.equal((await request(mount + path)).status, 404, path);
      }
      const js = entry.body.toString().match(/src="([^"]+\.js)"/)?.[1];
      assert(js?.startsWith(mount + '/assets/'));
      const plain = await request(js);
      assert.equal(plain.status, 200);
      assert.equal(plain.headers['cache-control'], 'public, max-age=31536000, immutable');
      const compressed = await request(js, { 'Accept-Encoding': 'gzip' });
      assert.equal(compressed.headers['content-encoding'], 'gzip');
      assert.match(compressed.headers.vary, /Accept-Encoding/i);
      assert.deepEqual(gunzipSync(compressed.body), plain.body);
      assert(compressed.body.length < plain.body.length);
      const icon = await request(mount + '/logo.svg');
      assert.equal(icon.status, 200);
      assert.equal(icon.headers['cache-control'], 'no-cache');
      const conditional = await request(mount + '/logo.svg', { 'If-None-Match': icon.headers.etag });
      assert.equal(conditional.status, 304);
    });
  }
  await t.test('unmounted paths and execution entries are unavailable', async () => {
    for (const path of ['/', '/api/run', '/recording/index.html', site + '/preview.html', demo + '/preview.html']) {
      assert.equal((await request(path)).status, 404, path);
    }
  });
});
