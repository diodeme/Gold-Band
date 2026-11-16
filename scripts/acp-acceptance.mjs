// Generic ACP acceptance driver shared by DeepSeek Harness, Qoder and other adapters.
//
// Usage:
//   node scripts/acp-acceptance.mjs <acp-command> [args...]
//
// Env:
//   ACP_ACCEPTANCE_SKILLS=a,b   run a skill probe instead of the full lifecycle
//   ACP_ACCEPTANCE_TIMEOUT_SCALE=2   scale every request timeout
//
// The driver mirrors the production client contract: it creates the session
// first and only authenticates when `session/new` answers with the ACP
// AuthRequired error, retrying the session exactly once. It never prints
// credential values.

import { spawn, spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

const [command, ...commandArgs] = process.argv.slice(2);
if (!command) {
  console.error('Usage: node scripts/acp-acceptance.mjs <acp-command> [args...]');
  process.exit(2);
}

const cwd = process.cwd();
const timeoutScale = Number(process.env.ACP_ACCEPTANCE_TIMEOUT_SCALE ?? '1') || 1;
const skillNames = (process.env.ACP_ACCEPTANCE_SKILLS ?? '')
  .split(',')
  .map(name => name.trim())
  .filter(Boolean);
const rememberedToken = 'D5H2';
const authRequiredCode = -32000;

const events = [];
const permissions = [];
const pending = new Map();
let nextId = 1;
let child;
let phase = 'start';
let authMethods = [];
let stderrTail = '';
const authState = { called: false, methodId: null, trigger: null };

class JsonRpcError extends Error {
  constructor(error) {
    super(error?.message ?? 'ACP request failed');
    this.name = 'JsonRpcError';
    this.code = error?.code;
    this.data = error?.data;
  }
}

function scaled(ms) {
  return Math.round(ms * timeoutScale);
}

function progress(message) {
  console.error(`[acp-acceptance ${new Date().toISOString()}] ${message}`);
}

function summarize(value) {
  if (Array.isArray(value)) return value.map(summarize);
  if (!value || typeof value !== 'object') return value;
  const output = {};
  for (const [key, input] of Object.entries(value)) {
    const text = typeof input === 'string' ? input : JSON.stringify(input);
    output[key] = typeof input === 'string' && input.length > 300 ? `${text.slice(0, 300)}...` : summarize(input);
  }
  return output;
}

function scrubAuthMethod(method) {
  return {
    id: method?.id ?? method?.methodId ?? null,
    type: method?.type ?? null,
    name: method?.name ?? null,
    envVars: (method?.vars ?? []).map(variable => variable?.name).filter(Boolean),
  };
}

function quoteArg(value) {
  return /[\s"]/u.test(value) ? `"${value.replaceAll('"', '\\"')}"` : value;
}

function usesWindowsShellShim() {
  if (process.platform !== 'win32') return false;
  if (/\.(exe|cmd|bat)$/iu.test(command)) return false;
  return !command.includes('\\') && !command.includes('/');
}

function spawnChild() {
  const useShell = usesWindowsShellShim();
  const args = useShell ? commandArgs.map(quoteArg) : commandArgs;
  const proc = spawn(command, args, {
    cwd,
    stdio: ['pipe', 'pipe', 'pipe'],
    shell: useShell,
    windowsHide: true,
    env: process.env,
  });
  let stdoutBuffer = '';
  proc.stdout.setEncoding('utf8');
  proc.stdout.on('data', data => {
    stdoutBuffer += data;
    let newline = stdoutBuffer.indexOf('\n');
    while (newline >= 0) {
      const line = stdoutBuffer.slice(0, newline).trim();
      stdoutBuffer = stdoutBuffer.slice(newline + 1);
      if (line) {
        try {
          handleFrame(JSON.parse(line));
        } catch (error) {
          record('stdoutParseError', { message: error.message, prefix: line.slice(0, 300) });
        }
      }
      newline = stdoutBuffer.indexOf('\n');
    }
  });
  let stderr = '';
  proc.stderr.setEncoding('utf8');
  proc.stderr.on('data', data => {
    stderr += data;
    stderrTail = `${stderrTail}${data}`.slice(-2000);
  });
  proc.on('exit', (code, signal) => record('exit', { code, signal, stderrTail: stderr.slice(-2000) }));
  return proc;
}

function record(kind, detail) {
  events.push({ at: new Date().toISOString(), kind, detail: summarize(detail) });
}

function send(method, params) {
  const id = nextId++;
  const frame = { jsonrpc: '2.0', id, method, params };
  child.stdin.write(`${JSON.stringify(frame)}\n`);
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject, method, startedAt: Date.now() });
  });
}

