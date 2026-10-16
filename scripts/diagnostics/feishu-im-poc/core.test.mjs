import test from 'node:test';
import assert from 'node:assert/strict';
import {
  ACTION_DECLINE, ACTION_SUBMIT, CHECK_STATUS, SUITE, suiteScenarios, EventDeduper, POC_ERROR, RequestRegistry, SCENARIOS, buildPendingCard,
  buildTerminalCard, emptyResults, findScenario, interpretCardAction, readCallbackIdentity, redact, renderReport,
} from './core.mjs';

const callback = (scenario, act, formValue, openId = 'ou_bound_user_0001') => ({
  event_id: 'ev_1',
  operator: { open_id: openId },
  context: { open_message_id: 'om_1', open_chat_id: 'oc_1' },
  action: { value: { poc: scenario.id, rid: 'r1', act }, ...(formValue ? { form_value: formValue } : {}) },
});

const walk = (node, visit) => {
  visit(node);
  for (const child of node.elements ?? node.body?.elements ?? []) walk(child, visit);
};

test('every scenario renders a Card JSON 2.0 shared card', () => {
  for (const s of SCENARIOS) {
    const card = buildPendingCard(s, 'r1');
    assert.equal(card.schema, '2.0');
    assert.equal(card.config.update_multi, true);
    const body = JSON.stringify(card);
    assert.ok(body.includes(`id=${s.displayRef}`), s.id);
  }
});

test('notifications are read-only; interactive cards carry poc/rid/act values', () => {
  for (const s of SCENARIOS) {
    const values = [];
    walk(buildPendingCard(s, 'r1'), (n) => n.behaviors?.forEach((b) => values.push(b.value)));
    if (s.kind === 'notification') assert.equal(values.length, 0, s.id);
    else {
      assert.ok(values.length > 0, s.id);
      for (const v of values) assert.deepEqual({ poc: v.poc, rid: v.rid }, { poc: s.id, rid: 'r1' });
    }
  }
});

test('manual check orders fail before success', () => {
  const ids = [];
  walk(buildPendingCard(findScenario('manual-check'), 'r1'), (n) => n.tag === 'button' && ids.push(n.behaviors[0].value.act));
  assert.deepEqual(ids, ['fail', 'success']);
});

test('20-option permission uses a required single select inside a form', () => {
  const selects = [];
  walk(buildPendingCard(findScenario('permission-select'), 'r1'), (n) => n.tag === 'select_static' && selects.push(n));
  assert.equal(selects.length, 1);
  assert.equal(selects[0].options.length, 20);
  assert.equal(selects[0].required, true);
});

test('button callback maps to an offered action and rejects others', () => {
  const s = findScenario('permission-buttons');
  assert.deepEqual(interpretCardAction(s, callback(s, 'allow_once')), { resolution: { actionId: 'allow_once' } });
  assert.deepEqual(interpretCardAction(s, callback(s, 'nope')), { error: POC_ERROR.invalidAction });
});

test('select permission submit maps q0 to an action id', () => {
  const s = findScenario('permission-select');
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'opt_20' })), { resolution: { actionId: 'opt_20' } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, {})), { error: POC_ERROR.invalidAnswer });
});

test('multi select accepts explicit empty selection and rejects unknown values', () => {
  const s = findScenario('elicitation-multi');
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, {})), { resolution: { answers: { q0: [] } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: ['lint', 'e2e'] })), { resolution: { answers: { q0: ['lint', 'e2e'] } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: ['x'] })), { error: POC_ERROR.invalidAnswer });
});

// Observed on Feishu (2026-10-12): an empty multi-select submit omits action.form_value entirely.
test('multi select treats a callback without form_value as explicit empty selection', () => {
  const s = findScenario('elicitation-multi');
  const observed = {
    schema: '2.0', event_type: 'card.action.trigger', event_id: 'ev_empty',
    operator: { open_id: 'ou_bound_user_0001' },
    action: { value: { act: ACTION_SUBMIT, poc: s.id, rid: 'r1' }, tag: 'button', timezone: 'Asia/Shanghai', name: 'gb_submit' },
    host: 'im_message', context: { open_message_id: 'om_1', open_chat_id: 'oc_1' },
  };
  assert.deepEqual(interpretCardAction(s, observed), { resolution: { answers: { q0: [] } } });
});

