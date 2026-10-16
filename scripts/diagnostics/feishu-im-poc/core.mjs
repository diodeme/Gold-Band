// Pure PoC domain: scenarios, Card JSON 2.0 builders, callback interpretation,
// request convergence, redaction and report rendering. No I/O here.

export const CHECK_STATUS = Object.freeze({
  pending: 'pending',
  pass: 'pass',
  fail: 'fail',
  observed: 'observed',
  skipped: 'skipped',
});

export const ACTION_SUBMIT = 'submit';
export const FORM_NAME = 'gb_form';
export const SUBMIT_NAME = 'gb_submit';

export const POC_ERROR = Object.freeze({
  unknownRequest: { code: 'poc.unknown_request', msg: 'request id not found in this run' },
  unboundActor: { code: 'poc.unbound_actor', msg: 'operator is not the bound user' },
  invalidAction: { code: 'poc.invalid_action', msg: 'action id is not offered by the card' },
  invalidAnswer: { code: 'poc.invalid_answer', msg: 'form answer is missing or not offered' },
  notInteractive: { code: 'poc.not_interactive', msg: 'scenario has no actions' },
  customAnswerMissing: { code: 'poc.custom_answer_missing', msg: 'custom option selected without custom text' },
  textTooLong: { code: 'poc.text_too_long', msg: 'text answer exceeds max length' },
});

/** Scenario suites; `run --suite <id>` sends one suite. */
export const SUITE = Object.freeze({ approval: 'approval', custom: 'custom' });
export const ACTION_DECLINE = 'decline';
const OTHER = 'other';

const options = (pairs) => pairs.map(([id, label]) => ({ id, label }));
const PERMISSION_OPTIONS = options([
  ['allow_once', '允许一次'],
  ['allow_always', '始终允许'],
  ['reject', '拒绝'],
]);
const PERMISSION_OPTIONS_20 = Array.from({ length: 20 }, (_, i) => ({
  id: `opt_${String(i + 1).padStart(2, '0')}`,
  label: `动作 ${i + 1}`,
}));

/**
 * One scenario per approval capability in docs §5/§6.1. `kind` mirrors the
 * product request kinds; `layout` is the Feishu rendering under test.
 */
