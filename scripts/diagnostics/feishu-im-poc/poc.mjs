// Feishu IM PoC CLI: `register` (scan-to-create), `run` (approval scenarios over
// the long connection), `report` (markdown matrix). State lives in .poc-state/.
import http from 'node:http';
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import * as lark from '@larksuiteoapi/node-sdk';
import QRCode from 'qrcode';
import {
  CHECK_STATUS, INBOUND_CHECKS, POC_ERROR, REGISTER_CHECKS, SUITE, buildPendingCard, buildTerminalCard, suiteScenarios,
  EventDeduper, RequestRegistry, emptyResults, findScenario, interpretCardAction, isInteractive,
  maskId, readCallbackIdentity, redact, renderReport, resolutionText,
} from './core.mjs';

const STATE_DIR = new URL('./.poc-state/', import.meta.url);
const FIXTURE_DIR = new URL('./fixtures/', STATE_DIR);
const FILES = {
  credentials: new URL('credentials.json', STATE_DIR),
  results: new URL('results.json', STATE_DIR),
  report: new URL('report.md', STATE_DIR),
  qr: new URL('register-qr.png', STATE_DIR),
};
const OPEN_DOMAIN = { feishu: 'https://open.feishu.cn', lark: 'https://open.larksuite.com' };
const PORT = Number(process.env.FEISHU_POC_PORT ?? 47831);
const DEFAULT_RUN_MINUTES = 20;
const SEND_GAP_MS = 250;
const ASYNC_PATCH_DELAY_MS = 1000;

/** Least-privilege config pre-filled on the confirm page (verifies addons + preset:false). */
const ADDONS = {
  preset: false,
  scopes: { tenant: ['im:message:send_as_bot', 'im:message.p2p_msg:readonly', 'im:message:update'] },
  events: { items: { tenant: ['im.message.receive_v1'] } },
  callbacks: { items: ['card.action.trigger'] },
};

/** Feishu headers carry create_time as ms or µs strings depending on the event family. */
const epochMs = (t) => { const n = Number(t); return n > 1e14 ? Math.floor(n / 1000) : n; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => new Date().toISOString();
const log = (...args) => console.log(`[${now().slice(11, 19)}]`, ...args);

function ensureDirs() {
  mkdirSync(FIXTURE_DIR, { recursive: true });
}

function writeJsonAtomic(url, value) {
  const tmp = new URL(`${url.pathname.split('/').pop()}.tmp`, url);
  writeFileSync(tmp, JSON.stringify(value, null, 2), { mode: 0o600 });
  renameSync(tmp, url);
}

const readJson = (url) => (existsSync(url) ? JSON.parse(readFileSync(url, 'utf8')) : null);

function loadCredentials() {
  const stored = readJson(FILES.credentials) ?? {};
  const creds = {
    appId: process.env.FEISHU_POC_APP_ID ?? stored.appId,
    appSecret: process.env.FEISHU_POC_APP_SECRET ?? stored.appSecret,
    openId: process.env.FEISHU_POC_OPEN_ID ?? stored.openId,
    tenantBrand: process.env.FEISHU_POC_BRAND ?? stored.tenantBrand ?? 'feishu',
  };
  if (!creds.appId || !creds.appSecret || !creds.openId) {
    throw new Error('missing credentials: run `npm run register` first');
  }
  return creds;
}

// ---------- results ----------
let results = readJson(FILES.results) ?? emptyResults();

function persistResults() {
  writeJsonAtomic(FILES.results, results);
}

function record(id, status, note, data) {
  const check = results.checks[id];
  if (status) check.status = status;
  if (note) check.summary = note;
  check.observations.push({ at: now(), note, ...(data === undefined ? {} : { data: redact(data) }) });
  persistResults();
  log(`${id}: ${status ?? '-'} ${note ?? ''}`);
}

function resetChecks(ids) {
  const fresh = emptyResults();
  for (const id of ids) results.checks[id] = fresh.checks[id];
}

// Run-scoped prefix keeps fixtures from earlier runs instead of overwriting them.
const FIXTURE_RUN_TAG = new Date().toISOString().replace(/[-:]/g, '').slice(0, 15);
let fixtureSeq = 0;
function saveFixture(type, payload) {
  fixtureSeq += 1;
  const name = `${FIXTURE_RUN_TAG}-${String(fixtureSeq).padStart(3, '0')}-${type.replace(/[^\w.-]/g, '_')}.json`;
  writeFileSync(new URL(name, FIXTURE_DIR), JSON.stringify(redact(payload), null, 2));
}

// ---------- dashboard ----------
const escapeHtml = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

function checksTable() {
  const rows = Object.entries(results.checks).map(([id, c]) =>
    `<tr class="${c.status}"><td>${escapeHtml(c.title)}<br><small>${id}</small></td><td>${c.status}</td>` +
    `<td>${escapeHtml(c.summary)}</td><td>${escapeHtml((c.manual ?? []).join('；'))}</td></tr>`).join('');
  return `<table><tr><th>检查项</th><th>状态</th><th>结论</th><th>人工确认</th></tr>${rows}</table>`;
}

function startDashboard(renderBody) {
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(`<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="2"><title>Feishu PoC</title>
<style>body{font:14px system-ui;margin:24px;background:#111;color:#ddd}table{border-collapse:collapse;width:100%}
td,th{border-bottom:1px solid #333;padding:6px;text-align:left;vertical-align:top}small{color:#888}
.pass td:nth-child(2){color:#4ade80}.fail td:nth-child(2){color:#f87171}.observed td:nth-child(2){color:#60a5fa}
img{background:#fff;padding:12px}</style>${renderBody()}`);
  });
  server.listen(PORT, '127.0.0.1', () => log(`dashboard: http://127.0.0.1:${PORT}/`));
  return server;
}

