import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');

export async function composeWebsite(site, demo) {
  const siteIndex = await readFile(join(site, 'index.html'));
  const demoIndex = await readFile(join(demo, 'index.html'));
  const files = await readdir(join(demo, 'assets'), { withFileTypes: true });
  // Both Vite builds use fingerprinted root assets. Validate collisions before copying anything.
  for (const entry of files) {
    if (!entry.isFile()) throw new Error('site.unexpected-build-asset');
    try {
      const existing = await readFile(join(site, 'assets', entry.name));
      const incoming = await readFile(join(demo, 'assets', entry.name));
      if (digest(existing) !== digest(incoming)) throw new Error('site.build-asset-collision');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  await mkdir(join(site, 'assets'), { recursive: true });
  await mkdir(join(site, 'demo'), { recursive: true });
  for (const entry of files) await copyFile(join(demo, 'assets', entry.name), join(site, 'assets', entry.name));
  await writeFile(join(site, 'demo', 'index.html'), demoIndex);
  const websiteRoutes = ['', '/zh', '/en'].flatMap(prefix => [
    prefix || '/', ...(prefix ? [`${prefix}/`] : []),
    `${prefix}/documentation`, `${prefix}/documentation/`,
    ...(prefix ? [`${prefix}/demo`, `${prefix}/demo/`] : []),
  ]);
  // `_redirects` only works on hosts that implement it. Mirror every shell route into a real
  // directory index so plain static servers resolve the same deep links.
  const shellRoutes = [...new Set(websiteRoutes
    .filter(route => route !== '/')
    .map(route => route.replace(/\/$/, '')))];
  for (const route of shellRoutes) {
    const directory = join(site, ...route.split('/').filter(Boolean));
    await mkdir(directory, { recursive: true });
    await writeFile(join(directory, 'index.html'), siteIndex);
  }
  await writeFile(join(site, '_redirects'), [
    '/demo/* /demo/index.html 200',
    '/demo /demo/index.html 200',
    ...websiteRoutes.map(route => `${route} /index.html 200`),
    '/* /index.html 404',
    '',
  ].join('\n'));
  await writeFile(join(site, '_headers'), '/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n/*.html\n  Cache-Control: no-cache\n');
  return { demo: '/demo/', assets: files.length, shellRoutes: shellRoutes.length };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await composeWebsite(resolve('.codex-temp/site-dist'), resolve('.codex-temp/demo-dist'))));
}
