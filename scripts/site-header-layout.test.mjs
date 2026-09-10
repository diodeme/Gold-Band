import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('route-specific width belongs to content and cannot resize the shared header', () => {
  const css = readFileSync('marketing/site/style.css', 'utf8');
  assert.doesNotMatch(css, /\.site\.site-with-demo\s*\{|\.site-with-demo\s+\.site-header\s*\{/);
  assert.match(css, /\.site-demo-page\s*\{[^}]*height: 100dvh/);
  assert.doesNotMatch(css, /\.site-header\s*\{[^}]*(position:\s*(sticky|fixed))/);
});