function notify(method, params) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, params })}\n`);
}

function respond(id, result) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, result })}\n`);
}

function respondError(id, code, message) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, error: { code, message } })}\n`);
}

function handleFrame(frame) {
  if (frame.id !== undefined && frame.method === undefined) {
    const entry = pending.get(frame.id);
    if (!entry) return;
    pending.delete(frame.id);
    if (frame.error) entry.reject(new JsonRpcError(frame.error));
    else entry.resolve(frame.result ?? {});
    return;
  }
  if (frame.id !== undefined && frame.method !== undefined) {
    handleAgentRequest(frame);
    return;
  }
  if (frame.method?.startsWith('session/')) {
    const update = frame.params?.update ?? {};
    events.push({
      at: new Date().toISOString(),
      kind: 'notification',
      method: frame.method,
      detail: summarize(frame.params),
    });
    record('updateClass', {
      method: frame.method,
      update: update.sessionUpdate,
      toolCallId: update.toolCallId,
      status: update.status,
    });
  }
}

function handleAgentRequest(frame) {
  if (frame.method === 'session/request_permission') {
    const options = frame.params?.options ?? [];
    const selected = options.find(option => option.kind === 'allow_once')
      ?? options.find(option => option.kind === 'allow_always')
      ?? options[0];
    permissions.push({
      title: frame.params?.title,
      subjectType: frame.params?.subject?.type,
      selectedKind: selected?.kind ?? null,
      selectedName: selected?.name ?? null,
    });
    if (selected) {
      respond(frame.id, { outcome: { outcome: 'selected', optionId: selected.optionId } });
    } else {
      respond(frame.id, { outcome: { outcome: 'cancelled' } });
    }
    return;
  }
  respondError(frame.id, -32601, `Unsupported client method: ${frame.method}`);
}

async function request(method, params, timeoutMs) {
  const requestPromise = send(method, params);
  let timer;
  const timeoutPromise = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${method} timed out after ${Math.round(timeoutMs / 1000)}s`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([requestPromise, timeoutPromise]);
  } finally {
    clearTimeout(timer);
  }
}

function isAuthRequired(error) {
  return error instanceof JsonRpcError && error.code === authRequiredCode;
}

function missingEnvFor(method) {
  return (method?.vars ?? [])
    .filter(variable => variable?.optional !== true)
    .map(variable => variable?.name)
    .filter(name => typeof name === 'string' && name.trim() && !process.env[name]);
}

function selectAuthMethod(methods) {
  const configuredEnv = methods.find(method => method.type === 'env_var'
    && missingEnvFor(method).length === 0
    && (method.id ?? method.methodId));
  return configuredEnv ?? methods.find(method => !['terminal', 'env_var'].includes(method.type)
    && (method.id ?? method.methodId)) ?? null;
}

function methodIdOf(method) {
  return method.id ?? method.methodId ?? null;
}

async function initializeSession() {
  phase = 'initialize';
  progress('initialize');
  const result = await request('initialize', {
    protocolVersion: 1,
    clientCapabilities: {
      subagents: {},
      session: { compaction: {} },
      elicitation: { form: {} },
    },
    clientInfo: { name: 'gold-band-acceptance', title: 'Gold Band acceptance', version: '0.0.0' },
  }, scaled(120_000));
  authMethods = result.authMethods ?? [];
  return result;
}