export const SCENARIOS = Object.freeze([
  {
    id: 'permission-buttons', kind: 'permission', layout: 'buttons', displayRef: '1001',
    title: 'Permission：按钮单选',
    detail: 'Agent 请求执行命令：`git push origin main`',
    options: PERMISSION_OPTIONS,
    manual: ['点击任一按钮后卡片原地变为终态', '再次点击无按钮可点（已收敛）'],
  },
  {
    id: 'permission-select', kind: 'permission', layout: 'select', displayRef: '1002',
    title: 'Permission：20 个选项下拉单选',
    detail: 'Agent 请求权限，选项数达到上限 20',
    options: PERMISSION_OPTIONS_20,
    manual: ['下拉可展示全部 20 项', '提交后卡片变为终态'],
  },
  {
    id: 'manual-check', kind: 'manual_check', layout: 'buttons', displayRef: '1003',
    title: 'ManualCheck：人工检查',
    detail: '请确认 Workflow 产物是否符合预期',
    options: options([['fail', '失败'], ['success', '成功']]),
    manual: ['“失败”在前、“成功”在后', '点击后卡片变为终态'],
  },
  {
    id: 'elicitation-single', kind: 'elicitation', layout: 'form', displayRef: '1004',
    title: 'Elicitation：单选',
    detail: 'Agent 需要你选择部署环境',
    questions: [{ key: 'q0', label: '部署环境', multi: false, allowEmpty: false,
      options: options([['dev', '开发'], ['staging', '预发'], ['prod', '生产']]) }],
    manual: ['未选择时提交被客户端拦截（必填）', '提交后卡片变为终态'],
  },
  {
    id: 'elicitation-multi', kind: 'elicitation', layout: 'form', displayRef: '1005',
    title: 'Elicitation：多选（允许空选）',
    detail: 'Agent 需要你选择要运行的检查项，可不选',
    questions: [{ key: 'q0', label: '检查项', multi: true, allowEmpty: true,
      options: options([['lint', 'Lint'], ['unit', '单元测试'], ['e2e', 'E2E 测试'], ['bench', '基准测试']]) }],
    manual: ['可多选', '不选直接提交也能成功（显式空选）'],
  },
  {
    id: 'elicitation-questions', kind: 'elicitation', layout: 'form', displayRef: '1006',
    title: 'Elicitation：3 个单选问题',
    detail: 'Agent 需要你回答 3 个问题',
    questions: [
      { key: 'q0', label: '目标分支', multi: false, allowEmpty: false, options: options([['main', 'main'], ['release', 'release']]) },
      { key: 'q1', label: '是否保留 worktree', multi: false, allowEmpty: false, options: options([['keep', '保留'], ['drop', '删除']]) },
      { key: 'q2', label: '失败重试次数', multi: false, allowEmpty: false, options: options([['0', '0'], ['1', '1'], ['3', '3']]) },
    ],
    manual: ['三个问题必须都选择才能提交', '提交后卡片列出三个答案'],
  },
  {
    id: 'desktop-resolve', kind: 'permission', layout: 'buttons', displayRef: '1007',
    title: '桌面端处理后同步终态',
    detail: '不要点击：约 8 秒后模拟桌面端处理，卡片应自动变为终态',
    options: PERMISSION_OPTIONS,
    autoResolveAfterMs: 8000,
    manual: ['不点击的情况下卡片约 8 秒后自动变为终态'],
  },
  {
    id: 'already-handled', kind: 'permission', layout: 'buttons', displayRef: '1008',
    title: '已处理收敛',
    detail: '该请求已在其他端处理但卡片尚未更新；点击任一按钮应收到“已处理”提示并收敛为终态',
    options: PERMISSION_OPTIONS,
    preResolved: { actionId: 'allow_once', by: 'desktop' },
    manual: ['点击后提示已处理，卡片显示桌面端处理结果“允许一次”'],
  },
  {
    id: 'slow-callback', kind: 'permission', layout: 'buttons', displayRef: '1009',
    title: '回调超时探测',
    detail: '点击后服务端故意延迟 5 秒响应，观察客户端表现及是否重投；随后异步更新卡片',
    options: PERMISSION_OPTIONS,
    callbackDelayMs: 5000,
    manual: ['点击后客户端表现（报错/转圈）', '约 6 秒后卡片是否经异步 PATCH 变为终态'],
  },
  {
    id: 'notify-run-success', kind: 'notification', variant: 'run_success', displayRef: '1010',
    title: 'Workflow 运行成功', detail: 'Workflow「发布流水线」运行成功，耗时 3m12s', manual: ['只读，无按钮'],
  },
  {
    id: 'notify-run-failure', kind: 'notification', variant: 'run_failure', displayRef: '1011',
    title: 'Workflow 运行失败', detail: 'Workflow「发布流水线」在步骤「单元测试」失败', manual: ['只读，无按钮'],
  },
  {
    id: 'notify-acp-turn-finished', kind: 'notification', variant: 'acp_turn_finished', displayRef: '1012',
    title: 'Agent 本轮完成', detail: 'Claude 已完成本轮回复，等待你的下一条指令', manual: ['只读，无按钮'],
  },
  // ---- custom suite: “其他（自行填写）”、自由文本、混合题型、拒绝回答 ----
  {
    id: 'custom-single', suite: SUITE.custom, kind: 'elicitation', layout: 'form', displayRef: '2001',
    title: '单选 + 其他（自行填写）',
    detail: '选择“其他”时必须在输入框填写；输入框始终显示',
    questions: [{ key: 'q0', label: '部署环境', multi: false, allowEmpty: false,
      options: options([['dev', '开发'], ['staging', '预发'], [OTHER, '其他（自行填写）']]),
      custom: { key: 'q0_other', maxLength: 100 } }],
    manual: ['选“其他”但不填直接提交：应提示 poc.custom_answer_missing 且卡片仍可编辑', '再填写后提交：成功，终态显示填写内容', '（另发一次“重发”）选固定项同时填写输入框：观察输入值是否回传'],
  },
  {
    id: 'custom-multi', suite: SUITE.custom, kind: 'elicitation', layout: 'form', displayRef: '2002',
    title: '多选 + 其他（自行填写）',
    detail: '可多选；勾选“其他”时必须填写',
    questions: [{ key: 'q0', label: '检查项', multi: true, allowEmpty: true,
      options: options([['lint', 'Lint'], ['unit', '单元测试'], [OTHER, '其他（自行填写）']]),
      custom: { key: 'q0_other', maxLength: 100 } }],
    manual: ['勾选“其他”+固定项并填写后提交：终态同时显示固定项与填写内容'],
  },
  {
    id: 'free-text', suite: SUITE.custom, kind: 'elicitation', layout: 'form', displayRef: '2003',
    title: '自由文本（必填单行 + 选填多行）',
    detail: '单行必填、最多 20 字；多行选填',
    questions: [
      { key: 'q0', label: '提交说明（必填，≤20 字）', input: { required: true, maxLength: 20, multiline: false } },
      { key: 'q1', label: '补充备注（选填，多行）', input: { required: false, maxLength: 500, multiline: true } },
    ],
    manual: ['单行不填提交被客户端拦截', '单行能否输入超过 20 字', '多行可换行；不填时观察回传是缺失还是空串'],
  },
  {
    id: 'mixed-5', suite: SUITE.custom, kind: 'elicitation', layout: 'form', displayRef: '2004',
    title: '混合 5 题（单选/多选/文本/其他）',
    detail: '验证混合题型与超过 3 题的容量',
    questions: [
      { key: 'q0', label: '目标分支', multi: false, allowEmpty: false, options: options([['main', 'main'], ['release', 'release']]) },
      { key: 'q1', label: '运行检查', multi: true, allowEmpty: true, options: options([['lint', 'Lint'], ['unit', '单元测试']]) },
      { key: 'q2', label: '提交说明', input: { required: true, maxLength: 200, multiline: false } },
      { key: 'q3', label: '重试策略', multi: false, allowEmpty: false,
        options: options([['none', '不重试'], [OTHER, '其他（自行填写）']]), custom: { key: 'q3_other', maxLength: 100 } },
      { key: 'q4', label: '通知方式', multi: true, allowEmpty: false, options: options([['im', 'IM'], ['mail', '邮件']]) },
    ],
    manual: ['5 题均正常显示', '一次提交得到全部答案'],
  },
  {
    id: 'decline', suite: SUITE.custom, kind: 'elicitation', layout: 'form', displayRef: '2005',
    title: '表单外“拒绝回答”',
    detail: '不填写表单，直接点“拒绝回答”',
    declinable: true,
    questions: [{ key: 'q0', label: '部署环境', multi: false, allowEmpty: false,
      options: options([['dev', '开发'], ['prod', '生产']]) }],
    manual: ['必填项为空时“拒绝回答”仍可点击并变为终态'],
  },
].map((s) => ({ suite: SUITE.approval, ...s })));