// ---------- HTTP helpers ----------
async function postJson(url, body, headers = {}) {
  const res = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', ...headers }, body: JSON.stringify(body) });
  return { httpStatus: res.status, ...(await res.json().catch(() => ({}))) };
}

async function getJson(url, headers) {
  const res = await fetch(url, { headers });
  return { httpStatus: res.status, ...(await res.json().catch(() => ({}))) };
}

async function tenantToken(domain, creds) {
  return postJson(`${domain}/open-apis/auth/v3/tenant_access_token/internal`, { app_id: creds.appId, app_secret: creds.appSecret });
}

/** SDK calls throw on non-2xx; normalize both paths into { code, msg, data }. */
async function callApi(fn) {
  try {
    const r = await fn();
    return { code: r?.code ?? 0, msg: r?.msg, data: r?.data };
  } catch (e) {
    const body = e?.response?.data;
    return { code: body?.code ?? -1, msg: body?.msg ?? e?.message, httpStatus: e?.response?.status, data: body?.error };
  }
}

// ---------- register ----------
async function probeApp(creds) {
  const domain = OPEN_DOMAIN[creds.tenantBrand];
  const token = await tenantToken(domain, creds);
  record('register-token', token.code === 0 ? CHECK_STATUS.pass : CHECK_STATUS.fail, `code=${token.code} ${token.msg ?? ''}`, { code: token.code, msg: token.msg, expire: token.expire });
  if (token.code !== 0) return;
  const auth = { authorization: `Bearer ${token.tenant_access_token}` };

  const bot = await getJson(`${domain}/open-apis/bot/v3/info`, auth);
  record('register-bot-info', bot.code === 0 ? CHECK_STATUS.pass : CHECK_STATUS.fail,
    `code=${bot.code} activate_status=${bot.bot?.activate_status} name=${bot.bot?.app_name ?? ''}`, bot);

  const ws = await postJson(`${domain}/callback/ws/endpoint`, { AppID: creds.appId, AppSecret: creds.appSecret }, { locale: 'zh' });
  record('register-ws-endpoint', ws.code === 0 && ws.data?.URL ? CHECK_STATUS.pass : CHECK_STATUS.fail,
    `code=${ws.code} ${ws.msg ?? ''} ClientConfig=${JSON.stringify(ws.data?.ClientConfig ?? {})}`, ws);

  const scopes = await getJson(`${domain}/open-apis/application/v6/scopes`, auth);
  const granted = (scopes.data?.scopes ?? []).map((s) => `${s.scope_name}:${s.grant_status}`).join(', ');
  record('register-scopes', CHECK_STATUS.observed, `code=${scopes.code} ${scopes.msg ?? ''} ${granted}`, scopes);
}

