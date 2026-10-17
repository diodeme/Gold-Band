import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

// A transparent stdio recorder around the UNMODIFIED official adapter. The Rust
// driver still owns every ACP request, response and permission decision.
const script = fileURLToPath(import.meta.url);
if (process.argv[2] === '--relay') {
  const [, , , entry, workspace, mode] = process.argv;
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
    /^(PATH|PATHEXT|SYSTEMROOT|WINDIR|COMSPEC|NUMBER_OF_PROCESSORS|PROCESSOR_ARCHITECTURE|CLAUDE_CODE_GIT_BASH_PATH)$/i.test(key)));
  for (const name of ['home', 'config', 'temp', 'appdata', 'localappdata']) fs.mkdirSync(path.join(workspace, name));
  Object.assign(env, {
    HOME: path.join(workspace, 'home'), USERPROFILE: path.join(workspace, 'home'),
    APPDATA: path.join(workspace, 'appdata'), LOCALAPPDATA: path.join(workspace, 'localappdata'),
    TEMP: path.join(workspace, 'temp'), TMP: path.join(workspace, 'temp'),
    CLAUDE_CONFIG_DIR: path.join(workspace, 'config'),
    ANTHROPIC_BASE_URL: process.env.GOLD_BAND_SUBAGENT_API,
    ANTHROPIC_API_KEY: 'local-fixture-only', ANTHROPIC_AUTH_TOKEN: 'local-fixture-only',
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', DISABLE_AUTOUPDATER: '1',
    CLAUDE_CODE_USE_BEDROCK: '0', CLAUDE_CODE_USE_VERTEX: '0', CLAUDE_CODE_USE_FOUNDRY: '0',
    HTTP_PROXY: 'http://127.0.0.1:1', HTTPS_PROXY: 'http://127.0.0.1:1',
    ALL_PROXY: 'http://127.0.0.1:1', NO_PROXY: '127.0.0.1,localhost',
  });
  assert.ok(env.ANTHROPIC_BASE_URL.startsWith('http://127.0.0.1:'));
  const trace = path.join(process.env.GOLD_BAND_SUBAGENT_ROOT, `${mode}.ndjson`);
  const record = (direction, line) => {
    const frame = JSON.parse(line);
    fs.appendFileSync(trace, `${JSON.stringify({ direction, frame })}\n`);
  };
  const child = spawn(process.execPath, [entry], { cwd: workspace, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  child.stderr.on('data', bytes => fs.appendFileSync(path.join(process.env.GOLD_BAND_SUBAGENT_ROOT, `${mode}.stderr.log`), bytes));
  const incoming = readline.createInterface({ input: process.stdin });
  incoming.on('line', line => { record('client', line); child.stdin.write(`${line}\n`); });
  incoming.on('close', () => child.stdin.end());
  readline.createInterface({ input: child.stdout }).on('line', line => { record('adapter', line); process.stdout.write(`${line}\n`); });
  const [code] = await once(child, 'exit');
  incoming.close();
  process.exit(code ?? 1);
}

const [testBinary, adapterEntry, selectedModes = 'off,on,async,cancel,permission,resume,replay'] = process.argv.slice(2);
assert.ok(testBinary && adapterEntry, 'Usage: node verify-claude-subagents.mjs <test-exe> <adapter-entry> [off,on,async,cancel,permission,resume,replay]');
const modes = selectedModes.split(',');
assert.ok(modes.every(mode => ['off', 'on', 'async', 'cancel', 'permission', 'resume', 'replay'].includes(mode)));
const packageRoot = path.resolve(adapterEntry, '../..');
const adapterPackage = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json')));
assert.equal(adapterPackage.name, '@agentclientprotocol/claude-agent-acp');
assert.equal(adapterPackage.version, '0.90.0');
const sdkPackage = JSON.parse(fs.readFileSync(path.join(packageRoot, '../../@anthropic-ai/claude-agent-sdk/package.json')));
const repository = path.resolve(path.dirname(script), '../..');
// Optional existing per-investigation parent keeps repeated runs together.
const root = fs.mkdtempSync(path.join(process.env.GOLD_BAND_SUBAGENT_TRACE_PARENT ?? path.join(repository, 'tmp'), 'subagent-protocol-'));
console.log(`Probe traces: ${root}`);
const failures = [];
const observations = [];
let serial = 0;
function send(response, blocks, stopReason = 'end_turn') {
  if (response.destroyed) return;
  response.writeHead(200, { 'Content-Type': 'text/event-stream' });
  const event = (type, fields) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  event('message_start', { message: { id: `msg_subagent_${++serial}`, type: 'message', role: 'assistant',
    model: 'claude-sonnet-4-6', content: [], stop_reason: null, stop_sequence: null,
    usage: { input_tokens: 10, output_tokens: 0 } } });
  blocks.forEach((block, index) => {
    event('content_block_start', { index, content_block: block.type === 'tool_use'
      ? { ...block, input: {} } : { type: 'text', text: '' } });
    event('content_block_delta', { index, delta: block.type === 'tool_use'
      ? { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
      : { type: 'text_delta', text: block.text } });
    event('content_block_stop', { index });
  });
  event('message_delta', { delta: { stop_reason: stopReason, stop_sequence: null }, usage: { output_tokens: 20 } });
  event('message_stop', {});
  response.end();
}
const textReply = (response, text) => send(response, [{ type: 'text', text }]);
const delayedChildren = new Map();
const server = http.createServer(async (request, response) => {
  try {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    if (request.url.startsWith('/v1/messages/count_tokens')) {
      response.writeHead(200, { 'Content-Type': 'application/json' }); response.end('{"input_tokens":10}'); return;
    }
    assert.ok(request.url.startsWith('/v1/messages'), `unexpected endpoint ${request.url}`);
    for (const [header, expected] of [['x-api-key', 'local-fixture-only'], ['authorization', 'Bearer local-fixture-only']]) {
      if (request.headers[header]) assert.equal(request.headers[header], expected, 'only fixture credentials are permitted');
    }
    const body = JSON.parse(Buffer.concat(chunks));
    fs.appendFileSync(path.join(root, 'model.ndjson'), `${JSON.stringify(body)}\n`);
    assert.ok(serial < 80, 'fixture model request bound exceeded');
    const blocks = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [{ type: 'text', text: message.content }]);
    const texts = blocks.filter(block => block.type === 'text').map(block => block.text).join('\n');
    const childMode = /SUBAGENT_CHILD:(off|on|async|cancel|permission|resume|replay)/.exec(texts)?.[1];
    if (childMode) {
      observations.push({ child: childMode });
      if (texts.includes('SUBAGENT_RESUMED_BODY')) {
        textReply(response, 'child-resumed-resume'); return;
      }
      if (childMode === 'permission') {
        const result = blocks.find(block => block.type === 'tool_result' && block.tool_use_id === 'tool_child_permission');
        if (result) {
          assert.equal(result.is_error, true, 'Rust must deny the child permission');
          textReply(response, 'child-complete-permission');
        } else send(response, [{ type: 'tool_use', id: 'tool_child_permission', name: 'Bash',
          input: { command: 'printf isolated-child-permission', description: 'Isolated permission probe' } }], 'tool_use');
        return;
      }
      if (childMode === 'async' || childMode === 'cancel') {
        delayedChildren.set(childMode, response);
        // The parent result, not an arbitrary sleep, releases async completion.
        if (observations.some(item => item.parentResult === childMode) && childMode === 'async') {
          textReply(response, `child-complete-${childMode}`); delayedChildren.delete(childMode);
        }
      } else textReply(response, `child-complete-${childMode}`);
      return;
    }
    const mode = /SUBAGENT_PROBE:(off|on|async|cancel|permission|resume|replay)/.exec(texts)?.[1];
    if (!mode) { textReply(response, 'Isolated protocol fixture'); return; }
    const id = `tool_subagent_${mode}`;
    const result = blocks.find(block => block.type === 'tool_result' && block.tool_use_id === id);
    if (!result) {
      const agent = body.tools.find(tool => tool.name === 'Agent');
      assert.ok(agent, 'Agent tool required');
      observations.push({ mode, agentSchema: agent.input_schema });
      send(response, [{ type: 'tool_use', name: 'Agent', id, input: {
        description: `Isolated ${mode} child`, subagent_type: 'general-purpose',
        prompt: `SUBAGENT_CHILD:${mode}`, run_in_background: mode === 'async',
      } }], 'tool_use');
    } else {
      assert.ok(!result.is_error, JSON.stringify(result));
      observations.push({ parentResult: mode, result });
      if (mode === 'resume' && !blocks.some(block => block.type === 'tool_result' && block.tool_use_id === 'tool_resume')) {
        assert.ok(body.tools.some(tool => tool.name === 'SendMessage'), 'SendMessage required');
        const taskId = /agentId: ([a-zA-Z0-9_-]+)/.exec(JSON.stringify(result.content))?.[1];
        assert.ok(taskId, 'completed Agent must expose its resumable agent id');
        send(response, [{ type: 'tool_use', id: 'tool_resume', name: 'SendMessage', input: {
          to: taskId, message: 'SUBAGENT_RESUMED_BODY', summary: 'Resume isolated child with same identity',
        } }], 'tool_use');
        return;
      }
      textReply(response, `parent-complete-${mode}`);
      if (mode === 'async' && delayedChildren.has(mode)) {
        textReply(delayedChildren.get(mode), `child-complete-${mode}`); delayedChildren.delete(mode);
      }
    }
  } catch (error) {
    failures.push(error.stack);
    if (!response.headersSent) textReply(response, 'Fixture assertion failed'); else response.end();
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let child;
try {
  child = spawn(path.resolve(testBinary), ['--ignored', '--exact', 'installed_claude_subagent_protocol_probe', '--nocapture'], {
    windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, GOLD_BAND_SUBAGENT_ENTRY: path.resolve(adapterEntry),
      GOLD_BAND_SUBAGENT_NODE: process.execPath, GOLD_BAND_SUBAGENT_SCRIPT: script,
      GOLD_BAND_SUBAGENT_ROOT: root, GOLD_BAND_SUBAGENT_MODES: selectedModes,
      GOLD_BAND_SUBAGENT_API: `http://127.0.0.1:${server.address().port}` },
  });
  child.stdout.pipe(process.stdout); child.stderr.pipe(process.stderr);
  const [code] = await once(child, 'exit');
  assert.deepEqual(failures, []);
  assert.equal(code, 0, 'Rust protocol driver failed');
  for (const mode of modes) {
    const frames = fs.readFileSync(path.join(root, `${mode}.ndjson`), 'utf8').trim().split('\n').map(line => JSON.parse(line).frame);
    const updates = frames.filter(frame => frame.method === 'session/update').map(frame => frame.params);
    const running = updates.find(item => item.update.sessionUpdate === 'subagent_update' && item.update.state.state === 'running');
    if (mode === 'off') {
      assert.equal(running, undefined, 'capability-off must not announce child sessions');
      assert.ok(observations.some(item => item.parentResult === mode && JSON.stringify(item.result.content).includes('child-complete-off')));
      continue;
    }
    assert.ok(running, `${mode}: missing child announcement`);
    const childId = running.update.sessionId;
    const childUpdates = updates.filter(item => item.sessionId === childId);
    const delegation = childUpdates.find(item => item.update.sessionUpdate === 'session_message');
    assert.equal(delegation?.update.senderSessionId, running.sessionId);
    assert.equal(delegation?.update.recipientSessionId, childId);
    const launch = updates.find(item => item.sessionId === running.sessionId && item.update.toolCallId === delegation.update.messageId);
    assert.ok(launch, 'child delegation must reference parent launch tool id');
    if (mode !== 'cancel') assert.ok(childUpdates.some(item => item.update.sessionUpdate === 'agent_message_chunk' && item.update.content.text.includes(`child-complete-${mode}`)), `${mode}: missing child body`);
    const idle = item => item.update.sessionUpdate === 'subagent_update' && item.update.sessionId === childId
      && item.update.state.state === 'idle' && item.update.state.stopReason === (mode === 'cancel' ? 'cancelled' : 'end_turn');
    assert.ok(updates.some(idle), `${mode}: missing child idle result`);
    if (mode === 'async') {
      const launchResult = updates.findIndex(item => item.update._meta?.claudeCode?.toolResponse?.status === 'async_launched');
      const bodyIndex = updates.findIndex(item => item.sessionId === childId && item.update.sessionUpdate === 'agent_message_chunk');
      assert.ok(launchResult >= 0 && launchResult < bodyIndex, 'async launch must precede delayed child completion');
      const idleIndex = frames.findIndex(frame => frame.method === 'session/update' && idle(frame.params));
      assert.ok(frames.findIndex(frame => frame.result?.stopReason === 'end_turn') > idleIndex, 'parent prompt must await child idle');
    }
    if (mode === 'permission') {
      const permissions = frames.filter(frame => frame.method === 'session/request_permission');
      assert.equal(permissions.length, 1);
      assert.equal(permissions[0].params.sessionId, childId);
      assert.equal(permissions[0].params.toolCall.toolCallId, 'tool_child_permission');
      assert.ok(frames.some(frame => frame.id === permissions[0].id && frame.result?.outcome?.outcome === 'cancelled'));
    }
    if (mode === 'replay') {
      const loadIndex = frames.findIndex(frame => frame.method === 'session/load');
      assert.ok(loadIndex > 0, 'session/load must follow live completion and close');
      const replay = frames.slice(loadIndex + 1).filter(frame => frame.method === 'session/update').map(frame => frame.params);
      const replayChild = replay.filter(item => item.sessionId === childId);
      assert.ok(replay.some(item => item.update.sessionUpdate === 'subagent_update' && item.update.sessionId === childId));
      assert.ok(replayChild.some(item => item.update.sessionUpdate === 'session_message'
        && item.update.messageId === delegation.update.messageId && item.update.senderSessionId === running.sessionId));
      assert.ok(replayChild.some(item => item.update.content?.text === 'child-complete-replay'));
      assert.ok(replay.some(idle), 'replay must restore child idle/end_turn');
    }
    if (mode === 'resume') {
      assert.ok(updates.some(item => item.update.toolCallId === 'tool_resume'
        && item.update._meta?.claudeCode?.toolResponse?.success === true), 'SendMessage must succeed, not just complete');
      const announcements = updates.filter(item => item.update.sessionUpdate === 'subagent_update');
      assert.deepEqual([...new Set(announcements.map(item => item.update.sessionId))], [childId]);
      assert.equal(announcements.filter(item => item.update.state.state === 'running').length, 2);
      assert.equal(announcements.filter(idle).length, 2);
      assert.ok(childUpdates.some(item => item.update.sessionUpdate === 'session_message'
        && item.update.messageId === 'tool_resume' && item.update.senderSessionId === running.sessionId));
      assert.ok(childUpdates.some(item => item.update.content?.text === 'child-resumed-resume'));
    }
  }
  const summary = { passed: true, adapter: adapterPackage.version, sdk: sdkPackage.version,
    cli: sdkPackage.claudeCodeVersion, modes, modelResponses: serial, traceRoot: root,
    untested: ['nested subagents', 'cancel background child after parent idle', 'replay after process restart'] };
  fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify(summary, null, 2));
} finally {
  if (child && child.exitCode === null) child.kill();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  fs.writeFileSync(path.join(root, 'observations.json'), JSON.stringify({ failures, observations }, null, 2));
}