export function suiteScenarios(suite) {
  return SCENARIOS.filter((s) => s.suite === suite);
}

export const INBOUND_CHECKS = Object.freeze([
  { id: 'inbound-p2p', title: '单聊消息事件（im.message.receive_v1）', manual: ['在与机器人的单聊中发送任意文本'] },
  { id: 'inbound-group', title: '群聊消息拒绝（可选）', manual: ['把机器人拉进群并 @ 它；预期收不到或被拒绝'] },
  { id: 'redelivery', title: '同一 event_id 重投幂等', manual: ['由回调超时探测触发，自动记录'] },
]);

export const REGISTER_CHECKS = Object.freeze([
  { id: 'register-scan', title: '扫码创建应用（registerApp）' },
  { id: 'register-token', title: '获取 tenant_access_token' },
  { id: 'register-bot-info', title: '机器人能力已开启（bot/v3/info）' },
  { id: 'register-ws-endpoint', title: '长连接 endpoint 可用' },
  { id: 'register-scopes', title: '权限授予状态（application/v6/scopes）' },
]);

const NOTIFICATION_TEMPLATE = { run_success: 'green', run_failure: 'red', acp_turn_finished: 'blue' };
const TERMINAL_TEMPLATE = 'grey';

export function findScenario(id) {
  return SCENARIOS.find((s) => s.id === id);
}

