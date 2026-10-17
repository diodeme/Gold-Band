import { writeFile, readFile } from 'node:fs/promises';

const assets = new URL('./assets/', import.meta.url);
const repo = new URL('../../../../', new URL('./', import.meta.url));

async function saveNew(name, bytes) {
  const target = new URL(name, assets);
  try {
    await writeFile(target, bytes, { flag: 'wx' });
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    const existing = await readFile(target);
    if (!existing.equals(Buffer.from(bytes))) throw new Error(`Existing asset differs: ${name}`);
  }
}

for (const [name, source] of [
  ['gold-band.svg', 'web/public/logo.svg'],
  ['inter-latin-variable.woff2', 'themes/gold-band/assets/fonts/inter-latin-variable.woff2'],
  ['misans-vf.woff2', 'themes/gold-band/assets/fonts/misans-vf.woff2'],
  ['LICENSES.json', 'themes/gold-band/LICENSES.json'],
]) {
  await saveNew(name, await readFile(new URL(source, repo)));
}

for (const [name, url] of [
  ['claude-code.svg', 'https://cdn.simpleicons.org/claudecode'],
  ['kubernetes.svg', 'https://raw.githubusercontent.com/cncf/artwork/main/projects/kubernetes/icon/color/kubernetes-icon-color.svg'],
]) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const text = await response.text();
  if (!text.includes('<svg') || /<script|<foreignObject|https?:\/\//i.test(text.replace(/xmlns="[^"]*"/g, ''))) {
    throw new Error(`Unexpected external asset content: ${name}`);
  }
  await saveNew(name, text);
}
console.log('Prepared three real logos, two local fonts and license declarations.');
