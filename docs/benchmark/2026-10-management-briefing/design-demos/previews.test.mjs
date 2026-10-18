import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

const root = new URL('./', import.meta.url);
const directions = ['roulette', 'reference', 'studio'];
const load = path => readFileSync(new URL(path, root), 'utf8');
const textOf = html => html.replace(/<style>[\s\S]*?<\/style>/g, '').replace(/<script[^>]*>[\s\S]*?<\/script>/g, '').replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ');

for (const direction of directions) {
  test(`${direction}: two full-sized slides with real local assets`, () => {
    for (const page of ['01-intent', '02-results']) {
      const path = `${direction}/${page}.html`;
      const html = load(path);
      assert.match(html, /1920px/);
      assert.match(html, /1080px/);
      assert.match(html, /fonts\.css/);
      assert.match(html, /gold-band\.svg|data:image\/svg\+xml/);
      assert.doesNotMatch(html, /<(?:script|link|img)[^>]+(?:src|href)=["']https?:/);
      for (const [, href] of html.matchAll(/(?:src|href)=["']([^"']+)["']/g)) {
        if (/^(?:data:|https?:|#)/.test(href)) continue;
        assert.ok(existsSync(new URL(href, new URL(path, root))), `${path}: missing ${href}`);
      }
    }
  });
  test(`${direction}: comparison preserves all numbers and the time tradeoff`, () => {
    const text = textOf(load(`${direction}/02-results.html`));
    for (const value of ['1,407', '1,405', '173', '325.8', '165', '97', '47']) assert.ok(text.includes(value), `${direction}: missing ${value}`);
    assert.match(text, /Claude\s*Code/);
    assert.match(text, /AUTO/);
    assert.match(text, /(?:耗时|时间).{0,20}(?:更长|较长|增加|更久)|(?:仍然|依然|仍).{0,10}(?:更慢|更长)/);
    assert.match(text, /复测/);
  });
  test(`${direction}: overview uses the shared template and resolves its slides and thumbnails`, () => {
    const html = load(`${direction}.html`);
    const manifest = JSON.parse(html.match(/window\.DECK_MANIFEST = (\[[\s\S]*?\]);/)[1]);
    assert.equal(manifest.length, 2);
    for (const item of manifest) {
      assert.ok(existsSync(new URL(item.file, root)), item.file);
      assert.ok(existsSync(new URL(item.thumb, root)), item.thumb);
    }
    assert.match(html, /function layoutWall\(/);
    assert.match(html, /function fit\(/);
    assert.match(html, /localStorage\.setItem/);
  });
}

test('comparison overview contains exactly the six slides, aligned by direction', () => {
  const html = load('index.html');
  const manifest = JSON.parse(html.match(/window\.DECK_MANIFEST = (\[[\s\S]*?\]);/)[1]);
  assert.equal(manifest.length, 6);
  assert.deepEqual(manifest.slice(0, 3).map(item => item.file.split('/')[0]), directions);
  assert.equal(new Set(manifest.map(item => item.file)).size, 6);
});

test('the comparison layout keeps the three directions paired in columns on desktop', () => {
  const html = load('index.html');
  const expression = html.match(/let cols = ([^;]+);/)[1];
  for (const width of [1280, 1440, 1920]) {
    const columns = runInNewContext(expression, { Math, availW: width * 0.86, gap: 26, target: 280 });
    assert.equal(columns, directions.length, `At ${width}px, each direction needs its own column.`);
  }
});

test('the three real logos and both WOFF2 fonts are packaged offline', () => {
  for (const name of ['gold-band.svg', 'claude-code.svg', 'kubernetes.svg']) {
    const svg = load(`assets/${name}`);
    assert.match(svg, /<svg\b/);
    assert.doesNotMatch(svg, /<script\b|<foreignObject\b/);
  }
  for (const name of ['inter-latin-variable.woff2', 'misans-vf.woff2']) {
    const bytes = readFileSync(new URL(`assets/${name}`, root));
    assert.equal(bytes.subarray(0, 4).toString('ascii'), 'wOF2');
  }
  const licenses = JSON.parse(load('assets/LICENSES.json'));
  assert.equal(licenses.licenses.length, 2);
});