export function isInteractive(scenario) {
  return scenario.kind !== 'notification';
}

const plain = (content) => ({ tag: 'plain_text', content });

function cardShell({ title, template, elements }) {
  return {
    schema: '2.0',
    config: { update_multi: true },
    header: { title: plain(title), template },
    body: { elements },
  };
}

function detailMarkdown(scenario) {
  return { tag: 'markdown', content: `**id=${scenario.displayRef}**\n${scenario.detail}` };
}

function callbackValue(scenario, requestId, act) {
  return { poc: scenario.id, rid: requestId, act };
}

function button(scenario, requestId, option, index) {
  return {
    tag: 'button',
    text: plain(option.label),
    type: option.id === 'reject' || option.id === 'fail' ? 'danger' : index === 0 ? 'primary' : 'default',
    behaviors: [{ type: 'callback', value: callbackValue(scenario, requestId, option.id) }],
  };
}

function selectElement(question) {
  return {
    tag: question.multi ? 'multi_select_static' : 'select_static',
    name: question.key,
    placeholder: plain(question.multi ? '可多选' : '请选择'),
    required: !question.allowEmpty,
    width: 'fill',
    options: question.options.map((o) => ({ text: plain(o.label), value: o.id })),
  };
}

function inputElement(key, { required, maxLength, multiline }, placeholder) {
  return {
    tag: 'input',
    name: key,
    required,
    max_length: maxLength,
    placeholder: plain(placeholder),
    width: 'fill',
    ...(multiline ? { input_type: 'multiline_text', rows: 3, auto_resize: true } : {}),
  };
}

function questionElements(q) {
  const elements = [{ tag: 'markdown', content: `**${q.label}**` }];
  if (q.input) {
    elements.push(inputElement(q.key, q.input, q.input.required ? '请输入' : '选填'));
    return elements;
  }
  elements.push(selectElement(q));
  if (q.custom) {
    elements.push(inputElement(q.custom.key, { required: false, maxLength: q.custom.maxLength, multiline: false }, '选择“其他”时填写'));
  }
  return elements;
}

/** Questions rendered by a form; Permission with a select layout is one single-choice question. */
export function formQuestions(scenario) {
  if (scenario.layout === 'select') {
    return [{ key: 'q0', label: '选择动作', multi: false, allowEmpty: false, options: scenario.options }];
  }
  return scenario.questions ?? [];
}

export function buildPendingCard(scenario, requestId) {
  if (!isInteractive(scenario)) {
    return cardShell({
      title: scenario.title,
      template: NOTIFICATION_TEMPLATE[scenario.variant],
      elements: [detailMarkdown(scenario)],
    });
  }
  const elements = [detailMarkdown(scenario)];
  if (scenario.layout === 'buttons') {
    scenario.options.forEach((o, i) => elements.push(button(scenario, requestId, o, i)));
  } else {
    const formElements = [];
    for (const q of formQuestions(scenario)) formElements.push(...questionElements(q));
    formElements.push({
      tag: 'button',
      name: SUBMIT_NAME,
      text: plain('提交'),
      type: 'primary',
      form_action_type: 'submit',
      behaviors: [{ type: 'callback', value: callbackValue(scenario, requestId, ACTION_SUBMIT) }],
    });
    elements.push({ tag: 'form', name: FORM_NAME, elements: formElements });
    if (scenario.declinable) {
      elements.push({
        tag: 'button',
        text: plain('拒绝回答'),
        type: 'danger',
        behaviors: [{ type: 'callback', value: callbackValue(scenario, requestId, ACTION_DECLINE) }],
      });
    }
  }
  return cardShell({ title: scenario.title, template: 'blue', elements });
}

