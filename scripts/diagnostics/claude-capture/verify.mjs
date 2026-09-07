import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const installed = await readFile(path.join(here, 'config.json'), 'utf8').then(JSON.parse).catch(() => ({}));
const adapterEntry = path.resolve(process.argv[3] ?? installed.adapterEntry ??
  path.join(here, 'node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js'));
const cliExecutable = createRequire(adapterEntry).resolve('@anthropic-ai/claude-agent-sdk-win32-x64/claude.exe');
const root = process.argv[2] ?? await mkdtemp(path.join(os.tmpdir(), 'claude-capture-verified-'));
await mkdir(root, { recursive: true });
const modes = ['normal', 'thinking_eof', 'partial_text_eof'];
let mode = modes[0];
let calls = 0;
const server = http.createServer(async (request, response) => {
  for await (const chunk of request) { /* Drain the local synthetic request. */ }
  if (!request.url.startsWith('/v1/messages') || request.url.includes('count_tokens')) {
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end('{"input_tokens":10}');
    return;
  }
  calls++;
  response.writeHead(200, { 'Content-Type': 'text/event-stream', 'request-id': `req_capture_${mode}` });
  const send = (type, fields) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  send('message_start', { message: { id: `msg_${mode}`, type: 'message', role: 'assistant',
    model: 'claude-sonnet-4-6', content: [], stop_reason: null, stop_sequence: null,
    usage: { input_tokens: 10, output_tokens: 0 } } });
  let index = 0;
  if (mode !== 'normal') {
    send('content_block_start', { index, content_block: { type: 'thinking', thinking: '', signature: '' } });
    send('content_block_delta', { index, delta: { type: 'thinking_delta', thinking: 'Planning next work.' } });
    send('content_block_delta', { index, delta: { type: 'signature_delta', signature: 'capture-test' } });
    send('content_block_stop', { index });
    index++;
  }
  if (mode !== 'thinking_eof') {
    send('content_block_start', { index, content_block: { type: 'text', text: '' } });
    send('content_block_delta', { index, delta: { type: 'text_delta',
      text: mode === 'normal' ? 'Done.' : '{"status":"unfinished' } });
  }
  if (mode === 'normal') {
    send('content_block_stop', { index });
    send('message_delta', { delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 5 } });
    send('message_stop', {});
  }
  response.end();
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const configPath = path.join(root, 'verify-config.json');
await writeFile(configPath, JSON.stringify({ logsRoot: path.join(root, 'captures'),
  telemetryEndpoint: installed.telemetryEndpoint, collectorStartup: installed.collectorStartup,
  adapterEntry }));
const child = spawn(process.execPath, [path.join(here, 'capture.mjs'), configPath], {
  cwd: root, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
  env: { ...process.env, CLAUDE_CONFIG_DIR: root,
    CLAUDE_CODE_EXECUTABLE: cliExecutable,
    ANTHROPIC_BASE_URL: `http://127.0.0.1:${server.address().port}`,
    ANTHROPIC_API_KEY: 'capture-dummy', ANTHROPIC_AUTH_TOKEN: 'capture-dummy',
    CLAUDE_CODE_OAUTH_TOKEN: '', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1',
    CLAUDE_CODE_USE_BEDROCK: '0', CLAUDE_CODE_USE_VERTEX: '0', CLAUDE_CODE_USE_FOUNDRY: '0' },
});
const exit = once(child, 'exit');
const pending = new Map();
const lines = readline.createInterface({ input: child.stdout });
let nextId = 0;
let stderrBytes = 0;
child.stderr.on('data', chunk => { stderrBytes += chunk.length; });
lines.on('line', line => {
  const frame = JSON.parse(line);
  if (frame.id !== undefined && !frame.method) {
    const p = pending.get(frame.id);
    if (!p) return;
    pending.delete(frame.id);
    clearTimeout(p.timer);
    if (frame.error) p.reject(new Error(`RPC ${frame.id}: ${JSON.stringify(frame.error)}`));
    else p.resolve(frame.result);
  } else if (frame.id !== undefined && frame.method) {
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id,
      error: { code: -32601, message: 'No interactive tools in local validation' } }) + '\n');
  }
});
function rpc(method, params) {
  return new Promise((resolve, reject) => {
    const id = ++nextId;
    const timer = setTimeout(() => reject(new Error(`timeout ${method}`)), 30000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}
const results = [];
try {
  const agent = await rpc('initialize', { protocolVersion: 1, clientCapabilities: {},
    clientInfo: { name: 'gold-band-capture-validation', version: '1' } });
  for (mode of modes) {
    const before = calls;
    const session = await rpc('session/new', { cwd: root, mcpServers: [], _meta: { claudeCode: {
      options: { settingSources: [], tools: [], persistSession: false,
        model: 'claude-sonnet-4-6', maxTurns: 2 },
    } } });
    const result = await rpc('session/prompt', { sessionId: session.sessionId,
      prompt: [{ type: 'text', text: 'Reply briefly.' }] });
    results.push({ mode, sessionId: session.sessionId, calls: calls - before, result });
  }
  // Give the native console exporter a chance to flush before closing the connection.
  await new Promise(resolve => setTimeout(resolve, 3000));
  child.stdin.end();
  assert.equal((await exit)[0], 0);
  const directory = path.join(root, 'captures', (await readdir(path.join(root, 'captures')))[0]);
  const files = (await readdir(directory)).filter(x => x.startsWith('events-')).sort();
  const events = (await Promise.all(files.map(f => readFile(path.join(directory, f), 'utf8'))))
    .join('').trim().split('\n').map(JSON.parse);
  for (const row of results) {
    const sdk = events.filter(x => x.kind === 'sdk' && x.sessionId === row.sessionId).map(x => x.message);
    const terminal = sdk.find(x => x.type === 'result');
    assert(terminal, `${row.mode}: missing SDK result`);
    assert.equal(sdk.some(x => x.event?.type === 'message_stop'), row.mode === 'normal');
    assert.equal(terminal.stop_reason, row.mode === 'normal' ? 'end_turn' : null);
    assert.equal(row.result.stopReason, 'end_turn');
  }
  const debug = (await readdir(directory)).filter(x => x.endsWith('.debug.log'));
  assert.equal(debug.length, 3);
  let telemetryVerified = false;
  if (installed.telemetryEndpoint) {
    const telemetry = await readFile(path.join(here, 'telemetry', 'events.jsonl'), 'utf8');
    assert(results.every(row => telemetry.includes(row.sessionId)), 'Missing session IDs in telemetry');
    assert(telemetry.includes('req_capture_normal'), 'Missing request ID in telemetry');
    telemetryVerified = true;
  }
  const report = { passed: true, telemetryVerified, agent: agent.agentInfo, adapterEntry, cliExecutable, root, directory,
    assertions: ['real adapter normal termination', 'two incomplete streams observable',
      'unchanged ACP results', 'three native debug files', 'clean wrapper shutdown'],
    results, debugFiles: debug, stderrBytes };
  await writeFile(path.join(root, 'verification.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally {
  for (const p of pending.values()) clearTimeout(p.timer);
  if (child.exitCode === null) {
    child.stdin.end();
    const timer = setTimeout(() => child.kill(), 3000);
    await exit;
    clearTimeout(timer);
  }
  lines.close();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}
