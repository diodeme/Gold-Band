import { createHash } from 'node:crypto';
import { mkdir, open, writeFile } from 'node:fs/promises';
import path from 'node:path';

export const sessionMethods = new Set(['session/new', 'session/load', 'session/resume', 'session/fork']);
const sdkTypes = ['result', 'stream_event', 'system', 'assistant', 'user',
  'command_lifecycle', 'session_state_changed', 'task_started', 'task_notification',
  'background_tasks_changed'];
const scalarKeys = ['type', 'subtype', 'uuid', 'session_id', 'user_message_uuid',
  'parent_tool_use_id', 'task_id', 'tool_use_id', 'command_id', 'state', 'status',
  'stop_reason', 'is_error', 'num_turns', 'duration_ms', 'duration_api_ms',
  'request_id', 'client_request_id', 'model', 'id', 'index', 'exit_code',
  'task_type', 'subagent_type', 'output_file', 'claude_code_version', 'timestamp',
  'attempt', 'max_retries', 'retry_delay_ms', 'error_status', 'compact_result'];

function scalars(value, keys = scalarKeys) {
  return Object.fromEntries(keys.filter(k => value?.[k] !== undefined &&
    (value[k] === null || ['string', 'number', 'boolean'].includes(typeof value[k])))
    .map(k => [k, typeof value[k] === 'string' ? value[k].slice(0, 2048) : value[k]]));
}

export function requestedSDK(config, message) {
  return config === true || (Array.isArray(config) && config.some(f =>
    f.type === message.type && (f.subtype === undefined || f.subtype === message.subtype) &&
    (f.origin === undefined || f.origin === message.origin?.kind)));
}

export function injectCapture(frame, debugFile) {
  if (!sessionMethods.has(frame.method)) return frame;
  const output = structuredClone(frame);
  const params = output.params ??= {};
  const meta = params._meta ??= {};
  const claude = meta.claudeCode ??= {};
  const original = claude.emitRawSDKMessages;
  claude.emitRawSDKMessages = original === true ? true : [
    ...(Array.isArray(original) ? original : []), ...sdkTypes.map(type => ({ type })),
  ];
  claude.options = { ...claude.options, debug: true, debugFile };
  return output;
}

export function sdkSummary(message) {
  const out = scalars(message);
  if (message.origin) out.origin = scalars(message.origin, ['kind', 'id']);
  if (Array.isArray(message.tasks)) {
    out.tasks = message.tasks.map(task => scalars(task, ['task_id', 'task_type', 'status']));
  }
  if (message.patch) out.patch = scalars(message.patch, ['status', 'end_time', 'is_backgrounded', 'total_paused_ms']);
  if (message.compact_metadata) out.compact_metadata = scalars(message.compact_metadata,
    ['trigger', 'pre_tokens', 'post_tokens', 'duration_ms']);
  if (message.usage) out.usage = scalars(message.usage, Object.keys(message.usage));
  if (typeof message.result === 'string') out.resultBytes = Buffer.byteLength(message.result);
  if (message.event) {
    out.event = scalars(message.event);
    if (message.event.delta) out.event.delta = scalars(message.event.delta);
    if (message.event.message) out.event.message = scalars(message.event.message);
    if (message.event.content_block) out.event.block = scalars(message.event.content_block, ['type', 'id', 'name']);
    if (message.event.usage) out.event.usage = scalars(message.event.usage, Object.keys(message.event.usage));
    if (message.event.error) out.event.error = scalars(message.event.error, ['type', 'code']);
  }
  if (message.message) {
    out.message = scalars(message.message);
    if (Array.isArray(message.message.content)) {
      out.message.contentTypes = message.message.content.map(x => x.type);
    }
  }
  // Keep unknown lifecycle shapes observable without copying their payloads.
  out.keys = Object.keys(message);
  return out;
}

export function rpcSummary(frame) {
  const out = { id: frame.id, method: frame.method, sessionId: frame.params?.sessionId };
  if (frame.method === 'session/prompt') {
    const prompt = JSON.stringify(frame.params?.prompt ?? []);
    out.promptSha256 = createHash('sha256').update(prompt).digest('hex');
    out.promptBytes = Buffer.byteLength(prompt);
  }
  if (sessionMethods.has(frame.method)) out.cwd = frame.params?.cwd;
  if (frame.result !== undefined) {
    out.result = scalars(frame.result, ['sessionId', 'stopReason', 'protocolVersion']);
    if (frame.result.agentInfo) out.result.agentInfo = scalars(frame.result.agentInfo, ['name', 'version']);
    if (frame.result.usage) out.result.usage = scalars(frame.result.usage, Object.keys(frame.result.usage));
  }
  if (frame.error) out.error = scalars(frame.error, ['code']);
  const u = frame.params?.update;
  if (u) out.update = scalars(u, ['sessionUpdate', 'toolCallId', 'status', 'state', 'currentModeId']);
  return out;
}

export class Journal {
  constructor(directory, { segmentBytes = 8 * 1024 * 1024, maxBytes = 512 * 1024 * 1024,
    name = 'events', warn = message => process.stderr.write(message + '\n') } = {}) {
    Object.assign(this, { directory, segmentBytes, maxBytes, name, warn });
    this.sequence = 0;
    this.segment = 0;
    this.bytes = 0;
    this.segmentSize = 0;
    this.queue = Promise.resolve();
    this.start = process.hrtime.bigint();
    this.disabled = false;
  }

  record(event) {
    // Callers await writes; the two transport directions and heartbeat bound the queue.
    this.queue = this.queue.then(async () => {
      if (this.disabled) return;
      try {
        if (!this.file) {
          await mkdir(this.directory, { recursive: true });
          this.file = await open(path.join(this.directory, `${this.name}-${String(++this.segment).padStart(5, '0')}.jsonl`), 'wx');
        }
        const data = JSON.stringify({ ...event, seq: ++this.sequence, at: new Date().toISOString(),
          elapsedMs: Number(process.hrtime.bigint() - this.start) / 1e6 }) + '\n';
        const size = Buffer.byteLength(data);
        if (this.bytes + size > this.maxBytes) throw Object.assign(new Error('capture capacity reached'), { code: 'CAPTURE_LIMIT' });
        if (this.segmentSize > 0 && this.segmentSize + size > this.segmentBytes) {
          await this.file.close();
          this.file = await open(path.join(this.directory, `${this.name}-${String(++this.segment).padStart(5, '0')}.jsonl`), 'wx');
          this.segmentSize = 0;
        }
        await this.file.writeFile(data);
        this.bytes += size;
        this.segmentSize += size;
      } catch (error) {
        this.disabled = true;
        this.warn(`CLAUDE_CAPTURE_INCOMPLETE ${this.name} ${error.code ?? 'WRITE_ERROR'}`);
        await writeFile(path.join(this.directory, `${this.name}.incomplete.json`), JSON.stringify({
          at: new Date().toISOString(), code: error.code ?? 'WRITE_ERROR', lastSequence: this.sequence,
        })).catch(() => {});
      }
    });
    return this.queue;
  }

  async close() {
    await this.queue;
    if (this.file) {
      await this.file.sync().catch(() => {});
      await this.file.close();
      this.file = undefined;
    }
  }
}
