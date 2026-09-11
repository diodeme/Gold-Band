import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { checkDataset } from '../marketing/demo/build-dataset.mjs';

const root = 'marketing/demo/data/ji-history';
const read = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'));
const published = () => read('publish-index.json').files;

test('the bundled demo dataset passes the production build gate', () => {
  assert.doesNotThrow(() => checkDataset(root));
});

test('the bundled demo dataset stays inside its publication budget', () => {
  const files = published();
  const bytes = files.reduce((sum, file) => sum + file.byteLength, 0);
  assert.ok(bytes <= 50 * 1024 * 1024, `published bytes ${bytes} exceed the 50 MiB demo budget`);
  assert.ok(files.length <= 4000, `published file count ${files.length} exceeds the demo budget`);
});

test('runtime artifacts and per-event payloads stay out of the publication inventory', () => {
  const files = published();
  assert.equal(files.filter((file) => file.path.startsWith('archive/')).length, 0, 'raw run archive must not ship');
  const dropped = files.filter((file) => /\/(?:files|directories|raw|events)\//.test(file.path));
  assert.deepEqual(dropped.slice(0, 5).map((file) => file.path), [], 'per-event payloads must not ship');
});

test('activity rows keep labels and elapsed time but no tool payload', () => {
  const rows = published()
    .filter((file) => /\/activity\/\d+\.json$/.test(file.path))
    .flatMap((file) => read(file.path));
  assert.ok(rows.length > 0, 'expected projected activity rows');
  assert.ok(rows.every((row) => typeof row.id === 'string' && typeof row.kind === 'string'));
  assert.ok(rows.some((row) => typeof row.title === 'string' && row.title.length > 0));
  assert.ok(rows.every((row) => row.raw?.rawOutput === undefined && row.raw?.content === undefined));
  assert.ok(rows.filter((row) => row.kind === 'toolCall').every((row) => row.content === undefined));
});

test('every published session index, page, image and change resource exists', () => {
  const dataset = read('dataset.json');
  assert.ok(dataset.sessions.length > 0);
  for (const session of dataset.sessions) {
    assert.ok(existsSync(join(root, session.path)), `missing session index ${session.path}`);
    const index = read(session.path);
    for (const page of index.pages) assert.ok(existsSync(join(root, page.path)), `missing page ${page.path}`);
    for (const page of index.activityPages) assert.ok(existsSync(join(root, page.path)), `missing activity page ${page.path}`);
    for (const ref of Object.values(index.eventRefs ?? {})) {
      assert.ok(Number.isSafeInteger(ref.activityPage), `event ${ref.id} has no activity page`);
      assert.ok(index.activityPages[ref.activityPage], `event ${ref.id} points outside its activity pages`);
    }
  }
});

test('sessions advertise an empty workspace listing instead of dropped file trees', () => {
  const sessions = Object.entries(read('resources.json').sessions);
  assert.ok(sessions.length > 0);
  for (const [key, ref] of sessions) {
    assert.ok(existsSync(join(root, ref.path)), `missing resources for ${key}`);
    assert.deepEqual(read(read(ref.path).directory.path), []);
  }
});

test('message pages carry the tool card titles the demo shows without event payloads', () => {
  const dataset = read('dataset.json');
  const pages = dataset.sessions.flatMap((session) => read(session.path).pages.map((page) => read(page.path)));
  const toolCalls = pages.flat().filter((event) => event.kind === 'toolCall');
  assert.ok(toolCalls.length > 0, 'expected tool call cards in the published pages');
  assert.ok(toolCalls.every((event) => typeof event.title === 'string' && event.title.length > 0));
  assert.ok(toolCalls.every((event) => event.content === null));
  assert.ok(
    toolCalls.every((event) => event.raw?._meta?.goldBandConversation?.toolDetailAvailable === false),
    'tool cards must not advertise a detail payload that is not published',
  );
});