const RESOLVER_LABEL = { im: '飞书', desktop: '桌面端' };

export function resolutionText(scenario, resolution) {
  if (resolution.declined) return '拒绝回答';
  if (resolution.actionId) {
    return scenario.options.find((o) => o.id === resolution.actionId)?.label ?? resolution.actionId;
  }
  return formQuestions(scenario)
    .map((q) => {
      const value = resolution.answers[q.key];
      if (q.input) return `${q.label}=${value ? value : '（空）'}`;
      const ids = [value].flat().filter((v) => v !== undefined);
      const labels = ids.map((id) => (id === OTHER && q.custom
        ? `其他：${resolution.answers[q.custom.key]}`
        : q.options.find((o) => o.id === id)?.label ?? id));
      return `${q.label}=${labels.length ? labels.join('、') : '（空）'}`;
    })
    .join('；');
}

export function buildTerminalCard(scenario, resolution, resolvedBy) {
  return cardShell({
    title: scenario.title,
    template: TERMINAL_TEMPLATE,
    elements: [
      { tag: 'markdown', content: `id=${scenario.displayRef} 已处理：${resolutionText(scenario, resolution)}` },
      { tag: 'markdown', content: `${scenario.detail}\n处理端：${RESOLVER_LABEL[resolvedBy] ?? resolvedBy}` },
    ],
  });
}

const asObject = (v) => {
  if (v && typeof v === 'object') return v;
  if (typeof v === 'string') {
    try { return JSON.parse(v); } catch { return {}; }
  }
  return {};
};

/** Extract the PoC identity carried in a card.action.trigger callback (parsed by the SDK). */
export function readCallbackIdentity(event) {
  const value = asObject(event?.action?.value);
  return {
    scenarioId: value.poc,
    requestId: value.rid,
    act: value.act,
    operatorOpenId: event?.operator?.open_id,
    messageId: event?.context?.open_message_id,
    chatId: event?.context?.open_chat_id,
    eventId: event?.event_id,
  };
}

/**
 * Map a callback to a product resolution. Returns `{ resolution }` or `{ error }`
 * with a structured error from POC_ERROR.
 */
export function interpretCardAction(scenario, event) {
  if (!isInteractive(scenario)) return { error: POC_ERROR.notInteractive };
  const { act } = readCallbackIdentity(event);
  if (act === ACTION_DECLINE) {
    return scenario.declinable ? { resolution: { declined: true } } : { error: POC_ERROR.invalidAction };
  }
  if (act !== ACTION_SUBMIT) {
    if (scenario.layout !== 'buttons' || !scenario.options.some((o) => o.id === act)) {
      return { error: POC_ERROR.invalidAction };
    }
    return { resolution: { actionId: act } };
  }
  const formValue = asObject(event?.action?.form_value);
  const answers = {};
  for (const q of formQuestions(scenario)) {
    const raw = formValue[q.key];
    if (q.input) {
      const text = typeof raw === 'string' ? raw.trim() : '';
      if (q.input.required && !text) return { error: POC_ERROR.invalidAnswer };
      if (text.length > q.input.maxLength) return { error: POC_ERROR.textTooLong };
      answers[q.key] = text;
      continue;
    }
    const ids = q.multi ? [raw ?? []].flat() : raw;
    const offered = (id) => q.options.some((o) => o.id === id);
    if (q.multi) {
      if (!ids.every(offered) || (!q.allowEmpty && ids.length === 0)) return { error: POC_ERROR.invalidAnswer };
    } else if (typeof ids !== 'string' || !offered(ids)) {
      return { error: POC_ERROR.invalidAnswer };
    }
    answers[q.key] = ids;
    if (q.custom && [ids].flat().includes(OTHER)) {
      const custom = formValue[q.custom.key];
      const text = typeof custom === 'string' ? custom.trim() : '';
      if (!text) return { error: POC_ERROR.customAnswerMissing };
      if (text.length > q.custom.maxLength) return { error: POC_ERROR.textTooLong };
      answers[q.custom.key] = text;
    }
  }
  if (scenario.layout === 'select') return { resolution: { actionId: answers.q0 } };
  return { resolution: { answers } };
}

