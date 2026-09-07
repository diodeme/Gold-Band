import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { injectCapture, Journal, requestedSDK, rpcSummary, sdkSummary } from './core.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

test('new, load, resume and fork preserve business metadata and options', () => {
  for (const method of ['session/new', 'session/load', 'session/resume', 'session/fork']) {
    const frame = { id: 7, method, params: { cwd: 'work', sessionId: 's', _meta: {
      systemPrompt: { append: 'runtime' }, claudeCode: { options: { model: 'chosen', env: { KEEP: 'value' } } },
    } } };
    const output = injectCapture(frame, 'unique.debug.log');
    assert.equal(frame.params._meta.claudeCode.options.debug, undefined);
    assert.deepEqual(output.params._meta.systemPrompt, frame.params._meta.systemPrompt);
    assert.equal(output.params._meta.claudeCode.options.model, 'chosen');
    assert.deepEqual(output.params._meta.claudeCode.options.env, { KEEP: 'value' });
    assert.equal(output.params._meta.claudeCode.options.debugFile, 'unique.debug.log');
    assert(requestedSDK(output.params._meta.claudeCode.emitRawSDKMessages, { type: 'command_lifecycle' }));
  }
  const cancel = { method: 'session/cancel', params: { sessionId: 's' } };
  assert.equal(injectCapture(cancel, 'debug'), cancel);
});

test('SDK records distinguish null result and stream termination without body leakage', () => {
  const result = sdkSummary({ type: 'result', subtype: 'success', stop_reason: null,
    result: 'sensitive-result', usage: { input_tokens: 0 } });
  assert.equal(result.stop_reason, null);
  assert.equal(result.resultBytes, 16);
  assert(!JSON.stringify(result).includes('sensitive-result'));
  const summary = sdkSummary({ type: 'stream_event', event: { type: 'message_delta',
    delta: { stop_reason: 'end_turn', text: 'secret' } } });
  assert.equal(summary.event.delta.stop_reason, 'end_turn');
  assert(!JSON.stringify(summary).includes('secret'));
  assert(!JSON.stringify(rpcSummary({ method: 'session/prompt', params: { prompt: [{ text: 'secret' }] } })).includes('secret'));
  assert.equal(requestedSDK(false, { type: 'result' }), false);
  assert.equal(requestedSDK([{ type: 'system', subtype: 'init' }], { type: 'system', subtype: 'task_notification' }), false);
  const tasks = sdkSummary({ type: 'system', subtype: 'background_tasks_changed',
    tasks: [{ task_id: 'download', task_type: 'local_bash', description: 'secret' }] });
  assert.deepEqual(tasks.tasks, [{ task_id: 'download', task_type: 'local_bash' }]);
});

test('segments retain early records and capacity exhaustion marks incomplete', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'claude-capture-journal-'));
  try {
    const warnings = [];
    const journal = new Journal(dir, { segmentBytes: 170, maxBytes: 1000, warn: x => warnings.push(x) });
    for (let i = 0; i < 20; i++) await journal.record({ kind: 'test', index: i });
    await journal.close();
    const files = (await readdir(dir)).filter(x => x.endsWith('.jsonl')).sort();
    assert(files.length > 1);
    const events = (await Promise.all(files.map(f => readFile(path.join(dir, f), 'utf8'))))
      .join('').trim().split('\n').map(JSON.parse);
    assert.equal(events[0].index, 0);
    assert.deepEqual(events.map(x => x.seq), events.map((_, i) => i + 1));
    assert.equal(warnings.length, 1);
    const marker = JSON.parse(await readFile(path.join(dir, 'events.incomplete.json'), 'utf8'));
    assert.equal(marker.code, 'CAPTURE_LIMIT');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('stdio wrapper preserves responses, cancellation and activity after end_turn', { timeout: 15000 }, async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'claude-capture-stdio-'));
  const config = path.join(dir, 'config.json');
  await writeFile(config, JSON.stringify({ adapterEntry: path.join(here, 'fixture.mjs'), logsRoot: dir, heartbeatMs: 5 }));
  const child = spawn(process.execPath, [path.join(here, 'capture.mjs'), config], {
    windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
  });
  const exit = once(child, 'exit');
  const frames = [];
  child.stderr.resume();
  const lines = readline.createInterface({ input: child.stdout });
  let continued;
  const continuation = new Promise(resolve => { continued = resolve; });
  lines.on('line', line => {
    const frame = JSON.parse(line);
    frames.push(frame);
    if (frame.method === 'session/request_permission') {
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id,
        result: { outcome: { outcome: 'selected', optionId: 'allow' } } }) + '\n');
    }
    if (frame.method === 'session/update') continued();
  });
  const send = frame => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', ...frame }) + '\n');
  try {
    send({ id: 1, method: 'initialize', params: {} });
    send({ id: 2, method: 'session/new', params: { cwd: dir } });
    send({ id: 3, method: 'session/prompt', params: { sessionId: 'fixture-session', prompt: [{ text: 'sensitive-prompt' }] } });
    await continuation;
    send({ method: 'session/cancel', params: { sessionId: 'fixture-session' } });
    send({ id: 4, method: 'unknown/method', params: {} });
    child.stdin.end();
    assert.equal((await exit)[0], 0);
    assert.deepEqual(frames.find(x => x.id === 3).result, { stopReason: 'end_turn', usage: { totalTokens: 0 } });
    assert.equal(frames.find(x => x.id === 4).error.message, 'unchanged error');
    assert(!frames.some(x => x.method === '_claude/sdkMessage'));
    const captureDir = (await readdir(dir, { withFileTypes: true })).find(x => x.isDirectory()).name;
    const text = await readFile(path.join(dir, captureDir, 'events-00001.jsonl'), 'utf8');
    const events = text.trim().split('\n').map(JSON.parse);
    const resultIndex = events.findIndex(x => x.kind === 'acp_receive' && x.id === 3);
    const backgroundIndex = events.findIndex(x => x.message?.state === 'started');
    assert(backgroundIndex > resultIndex);
    assert(events.some(x => x.message?.state === 'cancelled'));
    assert(events.some(x => x.message?.command_id === 'permission-roundtrip'));
    assert(events.some(x => x.kind === 'client_stdin_eof'));
    assert(events.some(x => x.kind === 'adapter_exit'));
    assert.equal(events.at(-1).kind, 'adapter_exit');
    assert(!text.includes('sensitive-prompt') && !text.includes('sensitive-thinking'));
    assert.equal(JSON.parse(await readFile(path.join(dir, captureDir, 'manifest.json'), 'utf8')).status, 'closed');
  } finally {
    if (child.exitCode === null) child.kill();
    lines.close();
    await rm(dir, { recursive: true, force: true });
  }
});
