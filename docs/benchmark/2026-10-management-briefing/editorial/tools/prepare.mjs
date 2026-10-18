import { readFile, writeFile, mkdir, copyFile, constants } from 'node:fs/promises';

const root = new URL('../', import.meta.url);
const demos = new URL('../../design-demos/', import.meta.url);
await mkdir(new URL('slides/', root), { recursive: true });
await mkdir(new URL('previews/', root), { recursive: true });
for (const asset of ['gold-band.svg', 'claude-code.svg', 'kubernetes.svg', 'fonts.css', 'inter-latin-variable.woff2', 'misans-vf.woff2', 'LICENSES.json']) {
  await copyFile(new URL(`assets/${asset}`, demos), new URL(`assets/${asset}`, root), constants.COPYFILE_EXCL);
}
for (const [source, output] of [['01-intent.html', '01-intent.html'], ['02-results.html', '05-marathon.html']]) {
  const content = (await readFile(new URL(`reference/${source}`, demos), 'utf8'))
    .replace(/<!--[\s\S]*?-->/, '<!-- Approved B editorial direction, 2026-10-14. Source: ../../direction-approved.md.\nAdapted from the user-selected reference showcase; benchmark facts remain unchanged.\nFixed 1920x1080 page; the shared deck host owns navigation and scaling. -->')
    .replace('</body>', '<script src="../assets/slide.js"></script>\n</body>');
  await writeFile(new URL(`slides/${output}`, root), content, { flag: 'wx' });
}
console.log('Packaged approved pages and local brand assets without changing the original report.');
