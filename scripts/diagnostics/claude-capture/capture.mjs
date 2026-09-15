import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import split from 'split2';
import { injectCapture, Journal, requestedSDK, rpcSummary, sdkSummary, sessionMethods } from './core.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const config = JSON.parse(await readFile(process.argv[2] ?? path.join(here, 'config.json'), 'utf8'));
const directory = path.join(config.logsRoot, `${new Date().toISOString().replaceAll(':', '-')}-${process.pid}`);
await mkdir(directory, { recursive: true });
const journal = new Journal(directory, config.ledger);
const stderr = new Journal(directory, { ...config.stderr, name: 'stderr' });
const sessions = new Map();
const pending = new Map();
const deltaCounts = new Map();
let sessionOrdinal = 0;
let heartbeatBusy = false;
let heartbeatTask = Promise.resolve();
const manifest = { schemaVersion: 1, status: 'recording', startedAt: new Date().toISOString(),
  wrapperPid: process.pid, parentPid: process.ppid, cwd: process.cwd(),
  adapterEntry: config.adapterEntry, node: process.version,
  cliOverride: process.env.CLAUDE_CODE_EXECUTABLE ?? null,
  capture: { sdk: true, nativeDebug: true, telemetry: config.telemetryEndpoint ? 'otlp' : false, httpWire: false, apiBodies: false },
  limits: { ledger: config.ledger, stderr: config.stderr } };
manifest.adapterVersion = await readFile(path.resolve(config.adapterEntry, '../../package.json'), 'utf8')
  .then(JSON.parse).then(p => p.version).catch(() => null);
await writeFile(path.join(directory, 'manifest.json'), JSON.stringify(manifest, null, 2));
await journal.record({ kind: 'capture_start', ...manifest });
if (config.collectorStartup) {
  try {
    await promisify(execFile)('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', config.collectorStartup],
      { windowsHide: true, timeout: 45000 });
    await journal.record({ kind: 'collector_ready' });
  } catch (error) {
    manifest.collectorError = error.code ?? 'COLLECTOR_START_FAILED';
    await journal.record({ kind: 'collector_unavailable', code: manifest.collectorError });
    process.stderr.write('CLAUDE_CAPTURE_INCOMPLETE collector unavailable\n');
  }
}
const child = spawn(process.execPath, [config.adapterEntry], {
  cwd: process.cwd(), windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
  env: { ...process.env, CLAUDE_AGENT_LOGS: directory,
    CLAUDE_CODE_ENABLE_TELEMETRY: config.telemetryEndpoint ? '1' : '0',
    OTEL_LOGS_EXPORTER: config.telemetryEndpoint ? 'otlp' : 'none',
    OTEL_EXPORTER_OTLP_LOGS_ENDPOINT: config.telemetryEndpoint ?? '',
    OTEL_EXPORTER_OTLP_LOGS_PROTOCOL: 'http/json',
    OTEL_EXPORTER_OTLP_LOGS_HEADERS: '', OTEL_EXPORTER_OTLP_HEADERS: '',
    OTEL_RESOURCE_ATTRIBUTES: `gold_band.capture_id=${path.basename(directory)}`,
    OTEL_METRICS_EXPORTER: 'none', OTEL_TRACES_EXPORTER: 'none',
    OTEL_LOGS_EXPORT_INTERVAL: '1000', OTEL_LOG_USER_PROMPTS: '0', OTEL_LOG_TOOL_DETAILS: '0',
    OTEL_LOG_RAW_API_BODIES: '',
  },
});
const childDone = new Promise(resolve => {
  child.once('error', error => resolve({ code: null, errorCode: error.code }));
  child.once('exit', (code, signal) => resolve({ code, signal }));
});
await journal.record({ kind: 'adapter_spawn', pid: child.pid });

async function send(stream, frame) {
  if (!stream.write(JSON.stringify(frame) + '\n')) await once(stream, 'drain');
}

async function flushDeltas() {
  for (const [sessionId, counts] of deltaCounts) {
    deltaCounts.delete(sessionId);
    await journal.record({ kind: 'stream_counts', sessionId, ...counts });
  }
}

function countDelta(sessionId, bytes, type) {
  const counts = deltaCounts.get(sessionId) ?? { count: 0, bytes: 0 };
  counts.count++;
  counts.bytes += bytes;
  counts.lastType = type;
  counts.lastAt = new Date().toISOString();
  deltaCounts.set(sessionId, counts);
}