async function register() {
  ensureDirs();
  resetChecks(REGISTER_CHECKS.map((c) => c.id));
  const view = { qrDataUrl: null, url: null, expiresAt: null, status: '准备中' };
  const server = startDashboard(() => (view.qrDataUrl
    ? `<h2>用飞书 App 扫码创建 Gold Band PoC 机器人</h2><p>状态：${escapeHtml(view.status)}；过期时间：${view.expiresAt}</p>
<img src="${view.qrDataUrl}" width="280"><p><small>${escapeHtml(view.url)}</small></p>${checksTable()}`
    : `<p>${escapeHtml(view.status)}</p>${checksTable()}`));
  const started = Date.now();
  try {
    const result = await lark.registerApp({
      source: 'gold-band-poc',
      createOnly: true,
      appPreset: { name: 'Gold Band PoC', desc: 'Gold Band 远程干预 PoC' },
      addons: ADDONS,
      onQRCodeReady: ({ url, expireIn }) => {
        view.url = url;
        view.expiresAt = new Date(Date.now() + expireIn * 1000).toLocaleTimeString();
        view.status = '等待扫码';
        QRCode.toDataURL(url, { margin: 1, width: 280 }).then((d) => { view.qrDataUrl = d; });
        QRCode.toFile(fileURLToPath(FILES.qr), url, { width: 280 });
        QRCode.toString(url, { type: 'terminal', small: true }).then((s) => console.log(s));
        log(`QR ready, expires in ${expireIn}s: ${url}`);
      },
      onStatusChange: (info) => { view.status = `${info.status}${info.interval ? ` interval=${info.interval}s` : ''}`; },
    });
    const creds = {
      appId: result.client_id,
      appSecret: result.client_secret,
      openId: result.user_info?.open_id,
      tenantBrand: result.user_info?.tenant_brand ?? 'feishu',
      createdAt: now(),
    };
    writeJsonAtomic(FILES.credentials, creds);
    results.meta = { appId: creds.appId, tenantBrand: creds.tenantBrand, openId: maskId(creds.openId), registeredAt: creds.createdAt };
    view.status = '创建成功，正在探测';
    record('register-scan', CHECK_STATUS.pass, `用时 ${Math.round((Date.now() - started) / 1000)}s，brand=${creds.tenantBrand}，open_id ${creds.openId ? '已返回' : '缺失'}`,
      { client_id: creds.appId, user_info: result.user_info, addons: ADDONS });
    await probeApp(creds);
    view.status = '完成';
  } catch (e) {
    record('register-scan', CHECK_STATUS.fail, `${e?.code ?? ''} ${e?.message ?? e}`);
  } finally {
    await sleep(3000);
    server.close();
  }
}