/** First resolution wins; later attempts converge to the stored terminal state. */
export class RequestRegistry {
  #requests = new Map();

  open(requestId, scenarioId) {
    const record = { requestId, scenarioId, status: 'pending', resolution: null, resolvedBy: null, messageId: null, attempts: 0 };
    this.#requests.set(requestId, record);
    return record;
  }

  get(requestId) {
    return this.#requests.get(requestId);
  }

  resolve(requestId, resolution, resolvedBy) {
    const record = this.#requests.get(requestId);
    if (!record) return { error: POC_ERROR.unknownRequest };
    record.attempts += 1;
    if (record.status === 'resolved') return { outcome: 'already_resolved', record };
    Object.assign(record, { status: 'resolved', resolution, resolvedBy });
    return { outcome: 'resolved', record };
  }
}

/** Bounded FIFO set used for event_id idempotency. */
export class EventDeduper {
  #seen = new Set();
  constructor(capacity = 1000) {
    this.capacity = capacity;
  }

  /** Returns true when the id is new. */
  accept(eventId) {
    if (!eventId) return true;
    if (this.#seen.has(eventId)) return false;
    this.#seen.add(eventId);
    if (this.#seen.size > this.capacity) this.#seen.delete(this.#seen.values().next().value);
    return true;
  }
}

const SECRET_KEY = /secret|token|ticket|encrypt|password|authorization/i;
const ID_KEY = /^(open_id|union_id|user_id|tenant_key|receive_id|openId|operatorOpenId)$/;

export function maskId(value) {
  if (typeof value !== 'string' || value.length <= 8) return '***';
  return `${value.slice(0, 6)}…${value.slice(-2)}`;
}

/** Deep copy with secrets removed and user identifiers masked. */
export function redact(value, key = '') {
  if (Array.isArray(value)) return value.map((v) => redact(v));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, redact(v, k)]));
  }
  if (typeof value === 'string' && SECRET_KEY.test(key)) return '***';
  if (typeof value === 'string' && ID_KEY.test(key)) return maskId(value);
  if (typeof value === 'string' && /^wss?:\/\//.test(value)) return value.replace(/\?.*$/, '?***');
  return value;
}

export function emptyResults() {
  const checks = {};
  for (const c of REGISTER_CHECKS) checks[c.id] = { title: c.title, status: CHECK_STATUS.pending, observations: [] };
  for (const s of SCENARIOS) checks[s.id] = { title: s.title, status: CHECK_STATUS.pending, manual: s.manual, observations: [] };
  for (const c of INBOUND_CHECKS) checks[c.id] = { title: c.title, status: CHECK_STATUS.pending, manual: c.manual, observations: [] };
  return { meta: {}, checks };
}

const STATUS_MARK = { pass: '✅ 通过', fail: '❌ 失败', observed: '🔎 已观测', skipped: '⏭️ 跳过', pending: '⏳ 未完成' };

export function renderReport(results) {
  const lines = ['# 飞书 IM PoC 报告', ''];
  for (const [k, v] of Object.entries(results.meta ?? {})) lines.push(`- ${k}: ${v}`);
  lines.push('', '| 检查项 | 结果 | 结论 | 需人工确认 |', '| --- | --- | --- | --- |');
  for (const [id, c] of Object.entries(results.checks)) {
    const last = c.observations.at(-1);
    const summary = (c.summary ?? last?.note ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ');
    lines.push(`| ${c.title}（${id}） | ${STATUS_MARK[c.status]} | ${summary} | ${(c.manual ?? []).join('；')} |`);
  }
  lines.push('', '## 观测明细', '');
  for (const [id, c] of Object.entries(results.checks)) {
    if (!c.observations.length) continue;
    lines.push(`### ${id}`, '', '```json', JSON.stringify(c.observations, null, 2), '```', '');
  }
  return lines.join('\n');
}
