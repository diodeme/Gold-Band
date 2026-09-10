import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'vite';

test('Demo dev serves the configured dataset and rejects missing data as 404', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'demo-data-'));
  writeFileSync(join(directory, 'dataset.json'), JSON.stringify({ version: 1, taskId: 'task-original' }));
  process.env.DEMO_DATASET_PATH = directory;
  const server = await createServer({ configFile: 'marketing/demo/vite.config.ts', server: { host: '127.0.0.1', port: 0 } });
  try {
    await server.listen();
    const origin = 'http://127.0.0.1:' + server.httpServer.address().port;
    const response = await fetch(origin + '/data/ji-history/dataset.json');
    assert.match(response.headers.get('content-type'), /json/);
    assert.equal((await response.json()).taskId, 'task-original');
    assert.equal((await fetch(origin + '/data/ji-history/missing.json')).status, 404);
  } finally { await server.close(); delete process.env.DEMO_DATASET_PATH; rmSync(directory, { recursive: true, force: true }); }
});
