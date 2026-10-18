import { test } from 'node:test';
import assert from 'node:assert/strict';
import { portableRecording, recordingResourceUrls } from './site-recording-assets.mjs';

test('omits executable preload dependencies from snapshots and later attribute mutations', () => {
  const input = { bytes: 0, events: [
    { type: 2, data: { node: { id: 1, type: 0, childNodes: [
      { id: 2, type: 2, tagName: 'link', attributes: { rel: 'modulepreload', href: '/client.js' }, childNodes: [] },
      { id: 3, type: 2, tagName: 'script', attributes: { src: '/runtime.js' }, childNodes: [] },
      { id: 4, type: 2, tagName: 'img', attributes: { src: '/logo.svg', alt: '/client.js' }, childNodes: [] },
    ] } } },
    { type: 3, data: { source: 0, attributes: [{ id: 2, attributes: { href: '/later.js' } }], adds: [], removes: [], texts: [] } },
  ] };
  const output = portableRecording(input, 'http://localhost:1443');
  assert.deepEqual(recordingResourceUrls(output), ['/logo.svg']);
  assert.equal(output.events[0].data.node.childNodes[2].attributes.alt, '/client.js');
});

test('relocates snapshot and inline stylesheet assets without changing external links or source data', () => {
  const input = { version: 1, bytes: 0, events: [{ type: 2, data: { node: { type: 2, id: 1, tagName: 'link', attributes: { href: 'http://127.0.0.1:1440/logo.svg', _cssText: '@font-face{src:url("http://127.0.0.1:1440/theme-assets/font.woff2")}' }, childNodes: [] }, external: 'https://github.com/diodeme/Gold-Band' } }] };
  const result = portableRecording(input, 'http://127.0.0.1:1440/preview.html');
  assert.equal(result.events[0].data.node.attributes.href, '/logo.svg');
  assert.equal(result.events[0].data.node.attributes._cssText, '@font-face{src:url(/theme-assets/font.woff2)}');
  assert.equal(result.events[0].data.external, input.events[0].data.external);
  assert.equal(input.events[0].data.node.attributes.href, 'http://127.0.0.1:1440/logo.svg');
  assert.equal(result.bytes, new TextEncoder().encode(JSON.stringify(result)).length);
});

test('relocates incremental styles, adopted sheets, fonts and responsive images using resource syntax', () => {
  const origin = 'http://127.0.0.1:1442';
  const input = { bytes: 0, events: [
    { type: 2, data: { node: { id: 1, type: 2, tagName: 'img', attributes: {
      srcset: `${origin}/small.png 1x, ${origin}/large.png 2x`,
      style: `background-image:image-set("${origin}/one.png" 1x,url("${origin}/two.png") 2x)`,
    }, childNodes: [] } } },
    { type: 3, data: { source: 0, adds: [{ parentId: 1, node: { id: 2, type: 2, tagName: 'style', attributes: {}, childNodes: [{ id: 3, type: 3, textContent: '' }] } }], texts: [{ id: 3, value: `@import "${origin}/nested.css";` }], attributes: [{ id: 1, attributes: { style: { background: [`url(${origin}/bg.png)`, 'important'], color: false } } }] } },
    { type: 3, data: { source: 8, adds: [{ rule: `.x{background:url(${origin}/add.png)}` }], replaceSync: `.x{background:url(${origin}/replace.png)}` } },
    { type: 3, data: { source: 13, set: { property: 'background', value: `url(${origin}/set.png)` } } },
    { type: 3, data: { source: 13, set: { property: 'background', value: null } } },
    { type: 3, data: { source: 15, styles: [{ styleId: 10, rules: [{ rule: `.x{background:url(${origin}/adopted.png)}` }] }] } },
    { type: 3, data: { source: 10, buffer: false, fontSource: `url(${origin}/font.woff2)` } },
  ] };
  const result = portableRecording(input, origin);
  assert.deepEqual(recordingResourceUrls(result).sort(), ['/small.png', '/large.png', '/one.png', '/two.png', '/nested.css', '/bg.png', '/add.png', '/replace.png', '/set.png', '/adopted.png', '/font.woff2'].sort());
  assert.equal(result.events[0].data.node.attributes.srcset, '/small.png 1x, /large.png 2x');
  assert.equal(result.bytes, Buffer.byteLength(JSON.stringify(result)));
});

test('rejects external resource dependencies but preserves navigation and custom payloads', () => {
  const node = { id: 1, type: 2, tagName: 'a', attributes: { href: 'https://example.com/docs' }, childNodes: [] };
  const input = { bytes: 0, events: [{ type: 2, data: { node } }] };
  assert.equal(portableRecording(input, 'http://127.0.0.1:1442').events[0].data.node.attributes.href, node.attributes.href);
  node.tagName = 'link';
  assert.throws(() => portableRecording(input, 'http://127.0.0.1:1442'), error => error.code === 'site.resource-external');
});

test('preserves recorded prose and CSS content strings while relocating actual snapshot resources', () => {
  const origin = 'http://127.0.0.1:1442';
  const prose = `Open ${origin}/docs and keep this URL unchanged.`;
  const input = { version: 1, bytes: 0, events: [
    { type: 2, data: { node: { type: 0, childNodes: [
      { id: 1, type: 2, tagName: 'img', attributes: { src: `${origin}/logo.svg`, alt: prose }, childNodes: [] },
      { id: 2, type: 3, textContent: prose },
      { id: 3, type: 2, tagName: 'style', attributes: {}, childNodes: [
        { id: 4, type: 3, isStyle: true, textContent: `.x{background:url("${origin}/logo.svg");content:"${origin}/docs"}` },
      ] },
    ] } } },
    { type: 3, data: { source: 0, texts: [{ id: 2, value: prose }], attributes: [{ id: 1, attributes: { src: `${origin}/next.svg` } }], adds: [], removes: [] } },
    { type: 5, data: { tag: 'checkpoint', payload: { text: prose } } },
  ] };
  const result = portableRecording(input, origin);
  const nodes = result.events[0].data.node.childNodes;
  assert.equal(nodes[0].attributes.src, '/logo.svg');
  assert.equal(nodes[0].attributes.alt, prose);
  assert.equal(nodes[1].textContent, prose);
  assert.ok(nodes[2].childNodes[0].textContent.includes('url(/logo.svg)'));
  assert.ok(nodes[2].childNodes[0].textContent.includes(`content:"${origin}/docs"`));
  assert.equal(result.events[1].data.texts[0].value, prose);
  assert.equal(result.events[1].data.attributes[0].attributes.src, '/next.svg');
  assert.equal(result.events[2].data.payload.text, prose);
  assert.equal(input.events[0].data.node.childNodes[0].attributes.src, `${origin}/logo.svg`);
});