async function createSession() {
  phase = 'session/new';
  progress('session/new');
  const params = { cwd, mcpServers: [] };
  try {
    return await request('session/new', params, scaled(60_000));
  } catch (error) {
    if (!isAuthRequired(error)) throw error;
    const method = selectAuthMethod(authMethods);
    if (!method) {
      const missingEnv = [...new Set(authMethods.flatMap(missingEnvFor))];
      const failure = new Error('session/new requires authentication and no automatic method is available');
      failure.code = 'provider.auth-required';
      failure.missingEnv = missingEnv;
      throw failure;
    }
    authState.called = true;
    authState.methodId = methodIdOf(method);
    authState.trigger = 'session/new AuthRequired';
    phase = 'authenticate';
    progress(`authenticate ${authState.methodId}`);
    await request('authenticate', { methodId: authState.methodId }, scaled(30_000));
    phase = 'session/new retry';
    progress('session/new retry after authenticate');
    return await request('session/new', params, scaled(60_000));
  }
}

function toolCalls() {
  return events
    .filter(item => item.kind === 'notification')
    .map(item => item.detail?.update ?? {})
    .filter(update => String(update.sessionUpdate ?? '').startsWith('tool_call'));
}

async function waitFor(predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await delay(100);
  }
  throw new Error(`${label} was not observed within ${Math.round(timeoutMs / 1000)}s`);
}

async function waitForExit(proc, timeoutMs) {
  return Promise.race([
    new Promise(resolve => proc.once('exit', () => resolve(true))),
    delay(timeoutMs).then(() => false),
  ]);
}

function forceProcessTree(proc) {
  if (!proc?.pid) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(proc.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    proc.kill('SIGKILL');
  }
}

function promptTurn(sessionId, text, timeoutMs) {
  return request('session/prompt', { sessionId, prompt: [{ type: 'text', text }] }, timeoutMs);
}

function finalTextsSince(index) {
  return events
    .slice(index)
    .filter(item => item.kind === 'notification')
    .map(item => item.detail?.update)
    .filter(update => update?.sessionUpdate === 'agent_message_chunk')
    .map(update => (typeof update.content === 'string' ? update.content : update.content?.text ?? ''))
    .join('');
}

function resetTransport() {
  for (const entry of pending.values()) entry.reject(new Error('transport restarted'));
  pending.clear();
  nextId = 1;
}

async function stopChild(proc, timeoutMs = 10_000) {
  if (!proc) return true;
  if (proc.exitCode !== null) return true;
  proc.removeAllListeners('exit');
  proc.kill();
  const exited = await waitForExit(proc, timeoutMs);
  if (!exited) forceProcessTree(proc);
  return exited;
}

async function runSkillProbe(sessionId, capabilities) {
  phase = 'skill prompt';
  progress(`skill prompt: ${skillNames.join(', ')}`);
  const instruction = skillNames.map(name => `exact skill name "${name}"`).join(', then ');
  const turn = await promptTurn(
    sessionId,
    `Use the skill tool to invoke the ${instruction}. After all skill calls complete, reply exactly SKILL_BOTH_OK.`,
    scaled(180_000),
  );
  const calls = events
    .filter(item => item.kind === 'notification')
    .map(item => item.detail?.update)
    .filter(update => update?.sessionUpdate === 'tool_call' && String(update.title ?? '').toLowerCase().includes('skill'))
    .map(update => ({ id: update.toolCallId, input: update.rawInput }));
  const texts = finalTextsSince(0);
  let closeResult = null;
  if (capabilities.sessionCapabilities?.close) {
    phase = 'session/close';
    closeResult = await request('session/close', { sessionId }, scaled(30_000));
  }
  const exited = await stopChild(child);
  console.log(JSON.stringify({
    mode: 'skills',
    cwd,
    command,
    requestedSkills: skillNames,
    skillCalls: calls,
    promptStopReason: turn.stopReason ?? null,
    bothMarkerObserved: texts.includes('SKILL_BOTH_OK'),
    closeResult,
    processExited: exited,
  }, null, 2));
}

