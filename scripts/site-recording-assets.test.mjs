import { test } from 'node:test';
import assert from 'node:assert/strict';
import { portableRecording } from './site-recording-assets.mjs';

test('relocates snapshot and inline stylesheet assets without changing external links or source data', () => {
  const input = { version: 1, bytes: 0, events: [{ type: 2, data: { attributes: { href: 'http://127.0.0.1:1440/logo.svg' }, css: '@font-face{src:url("http://127.0.0.1:1440/theme-assets/font.woff2")}', external: 'https://github.com/diodeme/Gold-Band' } }] };
  const result = portableRecording(input, 'http://127.0.0.1:1440/preview.html');
  assert.equal(result.events[0].data.attributes.href, '/logo.svg');
  assert.equal(result.events[0].data.css, '@font-face{src:url("/theme-assets/font.woff2")}');
  assert.equal(result.events[0].data.external, input.events[0].data.external);
  assert.equal(input.events[0].data.attributes.href, 'http://127.0.0.1:1440/logo.svg');
  assert.equal(result.bytes, new TextEncoder().encode(JSON.stringify(result.events)).length);
});
