import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { PDFDocument } from 'pdf-lib';

const root = new URL('../', import.meta.url);
const load = file => readFileSync(new URL(file, root), 'utf8');
const index = load('index.html');
const manifest = JSON.parse(index.match(/window\.DECK_MANIFEST = (\[[\s\S]*?\]);/)[1]);
const slideCode = load('assets/slide.js');
const text = file => load(file).replace(/<style>[\s\S]*?<\/style>|<!--[\s\S]*?-->/g, '').replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');

function mount({ standalone = false, expanded = false } = {}) {
  const messages = [];
  let listener;
  let focused = false;
  const detail = { open: expanded, querySelector: () => ({ focus() { focused = true; } }) };
  const document = {
    addEventListener(type, callback) { assert.equal(type, 'keydown'); listener = callback; },
    querySelector() { return detail.open ? detail : null; },
  };
  const window = { parent: { postMessage(message, origin) { messages.push({ ...message, origin }); } } };
  if (standalone) window.parent = window;
  runInNewContext(slideCode, { document, window, Set });
  return {
    messages, detail, focused: () => focused,
    key(key, tag = 'body', extra = {}) {
      const event = {
        key, target: { closest(selector) { return selector.split(',').some(part => part.trim() === tag) ? this : null; } },
        preventDefault() { this.defaultPrevented = true; }, ...extra,
      };
      listener(event);
      return event;
    },
  };
}

test('nine approved pages are in the intended narrative order', () => {
  assert.deepEqual(manifest.map(page => page.file), [
    'slides/01-intent.html', 'slides/02-benchmarks.html', 'slides/03-deepswe.html',
    'slides/04-mailauth.html', 'slides/05-marathon.html', 'slides/06-ruff.html',
    'slides/07-context.html', 'slides/08-auto.html', 'slides/09-recommendations.html',
  ]);
  assert.equal(readdirSync(new URL('slides/', root)).filter(file => file.endsWith('.html')).length, 9);
});

