import readline from 'node:readline';

const send = frame => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...frame }) + '\n');
const sdk = message => send({ method: '_claude/sdkMessage', params: { sessionId: 'fixture-session', message } });
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const frame = JSON.parse(line);
  if (!frame.method) {
    if (frame.id === 99 && frame.result?.outcome?.optionId === 'allow') {
      sdk({ type: 'command_lifecycle', state: 'completed', command_id: 'permission-roundtrip' });
    }
    continue;
  }
  if (frame.method === 'initialize') send({ id: frame.id, result: { protocolVersion: 1 } });
  if (frame.method === 'session/new') {
    if (!frame.params._meta?.claudeCode?.options?.debug ||
        !frame.params._meta.claudeCode.emitRawSDKMessages.some(x => x.type === 'stream_event')) process.exit(5);
    send({ id: frame.id, result: { sessionId: 'fixture-session' } });
  }
  if (frame.method === 'session/prompt') {
    send({ id: 99, method: 'session/request_permission', params: { sessionId: 'fixture-session',
      toolCall: { toolCallId: 'tool-1' }, options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow' }] } });
    sdk({ type: 'stream_event', event: { type: 'message_start', message: { id: 'msg_test', stop_reason: null } } });
    sdk({ type: 'stream_event', event: { type: 'content_block_delta', delta: { type: 'thinking_delta', thinking: 'sensitive-thinking' } } });
    sdk({ type: 'result', subtype: 'success', is_error: false, stop_reason: null, result: '', usage: { input_tokens: 0 } });
    send({ id: frame.id, result: { stopReason: 'end_turn', usage: { totalTokens: 0 } } });
    // A response is deliberately followed by more native activity in the same connection.
    sdk({ type: 'command_lifecycle', state: 'started', command_id: 'background-command' });
    sdk({ type: 'system', subtype: 'task_notification', task_id: 'download', status: 'completed' });
    send({ method: 'session/update', params: { sessionId: 'fixture-session', update: {
      sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'continuation' },
    } } });
  }
  if (frame.method === 'session/cancel') {
    sdk({ type: 'command_lifecycle', state: 'cancelled', command_id: 'background-command' });
  }
  if (frame.id !== undefined && !['initialize', 'session/new', 'session/prompt'].includes(frame.method)) {
    send({ id: frame.id, error: { code: -32601, message: 'unchanged error' } });
  }
}
