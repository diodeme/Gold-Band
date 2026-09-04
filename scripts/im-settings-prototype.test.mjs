import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { JSDOM } from 'jsdom';

const prototypeUrl = new URL(
  '../docs/gold-band/产品设计文档/interaction/app/原型/IM干预设置界面/code.html',
  import.meta.url,
);

function createPrototype() {
  const source = readFileSync(prototypeUrl, 'utf8')
    .replace(/<script src="[^"]+" defer><\/script>/, '');
  const dom = new JSDOM(source, {
    pretendToBeVisual: true,
    runScripts: 'dangerously',
    url: 'http://gold-band.local/im-settings-prototype',
  });
  return dom;
}

function click(window, selector) {
  const element = window.document.querySelector(selector);
  assert.ok(element, `missing element: ${selector}`);
  element.dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
  return element;
}

function changeCheckbox(window, key, checked) {
  const input = window.document.querySelector(`[data-notification="${key}"]`);
  assert.ok(input, `missing notification: ${key}`);
  input.checked = checked;
  input.dispatchEvent(new window.Event('change', { bubbles: true }));
}

test('progressively discloses setup and notification controls', () => {
  const dom = createPrototype();
  const { document } = dom.window;

  assert.match(document.querySelector('#channel-content').textContent, /接入企业微信机器人/);
  assert.equal(document.querySelector('#notification-section').hidden, true);

  click(dom.window, '[data-state="binding"]');
  assert.match(document.querySelector('#channel-content').textContent, /发送一条私聊消息/);
  assert.equal(document.querySelector('#notification-section').hidden, false);

  click(dom.window, '[data-state="connecting"]');
  assert.equal(document.querySelector('#notification-section').hidden, false);

  click(dom.window, '[data-state="connected"]');
  assert.match(document.querySelector('#channel-content').textContent, /企业微信已可用/);
  assert.equal(document.querySelector('#notification-section').hidden, false);
  assert.equal(document.querySelectorAll('[data-notification]').length, 6);
  dom.window.close();
});

test('keeps QR authorization and private binding in one short dialog flow', () => {
  const dom = createPrototype();
  const { document } = dom.window;

  click(dom.window, '[data-action="open-setup"]');
  assert.equal(document.querySelector('#setup-dialog').hidden, false);
  assert.match(document.querySelector('#setup-dialog-body').textContent, /等待扫码确认/);

  click(dom.window, '[data-action="scan-success"]');
  assert.match(document.querySelector('#setup-dialog-body').textContent, /等待一条私聊消息/);
  assert.match(document.querySelector('#channel-content').textContent, /等待消息/);

  click(dom.window, '[data-action="binding-success"]');
  assert.match(document.querySelector('#setup-dialog-body').textContent, /企业微信已可用/);
  assert.match(document.querySelector('#channel-content').textContent, /企业微信已可用/);
  dom.window.close();
});

test('shows dirty state, saves changes, and discards back to the last saved values', async () => {
  const dom = createPrototype();
  const { document } = dom.window;
  click(dom.window, '[data-state="connected"]');

  changeCheckbox(dom.window, 'runSuccess', true);
  assert.equal(document.querySelector('#save-strip').hidden, false);
  click(dom.window, '[data-action="save"]');
  await new Promise((resolve) => dom.window.setTimeout(resolve, 700));
  assert.equal(document.querySelector('#save-strip').hidden, true);
  assert.equal(document.querySelector('[data-notification="runSuccess"]').checked, true);

  changeCheckbox(dom.window, 'runSuccess', false);
  assert.equal(document.querySelector('#save-strip').hidden, false);
  click(dom.window, '[data-action="discard"]');
  assert.equal(document.querySelector('[data-notification="runSuccess"]').checked, true);
  assert.equal(document.querySelector('#save-strip').hidden, true);
  dom.window.close();
});

test('provides contextual recovery and distinct configuration confirmations', async () => {
  const dom = createPrototype();
  const { document } = dom.window;

  click(dom.window, '[data-state="error"]');
  assert.match(document.querySelector('#channel-content').textContent, /正在自动重连/);
  assert.equal(document.querySelector('[data-action="retry"]'), null);

  click(dom.window, '[data-state="conflict"]');
  click(dom.window, '[data-action="retry"]');
  assert.match(document.querySelector('#channel-content').textContent, /正在连接企业微信/);
  await new Promise((resolve) => dom.window.setTimeout(resolve, 950));
  assert.match(document.querySelector('#channel-content').textContent, /企业微信已可用/);

  click(dom.window, '[data-action="toggle-menu"]');
  click(dom.window, '[data-action="reset-binding"]');
  assert.equal(document.querySelector('#reset-dialog').hidden, false);
  click(dom.window, '[data-action="confirm-reset"]');
  assert.match(document.querySelector('#channel-content').textContent, /发送一条私聊消息/);

  click(dom.window, '[data-state="connected"]');
  click(dom.window, '[data-action="toggle-menu"]');
  click(dom.window, '[data-action="delete"]');
  assert.equal(document.querySelector('#delete-dialog').hidden, false);
  click(dom.window, '[data-action="confirm-delete"]');
  assert.match(document.querySelector('#channel-content').textContent, /接入企业微信机器人/);
  dom.window.close();
});

test('exposes accessible state, dialog, and preference semantics', () => {
  const dom = createPrototype();
  const { document } = dom.window;
  click(dom.window, '[data-state="connected"]');

  assert.equal(document.querySelector('[role="switch"]').getAttribute('aria-checked'), 'true');
  assert.equal(document.querySelectorAll('[role="tab"]').length, 3);
  assert.equal(document.querySelector('#setup-dialog [role="dialog"]').getAttribute('aria-modal'), 'true');
  assert.equal(document.querySelector('#delete-dialog [role="alertdialog"]').getAttribute('aria-modal'), 'true');
  assert.equal(document.querySelector('#reset-dialog [role="alertdialog"]').getAttribute('aria-modal'), 'true');
  assert.equal(document.querySelectorAll('label.check-row input[type="checkbox"]').length, 6);
  dom.window.close();
});

test('applies enablement immediately without mixing it into the notification draft', async () => {
  const dom = createPrototype();
  const { document } = dom.window;
  click(dom.window, '[data-state="connected"]');

  click(dom.window, '[data-action="toggle-enabled"]');
  const toggle = document.querySelector('[data-action="toggle-enabled"]');
  assert.equal(toggle.getAttribute('aria-busy'), 'true');
  assert.equal(toggle.disabled, true);
  assert.equal(document.querySelector('#save-strip').hidden, true);
  await new Promise((resolve) => dom.window.setTimeout(resolve, 550));
  assert.match(document.querySelector('#channel-content').textContent, /远程干预已暂停/);
  dom.window.close();
});
