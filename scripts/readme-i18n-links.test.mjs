import assert from 'node:assert/strict';
import { access, readFile } from 'node:fs/promises';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = fileURLToPath(new URL('..', import.meta.url));

const readRepoFile = (relativePath) =>
  readFile(new URL(relativePath, new URL('../', import.meta.url)), 'utf8');

const count = (value, token) => value.split(token).length - 1;

const fencedBlocks = (markdown) =>
  [...markdown.matchAll(/```[^\n]*\n([\s\S]*?)```/g)].map((match) => match[1]);

test('localized READMEs route macOS users to the matching guide language', async () => {
  const [english, chinese] = await Promise.all([
    readRepoFile('README.md'),
    readRepoFile('README.zh-CN.md'),
  ]);

  assert.match(
    english,
    /\[macOS Installation and Troubleshooting Guide\]\(docs\/guide\/macos-install\.md\)/,
  );
  assert.doesNotMatch(english, /macos-install\.zh-CN\.md/);
  assert.match(
    chinese,
    /\[macOS 安装与排错指南\]\(docs\/guide\/macos-install\.zh-CN\.md\)/,
  );
  assert.doesNotMatch(chinese, /docs\/guide\/macos-install\.md/);

  for (const readme of [english, chinese]) {
    assert.equal(count(readme, '<!-- README-I18N:START -->'), 1);
    assert.equal(count(readme, '<!-- README-I18N:END -->'), 1);
  }
});

test('localized macOS guides link to each other and preserve command blocks', async () => {
  const [english, chinese] = await Promise.all([
    readRepoFile('docs/guide/macos-install.md'),
    readRepoFile('docs/guide/macos-install.zh-CN.md'),
  ]);

  assert.match(english, /\*\*English\*\* \| \[中文\]\(\.\/macos-install\.zh-CN\.md\)/);
  assert.match(chinese, /\[English\]\(\.\/macos-install\.md\) \| \*\*中文\*\*/);

  for (const guide of [english, chinese]) {
    assert.equal(count(guide, '<!-- DOCS-I18N:START -->'), 1);
    assert.equal(count(guide, '<!-- DOCS-I18N:END -->'), 1);
  }

  assert.deepEqual(fencedBlocks(english), fencedBlocks(chinese));
  await assert.rejects(access(`${repoRoot}/docs/macos-install.md`));
});

test('every localized macOS guide target exists', async () => {
  await Promise.all([
    access(`${repoRoot}/docs/guide/macos-install.md`),
    access(`${repoRoot}/docs/guide/macos-install.zh-CN.md`),
  ]);
});
