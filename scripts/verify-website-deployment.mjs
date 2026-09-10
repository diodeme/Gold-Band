import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');

export async function verifyWebsiteDeployment({ origin, directory, fetchImpl = fetch }) {
  const site = await readFile(resolve(directory, 'index.html'));
  const demo = await readFile(resolve(directory, 'demo/index.html'));
  const assets = [...new Set([site, demo].flatMap(html =>
    [...html.toString().matchAll(/(?:src|href)="(\/assets\/[^"?#]+\.(?:js|css))"/g)].map(match => match[1])))];
  if (!assets.length) throw new Error('site.deployment-assets-missing');
  const checks = [];
  for (const [path, expected] of [
    ['/', site], ['/zh/', site], ['/en/', site], ['/zh/documentation', site], ['/en/documentation', site],
    ['/demo/', demo], ['/demo/?language=en', demo],
    ...await Promise.all(assets.map(async path => [path, await readFile(resolve(directory, `.${path}`))])),
  ]) {
    const response = await fetchImpl(new URL(path, origin), {
      headers: { 'Accept-Encoding': 'br, gzip' }, signal: AbortSignal.timeout(30_000),
    });
    const bytes = Buffer.from(await response.arrayBuffer());
    const asset = path.startsWith('/assets/');
    const cache = response.headers.get('cache-control') ?? '';
    const encoding = response.headers.get('content-encoding');
    const type = response.headers.get('content-type') ?? '';
    const failures = [];
    if (response.status !== 200) failures.push('status');
    if (hash(bytes) !== hash(expected)) failures.push('content');
    if (asset ? !/(?:javascript|text\/css)/i.test(type) : !/text\/html/i.test(type)) failures.push('content-type');
    if (asset ? !/\bimmutable\b/i.test(cache) || !/\bmax-age=31536000\b/.test(cache)
      : !/\bno-cache\b|\bmax-age=0\b/.test(cache) || /\bimmutable\b/i.test(cache)) failures.push('cache');
    if (asset && !['gzip', 'br'].includes(encoding)) failures.push('compression');
    checks.push({ path, status: response.status, type, cache, encoding, bytes: bytes.length, failures });
  }
  for (const path of ['/deployment-probe-missing-page', '/assets/deployment-probe-missing.json']) {
    const missing = await fetchImpl(new URL(path, origin), { signal: AbortSignal.timeout(30_000) });
    await missing.body?.cancel();
    checks.push({ path, status: missing.status,
      failures: missing.status === 404 ? [] : ['missing-route-status'] });
  }
  return { origin, passed: checks.every(check => !check.failures.length), checks };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const report = await verifyWebsiteDeployment({
    origin: process.env.SITE_URL ?? 'http://127.0.0.1:1461',
    directory: resolve(process.env.SITE_DIST ?? '.codex-temp/site-dist'),
  });
  console.log(JSON.stringify(report, null, 2));
  if (!report.passed) process.exitCode = 1;
}