test('multi-question form requires q0..q2', () => {
  const s = findScenario('elicitation-questions');
  const ok = interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'main', q1: 'keep', q2: '3' }));
  assert.deepEqual(ok, { resolution: { answers: { q0: 'main', q1: 'keep', q2: '3' } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'main', q1: 'keep' })), { error: POC_ERROR.invalidAnswer });
});

test('callback value given as JSON string is accepted', () => {
  const s = findScenario('manual-check');
  const event = callback(s, 'success');
  event.action.value = JSON.stringify(event.action.value);
  assert.equal(readCallbackIdentity(event).act, 'success');
});

test('registry: first resolution wins and repeats converge', () => {
  const reg = new RequestRegistry();
  reg.open('r1', 'permission-buttons');
  assert.equal(reg.resolve('r1', { actionId: 'reject' }, 'desktop').outcome, 'resolved');
  const again = reg.resolve('r1', { actionId: 'allow_once' }, 'im');
  assert.equal(again.outcome, 'already_resolved');
  assert.deepEqual(again.record.resolution, { actionId: 'reject' });
  assert.equal(again.record.resolvedBy, 'desktop');
  assert.deepEqual(reg.resolve('missing', {}, 'im'), { error: POC_ERROR.unknownRequest });
});

test('terminal card shows id and resolution without interactive elements', () => {
  const s = findScenario('elicitation-questions');
  const card = buildTerminalCard(s, { answers: { q0: 'main', q1: 'drop', q2: '0' } }, 'im');
  const text = JSON.stringify(card);
  assert.ok(text.includes('id=1006 已处理：目标分支=main；是否保留 worktree=删除；失败重试次数=0'));
  assert.ok(!text.includes('behaviors'));
});

test('deduper accepts each event id once within capacity', () => {
  const d = new EventDeduper(2);
  assert.equal(d.accept('a'), true);
  assert.equal(d.accept('a'), false);
  d.accept('b');
  d.accept('c');
  assert.equal(d.accept('a'), true);
});

test('redact removes secrets, masks ids and strips ws query', () => {
  const out = redact({ app_secret: 's', token: 't', operator: { open_id: 'ou_1234567890' }, URL: 'wss://x/ws?ticket=abc' });
  assert.deepEqual(out, { app_secret: '***', token: '***', operator: { open_id: 'ou_123…90' }, URL: 'wss://x/ws?***' });
});

test('report lists every check', () => {
  const results = emptyResults();
  results.checks['manual-check'].status = CHECK_STATUS.pass;
  const md = renderReport(results);
  for (const id of Object.keys(results.checks)) assert.ok(md.includes(`（${id}）`), id);
});

test('suites partition scenarios; existing approval scenarios stay in the default suite', () => {
  const approval = suiteScenarios(SUITE.approval).map((s) => s.id);
  const custom = suiteScenarios(SUITE.custom).map((s) => s.id);
  assert.ok(approval.includes('permission-buttons'));
  assert.deepEqual(custom, ['custom-single', 'custom-multi', 'free-text', 'mixed-5', 'decline']);
  assert.equal(approval.length + custom.length, SCENARIOS.length);
});

test('custom option renders a companion input inside the form; free text renders input only', () => {
  const inputs = (id) => {
    const found = [];
    walk(buildPendingCard(findScenario(id), 'r1'), (n) => { if (n.tag === 'input') found.push(n); });
    return found;
  };
  assert.deepEqual(inputs('custom-single').map((n) => [n.name, n.required]), [['q0_other', false]]);
  const text = inputs('free-text');
  assert.deepEqual(text.map((n) => [n.name, n.required, n.max_length]), [['q0', true, 20], ['q1', false, 500]]);
  assert.equal(text[1].input_type, 'multiline_text');
});