// ---------- run ----------
async function run({ minutes, suite }) {
  ensureDirs();
  const creds = loadCredentials();
  const domain = OPEN_DOMAIN[creds.tenantBrand];
  const scenarios = suiteScenarios(suite);
  if (!scenarios.length) throw new Error(`unknown suite: ${suite}`);
  resetChecks([...scenarios.map((s) => s.id), ...INBOUND_CHECKS.map((c) => c.id)]);
  results.meta = { ...results.meta, runStartedAt: now(), suite, sdk: '@larksuiteoapi/node-sdk 1.74.0' };
  persistResults();

  const client = new lark.Client({ appId: creds.appId, appSecret: creds.appSecret, domain, loggerLevel: lark.LoggerLevel.warn });
  const registry = new RequestRegistry();
  const deduper = new EventDeduper();
  const responses = new Map(); // event_id -> callback response, for converging redeliveries
  let sending = false;

  const send = (receiveId, msgType, content) => callApi(() => client.im.v1.message.create({
    params: { receive_id_type: 'open_id' },
    data: { receive_id: receiveId, msg_type: msgType, content: JSON.stringify(content) },
  }));
  const patchCard = (messageId, card) => callApi(() => client.im.v1.message.patch({
    path: { message_id: messageId }, data: { content: JSON.stringify(card) },
  }));
  const terminalResponse = (scenario, rec, toastType, toastText) => ({
    toast: { type: toastType, content: toastText },
    card: { type: 'raw', data: buildTerminalCard(scenario, rec.resolution, rec.resolvedBy) },
  });

  async function resolveFromDesktop(scenario, requestId) {
    const r = registry.resolve(requestId, { actionId: scenario.options[0].id }, 'desktop');
    if (r.outcome !== 'resolved') {
      record(scenario.id, null, '桌面端模拟处理前已在飞书处理，跳过 PATCH');
      return;
    }
    const t0 = Date.now();
    const patched = await patchCard(r.record.messageId, buildTerminalCard(scenario, r.record.resolution, 'desktop'));
    record(scenario.id, patched.code === 0 ? CHECK_STATUS.pass : CHECK_STATUS.fail,
      `异步 PATCH code=${patched.code} ${patched.msg ?? ''} 用时 ${Date.now() - t0}ms`, patched);
  }

  async function sendScenario(scenario, index) {
    const requestId = randomUUID().slice(0, 8);
    const rec = registry.open(requestId, scenario.id);
    if (scenario.preResolved) registry.resolve(requestId, { actionId: scenario.preResolved.actionId }, scenario.preResolved.by);
    const card = buildPendingCard(scenario, requestId);
    card.header.title.content = `[${index + 1}/${scenarios.length}] ${scenario.title}`;
    const t0 = Date.now();
    const sent = await send(creds.openId, 'interactive', card);
    rec.messageId = sent.data?.message_id;
    const ok = sent.code === 0;
    const status = !ok ? CHECK_STATUS.fail : isInteractive(scenario) ? null : CHECK_STATUS.pass;
    record(scenario.id, status, `发送 code=${sent.code} ${sent.msg ?? ''} 用时 ${Date.now() - t0}ms`,
      { code: sent.code, msg: sent.msg, message_id: rec.messageId, requestId, ...(ok ? {} : { error: sent.data }) });
    if (ok && scenario.autoResolveAfterMs) setTimeout(() => resolveFromDesktop(scenario, requestId), scenario.autoResolveAfterMs);
  }

  async function sendAll() {
    if (sending) return;
    sending = true;
    try {
      await send(creds.openId, 'text', { text: 'Gold Band PoC 已连接。请依次操作下面的卡片，并在本单聊回复任意文字验证消息接收；回复“重发”可重新下发全部卡片。' });
      for (const [i, s] of scenarios.entries()) {
        await sendScenario(s, i);
        await sleep(SEND_GAP_MS);
      }
    } finally {
      sending = false;
    }
  }

  async function onCardAction(event) {
    const receivedAt = Date.now();
    saveFixture('card.action.trigger', event);
    const id = readCallbackIdentity(event);
    const timing = { deliveryMs: event.create_time ? receivedAt - epochMs(event.create_time) : null };
    if (!deduper.accept(id.eventId)) {
      record('redelivery', CHECK_STATUS.observed, `收到重复 event_id，按幂等收敛（scenario=${id.scenarioId}）`, { eventId: id.eventId, ...timing });
      return responses.get(id.eventId) ?? { toast: { type: 'info', content: '处理中' } };
    }
    const scenario = findScenario(id.scenarioId);
    const rec = registry.get(id.requestId);
    if (!scenario || !rec) {
      if (scenario) record(scenario.id, null, '收到其他运行的旧卡片回调，已拒绝', { action: event.action });
      return { toast: { type: 'warning', content: 'PoC 会话已过期，请回复“重发”' } };
    }
    if (id.operatorOpenId !== creds.openId) {
      record(scenario.id, null, '非绑定用户点击，已拒绝', { operator: event.operator });
      return { toast: { type: 'error', content: '仅绑定用户可以处理' } };
    }
    if (scenario.callbackDelayMs) await sleep(scenario.callbackDelayMs);
    const parsed = interpretCardAction(scenario, event);
    const observed = { act: id.act, form_value: event.action?.form_value, tag: event.action?.tag, name: event.action?.name, ...timing };
    if (parsed.error) {
      // Server-side validation is an expected outcome for custom answers; the card stays editable.
      const expected = parsed.error === POC_ERROR.customAnswerMissing || parsed.error === POC_ERROR.textTooLong;
      record(scenario.id, expected ? CHECK_STATUS.observed : CHECK_STATUS.fail,
        `${expected ? '服务端校验拦截' : '回调解析失败'} ${parsed.error.code}（未更新卡片）`, observed);
      return { toast: { type: 'error', content: parsed.error.code } };
    }
    const r = registry.resolve(id.requestId, parsed.resolution, 'im');
    const handlerMs = Date.now() - receivedAt;
    let response;
    if (r.outcome === 'already_resolved') {
      response = terminalResponse(scenario, r.record, 'info', `id=${scenario.displayRef} 已处理`);
      const status = scenario.preResolved ? CHECK_STATUS.pass : null;
      record(scenario.id, status, `已处理收敛：${resolutionText(scenario, r.record.resolution)}（第 ${r.record.attempts} 次尝试）`, { ...observed, handlerMs });
    } else {
      response = terminalResponse(scenario, r.record, 'success', `已提交：${resolutionText(scenario, r.record.resolution)}`);
      const status = scenario.callbackDelayMs ? CHECK_STATUS.observed : scenario.autoResolveAfterMs ? null : CHECK_STATUS.pass;
      record(scenario.id, status, `回调 → ${resolutionText(scenario, r.record.resolution)}，处理 ${handlerMs}ms`, { ...observed, handlerMs });
      if (scenario.callbackDelayMs) {
        setTimeout(async () => {
          const patched = await patchCard(r.record.messageId, buildTerminalCard(scenario, r.record.resolution, 'im'));
          record(scenario.id, null, `超时后异步 PATCH code=${patched.code} ${patched.msg ?? ''}`, patched);
        }, ASYNC_PATCH_DELAY_MS);
      }
    }
    responses.set(id.eventId, response);
    return response;
  }

  async function onMessage(event) {
    saveFixture('im.message.receive_v1', event);
    if (!deduper.accept(event.event_id)) {
      record('redelivery', CHECK_STATUS.observed, '消息事件重复 event_id，已忽略', { eventId: event.event_id });
      return;
    }
    const { message, sender } = event;
    const senderOpenId = sender?.sender_id?.open_id;
    const text = (() => { try { return JSON.parse(message?.content ?? '{}').text ?? ''; } catch { return ''; } })();
    const info = { chat_type: message?.chat_type, message_type: message?.message_type, sender_type: sender?.sender_type, bound: senderOpenId === creds.openId };
    if (message?.chat_type !== 'p2p') {
      record('inbound-group', CHECK_STATUS.observed, `收到 ${message?.chat_type} 消息，已拒绝（不响应）`, info);
      return;
    }
    if (senderOpenId !== creds.openId) {
      record('inbound-p2p', null, '非绑定用户单聊，已拒绝', info);
      return;
    }
    record('inbound-p2p', CHECK_STATUS.pass, `收到绑定用户单聊：${text.slice(0, 20)}`, info);
    if (text.includes('重发')) {
      sendAll();
      return;
    }
    await send(creds.openId, 'text', { text: `已收到：${text.slice(0, 50)}` });
  }

  const dispatcher = new lark.EventDispatcher({ loggerLevel: lark.LoggerLevel.warn }).register({
    'im.message.receive_v1': onMessage,
    'card.action.trigger': onCardAction,
  });
  const wsClient = new lark.WSClient({
    appId: creds.appId,
    appSecret: creds.appSecret,
    domain,
    loggerLevel: lark.LoggerLevel.info,
    source: 'gold-band-poc',
    handshakeTimeoutMs: 15000,
    onReady: () => { log('long connection ready'); sendAll(); },
    onError: (err) => log('long connection failed', err?.message),
    onReconnecting: () => log('reconnecting'),
    onReconnected: () => log('reconnected'),
  });
  const server = startDashboard(() => `<h2>飞书审批 PoC 运行中</h2><p>app=${escapeHtml(creds.appId)} 截止 ${escapeHtml(results.meta.runDeadline)}</p>${checksTable()}`);

  let finished = false;
  const finish = () => {
    if (finished) return;
    finished = true;
    writeFileSync(FILES.report, renderReport(results));
    log(`report: ${fileURLToPath(FILES.report)}`);
    wsClient.close();
    server.close();
    setTimeout(() => process.exit(0), 200);
  };
  results.meta.runDeadline = new Date(Date.now() + minutes * 60_000).toLocaleTimeString();
  persistResults();
  process.on('SIGINT', finish);
  setTimeout(finish, minutes * 60_000);
  await wsClient.start({ eventDispatcher: dispatcher });
}

function report() {
  writeFileSync(FILES.report, renderReport(results));
  console.log(readFileSync(FILES.report, 'utf8'));
}

const { positionals, values } = parseArgs({
  allowPositionals: true,
  options: {
    minutes: { type: 'string', default: String(DEFAULT_RUN_MINUTES) },
    suite: { type: 'string', default: SUITE.approval },
  },
});
const command = positionals[0];
if (command === 'register') await register();
else if (command === 'run') await run({ minutes: Number(values.minutes), suite: values.suite });
else if (command === 'report') report();
else console.log('usage: node poc.mjs <register|run [--minutes N] [--suite approval|custom]|report>');
