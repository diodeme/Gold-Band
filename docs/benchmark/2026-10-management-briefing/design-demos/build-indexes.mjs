import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

const templatePath = process.argv[2];
if (!templatePath) throw new Error('Pass the installed huashu-design assets/deck_index.html path.');
const template = await readFile(pathToFileURL(templatePath), 'utf8');
const sourceManifest = /window\.DECK_MANIFEST = \[[\s\S]*?\];/;
if (!sourceManifest.test(template)) throw new Error('The upstream template manifest shape changed.');

const directions = [
  { id: 'roulette', label: 'A · 图解式', logic: '秒数轮盘 · Diagram-Driven Isotype' },
  { id: 'reference', label: 'B · 编辑式', logic: '现实标杆 · 管理层报告' },
  { id: 'studio', label: 'C · 舞台式', logic: '设计师思维 · 定制排印' },
];
const entries = direction => [
  { file: `${direction.id}/01-intent.html`, label: `${direction.label} · 测试意图`, thumb: `previews/${direction.id}-intent.png` },
  { file: `${direction.id}/02-results.html`, label: `${direction.label} · 重量级结果`, thumb: `previews/${direction.id}-results.png` },
];

async function build(filename, title, manifest, maxColumns = 9) {
  const output = template
    .replace('Math.min(9, Math.floor', `Math.min(${maxColumns}, Math.floor`)
    .replace('<meta charset="UTF-8">', '<meta charset="UTF-8">\n<meta name="viewport" content="width=device-width, initial-scale=1">')
    .replace('<title>Deck · Multi-file Slide Index</title>', `<title>${title} · Gold Band</title>`)
    .replace(sourceManifest, `window.DECK_MANIFEST = ${JSON.stringify(manifest, null, 4)};`)
    .replace('Overview · 点击任意页进入演示', `${title} · 点击任意页进入演示`)
    .replace('Infinite Gallery · 悬停暂停 · 点击任意页进入演示', `${title} · 悬停暂停 · 点击进入演示`);
  await writeFile(new URL(filename, import.meta.url), output, { flag: 'wx' });
}

for (const direction of directions) await build(`${direction.id}.html`, direction.label, entries(direction));
const all = directions.flatMap(entries);
// With a three-column overview, each direction's two representative pages align vertically.
await build('index.html', '三种方向 · 相同内容', [all[0], all[2], all[4], all[1], all[3], all[5]], directions.length);
console.log('Built three two-page decks and the six-page comparison from the supplied overview template.');