async function runFullLifecycle(initializeResult) {
  const capabilities = initializeResult.agentCapabilities ?? {};
  const sessionCapabilities = capabilities.sessionCapabilities ?? {};
  const newSession = await createSession();
  const sessionId = newSession.sessionId;
  if (!sessionId) throw new Error('session/new did not return sessionId');

  if (skillNames.length > 0) {
    await runSkillProbe(sessionId, capabilities);
    return 0;
  }

  phase = 'tool prompt';
  progress('tool prompt');
  const toolTurn = await promptTurn(
    sessionId,
    `Call one read-only file-system tool to list the immediate entries of the current working directory. Then reply exactly: TOOL_OK:${rememberedToken}`,
    scaled(180_000),
  );
  if (toolCalls().length === 0) await waitFor(() => toolCalls().length > 0, 15_000, 'tool call');

  let listResult = null;
  if (sessionCapabilities.list) {
    phase = 'session/list';
    progress('session/list');
    try {
      listResult = await request('session/list', {}, scaled(30_000));
    } catch (error) {
      listResult = { error: error.message, code: error.code ?? null };
    }
  }

  phase = 'stop turn';
  progress('stop turn');
  const baselineToolCalls = toolCalls().length;
  const stopRequest = promptTurn(
    sessionId,
    'Use your shell/terminal tool to sleep for 30 seconds, then reply DONE.',
    scaled(180_000),
  );
  await waitFor(() => toolCalls().length > baselineToolCalls, scaled(60_000), 'stop target tool call');
  notify('session/cancel', { sessionId });
  const stopTurn = await stopRequest;

  phase = 'restore';
  progress('restart transport for history restore');
  const oldChild = child;
  await stopChild(oldChild);
  resetTransport();
  child = spawnChild();
  const restoreInitialize = await initializeSession();
  const restoreMethod = capabilities.loadSession ? 'session/load' : 'session/resume';
  phase = restoreMethod;
  progress(restoreMethod);
  const restoreStartedAt = events.length;
  const restore = await request(restoreMethod, { cwd, mcpServers: [], sessionId }, scaled(60_000));
  const restoreTurn = await promptTurn(
    sessionId,
    'What token did I ask you to remember earlier? Reply with that token only.',
    scaled(180_000),
  );
  const restoreTexts = finalTextsSince(restoreStartedAt);

  let forkProbe = null;
  if (sessionCapabilities.fork) {
    phase = 'fork checkpoint turn';
    progress('fork checkpoint turn');
    const checkpointTurn = await promptTurn(
      sessionId,
      'Create a file named acp-fork-checkpoint.txt containing exactly CHECKPOINT_OK, then reply exactly CHECKPOINT_OK.',
      scaled(180_000),
    );
    phase = 'session/fork';
    progress('session/fork');
    const forkResult = await request('session/fork', { sessionId }, scaled(60_000));
    const forkSessionId = forkResult.forkedSessionId ?? forkResult.sessionId;
    if (!forkSessionId) throw new Error('session/fork did not return sessionId');
    const forkStartedAt = events.length;
    const forkTurn = await promptTurn(
      forkSessionId,
      'What token did I ask you to remember earlier? Reply with that token only.',
      scaled(180_000),
    );
    forkProbe = {
      checkpointStopReason: checkpointTurn.stopReason ?? null,
      forkSessionId,
      stopReason: forkTurn.stopReason ?? null,
      rememberedTokenObserved: finalTextsSince(forkStartedAt).includes(rememberedToken),
    };
  }

  let continueProbe = null;
  if (sessionCapabilities.resume && capabilities.loadSession) {
    phase = 'session/resume';
    progress('restart transport for explicit resume');
    const loadedChild = child;
    await stopChild(loadedChild);
    resetTransport();
    child = spawnChild();
    const continueInitialize = await initializeSession();
    phase = 'session/resume';
    progress('session/resume');
    const continueStartedAt = events.length;
    const resumed = await request('session/resume', { cwd, mcpServers: [], sessionId }, scaled(60_000));
    const continueTurn = await promptTurn(
      sessionId,
      'What token did I ask you to remember at the start of this conversation? Reply with that token only.',
      scaled(180_000),
    );
    continueProbe = {
      initializeProtocolVersion: continueInitialize.protocolVersion ?? null,
      responseKeys: Object.keys(resumed ?? {}),
      stopReason: continueTurn.stopReason ?? null,
      rememberedTokenObserved: finalTextsSince(continueStartedAt).includes(rememberedToken),
    };
  }

  let closeResult = null;
  let deleteResult = null;
  let forcedProcessKill = false;
  if (sessionCapabilities.close) {
    phase = 'session/close';
    progress('session/close');
    closeResult = await request('session/close', { sessionId }, scaled(30_000));
  } else {
    progress('session/close is not declared; testing transport shutdown only');
  }
  if (sessionCapabilities.delete) {
    phase = 'session/delete';
    progress('session/delete');
    try {
      deleteResult = await request('session/delete', { sessionId }, scaled(30_000));
    } catch (error) {
      deleteResult = { error: error.message, code: error.code ?? null };
    }
  }
  child.stdin.end();
  const exited = await waitForExit(child, scaled(20_000));
  if (!exited) {
    forcedProcessKill = true;
    forceProcessTree(child);
  }

  const texts = finalTextsSince(0);
  console.log(JSON.stringify({
    mode: 'full',
    cwd,
    command,
    initialize: {
      protocolVersion: initializeResult.protocolVersion ?? null,
      agentCapabilities: capabilities,
      authMethods: authMethods.map(scrubAuthMethod),
    },
    auth: authState,
    session: {
      sessionId,
      newResponseKeys: Object.keys(newSession),
      toolPromptStopReason: toolTurn.stopReason ?? null,
      toolCallCount: toolCalls().length,
      toolCalls: toolCalls().map(call => summarize({
        id: call.toolCallId,
        title: call.title,
        status: call.status,
        kind: call.kind,
      })),
      permissionCount: permissions.length,
      permissions,
      listResult,
      stopStopReason: stopTurn.stopReason ?? null,
      stopObserved: ['cancelled', 'canceled', 'interrupted'].includes(String(stopTurn.stopReason ?? '').toLowerCase()),
      restoreInitializeProtocolVersion: restoreInitialize.protocolVersion ?? null,
      restoreMethod,
      restoreResponseKeys: Object.keys(restore ?? {}),
      restoreStopReason: restoreTurn.stopReason ?? null,
      restoreTexts,
      rememberedTokenObserved: restoreTexts.includes(rememberedToken),
      toolMarkerObserved: texts.includes(`TOOL_OK:${rememberedToken}`),
      forkSupported: Boolean(sessionCapabilities.fork),
      forkProbe,
      explicitContinueProbe: continueProbe,
      closeSupported: Boolean(sessionCapabilities.close),
      closeResult,
      deleteSupported: Boolean(sessionCapabilities.delete),
      deleteResult,
      forcedProcessKill,
    },
  }, null, 2));
  return 0;
}

process.on('exit', () => {
  if (child && child.exitCode === null) forceProcessTree(child);
});

try {
  child = spawnChild();
  const initializeResult = await initializeSession();
  await runFullLifecycle(initializeResult);
  process.exit(0);
} catch (error) {
  progress(`failed during ${phase}`);
  console.log(JSON.stringify({
    mode: skillNames.length > 0 ? 'skills' : 'full',
    cwd,
    command,
    failedPhase: phase,
    error: { message: error.message, code: error.code ?? null, missingEnv: error.missingEnv ?? null },
    auth: authState,
    authMethods: authMethods.map(scrubAuthMethod),
    notifications: events.filter(item => item.kind === 'notification').slice(-20).map(item => summarize(item.detail)),
    exit: events.find(item => item.kind === 'exit')?.detail ?? null,
    stderrTail,
  }, null, 2));
  if (child) forceProcessTree(child);
  process.exit(1);
}
