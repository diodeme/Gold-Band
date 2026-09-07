import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import http from 'node:http';
import path from 'node:path';

// Drive the installed adapter through Gold Band's real connection boundary.
// All model responses come from this local fixture; no credentials are used.
const [testBinary, adapterEntry] = process.argv.slice(2);
assert.ok(testBinary && adapterEntry, 'Usage: node verify-claude-background-policy.mjs <test-exe> <adapter-entry>');
const observations = [];
const failures = [];
let serial = 0;
function send(response, blocks, stopReason = 'end_turn') {
  response.writeHead(200, { 'Content-Type': 'text/event-stream' });
  const event = (type, fields) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
  event('message_start', { message: { id: `msg_policy_${++serial}`, type: 'message', role: 'assistant',
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
const server = http.createServer(async (request, response) => {
  try {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    if (!request.url.startsWith('/v1/messages') || request.url.includes('count_tokens')) {
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end('{"input_tokens":10}');
      return;
    }
    const body = JSON.parse(Buffer.concat(chunks));
    const text = JSON.stringify(body.messages);
    const lastUserText = body.messages.filter(message => message.role === 'user')
      .flatMap(message => typeof message.content === 'string' ? [message.content]
        : message.content.filter(block => block.type === 'text').map(block => block.text)).at(-1);
    if (lastUserText?.includes('POLICY_CHILD_REPLY')) {
      send(response, [{ type: 'text', text: 'policy-child-complete' }]);
      return;
    }
    const mode = /POLICY_PROBE:(synchronous|background|monitor|subagent)/.exec(text)?.[1];
    if (!mode) { send(response, [{ type: 'text', text: 'Policy probe' }]); return; }
    const toolId = `tool_policy_${mode}`;
    const results = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
      .filter(block => block.type === 'tool_result' && block.tool_use_id === toolId);
    if (!results.length) {
      const bash = body.tools.find(tool => tool.name === 'Bash');
      assert.ok(bash, 'synchronous Bash must remain available');
      assert.ok(!body.tools.some(tool => tool.name === 'Monitor'), 'Monitor must be removed');
      assert.ok(!Object.hasOwn(bash.input_schema.properties, 'run_in_background'), 'background parameter must be removed');
      observations.push({ mode, bashAvailable: true, monitorAvailable: false, backgroundParameterAvailable: false });
      if (mode === 'subagent') {
        const agent = body.tools.find(tool => tool.name === 'Agent');
        assert.ok(agent, 'foreground Agent must remain available');
        assert.ok(!Object.hasOwn(agent.input_schema.properties, 'run_in_background'), 'Agent background parameter must be removed');
        send(response, [{ type: 'tool_use', id: toolId, name: 'Agent', input: {
          description: 'Synchronous child probe', subagent_type: 'general-purpose',
          prompt: 'POLICY_CHILD_REPLY: reply with policy-child-complete',
        } }], 'tool_use');
        return;
      }
      const command = mode === 'monitor' ? 'echo unexpected > monitor-must-not-run'
        : mode === 'synchronous' ? 'sleep 12; printf policy-sync-complete' : 'printf policy-background-complete';
      send(response, [{ type: 'tool_use', id: toolId, name: mode === 'monitor' ? 'Monitor' : 'Bash',
        input: { command, description: `Policy probe ${mode}`, timeout: 30000,
          ...(mode === 'background' ? { run_in_background: true } : {}) } }], 'tool_use');
      return;
    }
    const result = results.at(-1);
    const output = JSON.stringify(result.content);
    if (mode === 'monitor') assert.equal(result.is_error, true, 'forced Monitor call must fail');
    if (mode === 'synchronous') {
      assert.ok(!result.is_error, output);
      assert.ok(output.includes('policy-sync-complete'), 'long command must return its result in this prompt');
    }
    if (mode === 'background') {
      assert.ok(result.is_error || output.includes('policy-background-complete'), 'background request must fail or finish synchronously');
      assert.ok(!/running in background|background task|task-notification/i.test(output), output);
    }
    if (mode === 'subagent') {
      assert.ok(!result.is_error, output);
      assert.ok(output.includes('policy-child-complete'), 'child result must return synchronously');
    }
    observations.push({ mode, toolResultReceived: true, isError: result.is_error ?? false });
    send(response, [{ type: 'text', text: '{"status":"success","probe":"complete"}' }]);
  } catch (error) {
    failures.push(error.message);
    if (!response.headersSent) send(response, [{ type: 'text', text: 'Probe failed.' }]);
    else response.end();
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
let child;
try {
  child = spawn(path.resolve(testBinary), ['--ignored', '--exact', 'installed_claude_policy_probe', '--nocapture'], {
    windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, GOLD_BAND_POLICY_ADAPTER_ENTRY: path.resolve(adapterEntry),
      GOLD_BAND_POLICY_NODE: process.execPath, GOLD_BAND_POLICY_API: `http://127.0.0.1:${server.address().port}` },
  });
  child.stdout.pipe(process.stdout);
  child.stderr.pipe(process.stderr);
  const [code] = await once(child, 'exit');
  assert.equal(code, 0, 'installed adapter probe failed');
  assert.deepEqual(failures, []);
  for (const mode of ['synchronous', 'background', 'monitor', 'subagent']) {
    assert.ok(observations.some(item => item.mode === mode && item.toolResultReceived), `missing ${mode} tool result`);
  }
  console.log(JSON.stringify({ passed: true, observations }, null, 2));
} finally {
  if (child && child.exitCode === null) child.kill();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}
