import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

if (!process.argv[2]) throw new Error('Pass the installed huashu-design assets/deck_index.html path.');
const template = await readFile(pathToFileURL(process.argv[2]), 'utf8');
const pages = [
  ['01-intent', '测试意图'], ['02-benchmarks', '测试全景'], ['03-deepswe', '轻量任务'],
  ['04-mailauth', '中量任务'], ['05-marathon', '重量级结果'], ['06-ruff', '超长任务'],
  ['07-context', '上下文与漂移'], ['08-auto', 'AUTO 架构'], ['09-recommendations', '使用建议'],
];
const manifest = pages.map(([id, label]) => ({ file: `slides/${id}.html`, label, thumb: `previews/${id}.png` }));
const focusSteal = /  \/\/ Reclaim focus[\s\S]*?  \}\);/;
if (!focusSteal.test(template)) throw new Error('Review the upstream focus handling before adapting this template.');
const bridge = `  // Forward navigation only from the active slide; never steal focus from its native controls.
  window.addEventListener('message', event => {
    if (event.source !== frame.contentWindow || document.body.dataset.mode !== 'present') return;
    if (event.data?.type !== 'gold-band-deck:navigate') return;
    const key = event.data.key;
    if (!['ArrowLeft','ArrowRight','PageUp','PageDown','Home','End',' ','Escape','f','F','p','P'].includes(key)) return;
    document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
  const fullButton = document.getElementById('fullButton');
  async function fullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
    } catch { fullButton.textContent = '请用浏览器菜单全屏'; }
  }
  fullButton.addEventListener('click', fullscreen);
  document.addEventListener('fullscreenchange', () => {
    fullButton.textContent = document.fullscreenElement ? '退出全屏' : '全屏';
  });
  document.addEventListener('keydown', event => {
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target.closest?.('input, textarea, select, [contenteditable]')) return;
    if (event.key === 'f' || event.key === 'F') { event.preventDefault(); fullscreen(); }
  });`;

const output = template
  .replace('<title>Deck · Multi-file Slide Index</title>', '<title>Gold Band · Benchmark 阶段性汇报 · 编辑版</title>')
  .replace('<meta charset="UTF-8">', '<meta charset="UTF-8">\n<meta name="viewport" content="width=device-width, initial-scale=1">')
  .replace(/window\.DECK_MANIFEST = \[[\s\S]*?\];/, `window.DECK_MANIFEST = ${JSON.stringify(manifest, null, 4)};`)
  .replace("// window.DECK_OVERVIEW = 'grid';", "window.DECK_OVERVIEW = 'grid';")
  .replace('Math.max(3, Math.min(9, Math.floor', 'Math.max(1, Math.min(3, Math.floor')
  .replace('width: 15%; cursor: pointer;', 'width: 44px; cursor: pointer;')
  .replace("if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;", "if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;\n    if (e.target.closest?.('input, textarea, select, [contenteditable]')) return;\n    if (e.key === ' ' && e.target.closest?.('button, a, summary')) return;")
  .replace('current = parseInt(m[1], 10) - 1;', 'current = Math.max(0, Math.min(parseInt(m[1], 10) - 1, deck.length - 1));')
  .replace('current = Math.min(parseInt(hashMatch[1], 10) - 1, deck.length - 1);', 'current = Math.max(0, Math.min(parseInt(hashMatch[1], 10) - 1, deck.length - 1));')
  .replace('Overview · 点击任意页进入演示', 'Gold Band · Benchmark 阶段性汇报 · 点击任意页进入演示')
  .replace('<button class="overview-btn" id="overviewBtn">⊞ 概览</button>', '<button class="overview-btn" id="overviewBtn">⊞ 概览</button>\n  <button class="overview-btn" id="fullButton" style="left:auto;right:20px">全屏</button>')
  .replace(focusSteal, bridge);
await writeFile(new URL('../index.html', import.meta.url), output, { flag: 'wx' });
console.log('Built the nine-page editorial deck with bounded overview and focus-safe iframe navigation.');
