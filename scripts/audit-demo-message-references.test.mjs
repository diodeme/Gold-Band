import assert from 'node:assert/strict';
import test from 'node:test';
import { messageReferences, auditMessageReferences } from './audit-demo-message-references.mjs';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

test('resolves Markdown reference definitions and retains inline candidates without treating them as links', () => {
  const entries = messageReferences('[report][Doc]\n\n[doc]: ./report.md\n\n`src/main.ts` and `npm test`');
  assert.deepEqual(entries.map(({ kind, value }) => ({ kind, value })), [
    { kind: 'linkReference', value: './report.md' },
    { kind: 'inlineCode', value: 'src/main.ts' }, { kind: 'inlineCode', value: 'npm test' },
  ]);
  assert.equal(entries[0].position.line, 1);
});

test('uses HTML parsing for quoted attributes, entities and inert template contents', () => {
  const entries = messageReferences('<a href="./x?a=1&amp;b=2">x</a><img src="./a b.png"><template><video poster="p.png"></video></template>');
  assert.deepEqual(entries.map(({ attribute, value }) => [attribute, value]), [['href', './x?a=1&b=2'], ['src', './a b.png'], ['poster', 'p.png']]);
});

test('retains compound srcset for explicit review and excludes fenced-code lookalikes', () => {
  const entries = messageReferences('<img srcset="a.png 1x, b.png 2x">\n\n```html\n<img src="not-a-reference.png">\n```');
  assert.equal(entries.length, 1);
  assert.equal(entries[0].attribute, 'srcset');
  assert.equal(entries[0].value, 'a.png 1x, b.png 2x');
});

test('matches CommonMark first-definition precedence for duplicate identifiers', () => {
  assert.equal(messageReferences('[file][target]\n\n[target]: first.md\n[target]: second.md')[0].value, 'first.md');
});

test('audits all declared pages with complete scope and rejects changed resource bytes', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-reference-audit-'));
  try {
    await mkdir(join(directory, 'assets'));
    const assets = [];
    const asset = async value => {
      const bytes = JSON.stringify(value), sha256 = createHash('sha256').update(bytes).digest('hex');
      const path = `assets/${sha256}.json`;
      await writeFile(join(directory, path), bytes);
      assets.push({ path, sha256, bytes: Buffer.byteLength(bytes) });
      return path;
    };
    const first = await asset([{ id: 'first', seq: 1, content: '`first.ts`' }]);
    const second = await asset([{ id: 'second', seq: 2, content: '[last](last.md)' }]);
    const detail = await asset({ branches: [{ id: 'child', pages: [{ path: first }, { path: second }] }] });
    const locator = { projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', outerNodeId: 'outer', outerAttemptId: 'outer-attempt' };
    await writeFile(join(directory, 'manifest.json'), JSON.stringify({ assets, resources: [], sessions: [{ ...locator, detail }] }));
    const result = await auditMessageReferences(directory);
    assert.deepEqual(result.counts, { sessions: 1, branches: 1, pages: 2, events: 2, bodies: 2 });
    assert.deepEqual(result.records.map(record => record.eventId), ['first', 'second']);
    assert.deepEqual(result.records[1].locator, locator);
    assert.equal(result.records[1].branchId, 'child');
    await writeFile(join(directory, second), '[]');
    await assert.rejects(auditMessageReferences(directory), /audit.resource-integrity/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