test('selecting "other" requires custom text; text is returned beside the scalar choice', () => {
  const s = findScenario('custom-single');
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'other' })), { error: POC_ERROR.customAnswerMissing });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'other', q0_other: '   ' })), { error: POC_ERROR.customAnswerMissing });
  const ok = interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'other', q0_other: ' 灰度 ' }));
  assert.deepEqual(ok, { resolution: { answers: { q0: 'other', q0_other: '灰度' } } });
  assert.match(buildTerminalCard(s, ok.resolution, 'im').body.elements[0].content, /其他：灰度/);
  // Text typed without choosing "other" is ignored.
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'dev', q0_other: 'x' })), { resolution: { answers: { q0: 'dev' } } });
});

test('multi-select keeps fixed choices together with the custom answer', () => {
  const s = findScenario('custom-multi');
  const r = interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: ['lint', 'other'], q0_other: '安全扫描' }));
  assert.deepEqual(r, { resolution: { answers: { q0: ['lint', 'other'], q0_other: '安全扫描' } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, null)), { resolution: { answers: { q0: [] } } });
});

test('free text enforces required and max length; optional empty text is an empty string', () => {
  const s = findScenario('free-text');
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q1: 'x' })), { error: POC_ERROR.invalidAnswer });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'x'.repeat(21) })), { error: POC_ERROR.textTooLong });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: '修复', q1: '第一行\n第二行' })),
    { resolution: { answers: { q0: '修复', q1: '第一行\n第二行' } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: '修复' })), { resolution: { answers: { q0: '修复', q1: '' } } });
});

test('mixed form resolves all five questions in one submission', () => {
  const s = findScenario('mixed-5');
  const r = interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'main', q2: '发布', q3: 'other', q3_other: '重试 2 次', q4: ['im'] }));
  assert.deepEqual(r, { resolution: { answers: { q0: 'main', q1: [], q2: '发布', q3: 'other', q3_other: '重试 2 次', q4: ['im'] } } });
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_SUBMIT, { q0: 'main', q2: '发布', q3: 'none', q4: [] })), { error: POC_ERROR.invalidAnswer });
});

test('decline is accepted only where offered and bypasses form answers', () => {
  const s = findScenario('decline');
  assert.deepEqual(interpretCardAction(s, callback(s, ACTION_DECLINE)), { resolution: { declined: true } });
  const other = findScenario('elicitation-single');
  assert.deepEqual(interpretCardAction(other, callback(other, ACTION_DECLINE)), { error: POC_ERROR.invalidAction });
  assert.match(buildTerminalCard(s, { declined: true }, 'im').body.elements[0].content, /拒绝回答/);
});

test('observed Feishu shapes: empty inputs arrive as "", decline carries no form_value', () => {
  const single = findScenario('custom-single');
  assert.deepEqual(interpretCardAction(single, callback(single, ACTION_SUBMIT, { q0: 'other', q0_other: '' })), { error: POC_ERROR.customAnswerMissing });
  assert.deepEqual(interpretCardAction(single, callback(single, ACTION_SUBMIT, { q0: 'staging', q0_other: '睡觉就睡觉' })), { resolution: { answers: { q0: 'staging' } } });
  const multi = findScenario('custom-multi');
  assert.deepEqual(interpretCardAction(multi, callback(multi, ACTION_SUBMIT, { q0: ['unit'], q0_other: '' })), { resolution: { answers: { q0: ['unit'] } } });
  const decline = findScenario('decline');
  assert.deepEqual(interpretCardAction(decline, { ...callback(decline, ACTION_DECLINE), action: { value: { poc: 'decline', rid: 'r1', act: ACTION_DECLINE }, tag: 'button' } }),
    { resolution: { declined: true } });
});