async function writeHeartbeat() {
  if (heartbeatBusy) return;
  heartbeatBusy = true;
  try {
    await flushDeltas();
    await journal.record({ kind: 'heartbeat', adapterPid: child.pid, pendingRequests: pending.size });
    if (config.collectorHealth) {
      try {
        const response = await fetch(config.collectorHealth, { signal: AbortSignal.timeout(2000) });
        await response.body?.cancel();
        if (!response.ok) throw new Error('collector unhealthy');
      } catch {
        manifest.collectorError = 'COLLECTOR_HEALTH_GAP';
        await journal.record({ kind: 'collector_health_gap' });
        process.stderr.write('CLAUDE_CAPTURE_INCOMPLETE collector health gap\n');
        if (config.collectorStartup) {
          await promisify(execFile)('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', config.collectorStartup],
            { windowsHide: true, timeout: 45000 }).then(
              () => journal.record({ kind: 'collector_restarted' }),
              () => journal.record({ kind: 'collector_restart_failed' }));
        }
      }
    }
  } finally { heartbeatBusy = false; }
}
const heartbeat = setInterval(() => {
  if (!heartbeatBusy) heartbeatTask = writeHeartbeat();
}, config.heartbeatMs ?? 60000);
heartbeat.unref();

async function input() {
  for await (const line of process.stdin.pipe(split())) {
    if (!line.trim()) continue;
    const frame = JSON.parse(line);
    await journal.record({ kind: 'acp_send', ...rpcSummary(frame) });
    if (frame.id !== undefined && frame.method) pending.set(frame.id, {
      method: frame.method, sessionId: frame.params?.sessionId,
      raw: frame.params?._meta?.claudeCode?.emitRawSDKMessages,
    });
    const outgoing = injectCapture(frame, path.join(directory,
      `native-${sessionMethods.has(frame.method) ? ++sessionOrdinal : sessionOrdinal}.debug.log`));
    if (sessionMethods.has(frame.method)) await journal.record({ kind: 'session_capture_config',
      requestId: frame.id, debugFile: outgoing.params._meta.claudeCode.options.debugFile });
    await send(child.stdin, outgoing);
  }
  await journal.record({ kind: 'client_stdin_eof' });
  child.stdin.end();
}

async function output() {
  for await (const line of child.stdout.pipe(split())) {
    if (!line.trim()) continue;
    const frame = JSON.parse(line);
    if (frame.method === '_claude/sdkMessage') {
      const { message, sessionId } = frame.params;
      if (message.type === 'stream_event' && message.event?.type === 'content_block_delta') {
        countDelta(sessionId, Buffer.byteLength(line), message.event.delta?.type);
      } else {
        await flushDeltas();
        await journal.record({ kind: 'sdk', sessionId, message: sdkSummary(message) });
      }
      // Extra diagnostic notifications stay local unless the client requested them.
      if (requestedSDK(sessions.get(sessionId), message)) await send(process.stdout, frame);
      continue;
    }
    const update = frame.params?.update?.sessionUpdate;
    if (['agent_message_chunk', 'agent_thought_chunk', 'user_message_chunk'].includes(update)) {
      countDelta(frame.params.sessionId, Buffer.byteLength(line), update);
    } else {
      const request = frame.method === undefined && frame.id !== undefined ? pending.get(frame.id) : undefined;
      if (request) {
        if (sessionMethods.has(request.method) && !frame.error) {
          sessions.set(frame.result?.sessionId ?? request.sessionId, request.raw);
        }
        pending.delete(frame.id);
      }
      await flushDeltas();
      await journal.record({ kind: 'acp_receive', ...rpcSummary(frame),
        requestMethod: request?.method, requestSessionId: request?.sessionId });
    }
    await send(process.stdout, frame);
  }
  await journal.record({ kind: 'adapter_stdout_eof' });
}

async function errors() {
  for await (const chunk of child.stderr) {
    await stderr.record({ kind: 'stderr', text: chunk.toString('utf8') });
    if (!process.stderr.write(chunk)) await once(process.stderr, 'drain');
  }
}

let stopTimer;
function stop(signal) {
  void journal.record({ kind: 'wrapper_signal', signal });
  child.kill(signal);
}
process.once('SIGINT', () => stop('SIGINT'));
process.once('SIGTERM', () => stop('SIGTERM'));
// Install error listeners so broken pipes become recorded failures, not unhandled events.
for (const stream of [child.stdin, process.stdout, process.stderr]) stream.on('error', () => {});
const tasks = [input(), output(), errors()].map(p => p.catch(async error => {
  await journal.record({ kind: 'capture_transport_error', code: error.code ?? 'TRANSPORT_ERROR' });
  manifest.transportError = error.code ?? 'TRANSPORT_ERROR';
  child.stdin.end();
  stopTimer ??= setTimeout(() => child.kill(), 5000);
}));
const exit = await childDone;
clearInterval(heartbeat);
clearTimeout(stopTimer);
process.stdin.destroy();
await Promise.all(tasks);
await heartbeatTask;
await flushDeltas();
await journal.record({ kind: 'adapter_exit', ...exit });
await journal.close();
await stderr.close();
Object.assign(manifest, { status: manifest.transportError || manifest.collectorError || journal.disabled || stderr.disabled || exit.errorCode
  ? 'incomplete' : 'closed', endedAt: new Date().toISOString(), adapterExit: exit });
await writeFile(path.join(directory, 'manifest.json'), JSON.stringify(manifest, null, 2));
process.exitCode = exit.code ?? 1;