test('all slide resources resolve offline, including local logos and navigation bridge', () => {
  for (const page of manifest) {
    const html = load(page.file);
    assert.match(html, /\.\.\/assets\/slide\.js/);
    for (const [, source] of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
      if (/^(?:data:|#|https?:)/.test(source)) continue;
      assert.ok(existsSync(new URL(source, new URL(page.file, root))), `${page.file}: ${source}`);
    }
    assert.doesNotMatch(html, /<(?:script|img|link)[^>]+(?:src|href)="https?:/);
  }
});

test('benchmark disclosure contains only the five selected comparisons', () => {
  const html = load('slides/02-benchmarks.html');
  const groups = [...html.matchAll(/<details[^>]+name="([^"]+)"/g)].map(match => match[1]);
  assert.equal(groups.length, 5);
  assert.equal(new Set(groups).size, 1);
  for (const name of ['DeepSWE', 'zstd-decoder', 'mailauth', 'Kubernetes', 'Ruff']) assert.ok(html.includes(name));
  assert.doesNotMatch(html, /stripe-clone|AUTO 单臂|12 项验收关卡|52\.1M/);
  assert.match(html, /@media print/);
});

test('the context slide retains its conclusion without a disclaimer footer', () => {
  const html = load('slides/07-context.html');
  assert.match(html, /继续执行的条件，与长期保留约束的能力，需要分别解决。/);
  assert.doesNotMatch(html, /<footer\b|非已证实因果|来源：汇报 README/);
});

test('light and medium comparisons preserve the complete observed data', () => {
  const deep = text('slides/03-deepswe.html');
  for (const pattern of [/15\s*\/\s*29/, /12\s*\/\s*29/, /10\.7M/, /4\.5M/, /43\s*\/\s*43/, /126M/, /64M/]) assert.match(deep, pattern);
  assert.match(deep, /输出 token/);
  const mail = text('slides/04-mailauth.html');
  for (const value of ['684 / 702', '694 / 702', '702 / 702', '701 / 702', '336.6M', '407.9M', '397.6M', '699.5M']) assert.ok(mail.includes(value), value);
});

test('long-task results keep the token/time distinction and unfinished-run status', () => {
  const marathon = text('slides/05-marathon.html');
  for (const value of ['1,405 / 1,407', '1,407 / 1,407', '325.8M', '173M', '97', '165', '47%']) assert.ok(marathon.includes(value), value);
  assert.match(marathon, /Agent 耗时/);
  assert.match(marathon, /耗时仍更长/);
  assert.match(marathon, /复测/);
  const ruff = text('slides/06-ruff.html');
  assert.match(ruff, /8\.2/);
  assert.match(ruff, /1\.64/);
  assert.match(ruff, /仍在|进行中|running/);
  assert.match(text('slides/07-context.html'), /架构判断/);
});

test('the execution-framework comparison explicitly allows Claude Code subagents', () => {
  const intent = load('slides/01-intent.html');
  assert.match(intent, /Claude Code<\/p><p class="identity-mode">[^<]*（允许分发 subagent）/);
});

test('the benchmark overview leads with a mode-selection conclusion', () => {
  const overview = load('slides/02-benchmarks.html');
  const heading = overview.match(/<h1[^>]*>([\s\S]*?)<\/h1>/)[1];
  assert.match(heading, /中短任务.*Direct.*长任务.*编排/);
  assert.match(overview, /质量略好或持平.*token.*耗时更高/);
  assert.ok(overview.indexOf('<h1') < overview.indexOf('class="task-list"'));
});

test('the mailauth heading identifies the task and the higher-overhead arm', () => {
  const mail = load('slides/04-mailauth.html');
  const heading = mail.match(/<h1[^>]*>([\s\S]*?)<\/h1>/)[1];
  assert.match(heading, /mailauth.*质量接近.*AUTO.*开销更高/);
  assert.match(mail, /与 Claude Code 相比.*AUTO.*token 与 Eval 耗时/);
});

test('Ruff contrasts early exit with AUTO orchestration rather than requiring both arms to add gates', () => {
  const ruff = load('slides/06-ruff.html');
  const heading = ruff.match(/<h1[^>]*>([\s\S]*?)<\/h1>/)[1];
  assert.match(heading, /Claude Code 未完成就主动退出/);
  assert.match(heading, /AUTO 无需额外闸门，持续推进/);
  assert.doesNotMatch(ruff, /加入 goal \/ 完成约束|再观察能否持续推进/);
  assert.match(ruff, /AUTO 首段异常暂停后修复并续跑/);
  assert.match(ruff, /最终验收尚未完成/);
});

test('the benchmark overview explicitly labels its headline as a conclusion', () => {
  const heading = load('slides/02-benchmarks.html').match(/<h1[^>]*>([\s\S]*?)<\/h1>/)[1];
  assert.match(heading, /^结论：/);
});

test('every benchmark row provides a scoped code-size label', () => {
  const overview = load('slides/02-benchmarks.html');
  const sizes = [...overview.matchAll(/class="task-size">([\s\S]*?)<\/span>/g)]
    .map(match => match[1].replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim());
  assert.equal(sizes.length, 5);
  for (const size of sizes) {
    assert.match(size, /行/);
    assert.match(size, /改动|交付|参考/);
    assert.doesNotMatch(size, /xxx|待填|TBD/);
  }
});

test('overview and recommendations provide an explicitly provisional size and context heuristic', () => {
  for (const file of ['slides/02-benchmarks.html', 'slides/09-recommendations.html']) {
    const html = load(file);
    const footer = html.match(/<footer[^>]*>([\s\S]*?)<\/footer>/)[1].replace(/<[^>]*>/g, '');
    assert.match(footer, /参考建议/);
    assert.match(footer, /新增.*修改.*[\d,]+.*行/);
    assert.match(footer, /上下文.*压缩.*3 次及以上/);
    assert.match(footer, /工作流.*AUTO/);
    assert.doesNotMatch(footer, /统一难度分数|阶段汇报 README/);
  }
  assert.match(load('SOURCES.md'), /启发式.*不是.*实测.*阈值/);
});

test('slide body forwards navigation to its host', () => {
  const app = mount();
  for (const key of ['ArrowRight', 'ArrowLeft', 'PageDown', 'Home', 'End', 'f']) assert.equal(app.key(key).defaultPrevented, true);
  assert.equal(app.messages.length, 6);
  assert.ok(app.messages.every(message => message.type === 'gold-band-deck:navigate'));
});

test('native summary space and editable inputs keep keyboard ownership', () => {
  const app = mount();
  assert.equal(app.key(' ', 'summary').defaultPrevented, undefined);
  assert.equal(app.key('Enter', 'summary').defaultPrevented, undefined);
  assert.equal(app.key('ArrowRight', 'input').defaultPrevented, undefined);
  assert.equal(app.key('ArrowRight', 'body', { ctrlKey: true }).defaultPrevented, undefined);
  assert.equal(app.messages.length, 0);
});

test('Escape closes a disclosure before returning to the deck overview', () => {
  const app = mount({ expanded: true });
  assert.equal(app.key('Escape').defaultPrevented, true);
  assert.equal(app.detail.open, false);
  assert.equal(app.focused(), true);
  assert.equal(app.messages.length, 0);
  app.key('Escape');
  assert.equal(app.messages.at(-1).key, 'Escape');
});

test('standalone slides do not post navigation to themselves', () => {
  const app = mount({ standalone: true });
  assert.equal(app.key('ArrowRight').defaultPrevented, undefined);
  assert.equal(app.messages.length, 0);
});

test('the host accepts messages only from its active frame and does not steal focus', () => {
  assert.match(index, /event\.source !== frame\.contentWindow/);
  assert.match(index, /document\.body\.dataset\.mode !== 'present'/);
  assert.doesNotMatch(index, /setTimeout\(\(\) => window\.focus/);
  assert.match(index, /width: 44px; cursor: pointer/);
  assert.match(index, /id="fullButton"/);
});

test('the exported PDF contains exactly nine landscape pages', async () => {
  const pdf = await PDFDocument.load(readFileSync(new URL('Gold-Band-Benchmark.pdf', root)));
  assert.equal(pdf.getPageCount(), 9);
  for (const page of pdf.getPages()) {
    assert.equal(page.getWidth(), 1440);
    assert.equal(page.getHeight(), 810);
  }
  const verification = JSON.parse(load('Gold-Band-Benchmark.verification.json'));
  assert.equal(verification.checks.length, 9);
  for (const page of verification.checks) {
    assert.deepEqual(page.outside, []);
    assert.deepEqual(page.errors, []);
  }
});
