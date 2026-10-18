import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
const sections = [...html.matchAll(/<section class="slide" id="([^"]+)"[\s\S]*?<\/section>/g)];
const plainText = text => text.replace(/<[^>]*>/g, '');

function element(attributes = {}) {
  const handlers = new Map();
  return {
    hidden: false,
    open: false,
    textContent: '',
    scrollTop: 0,
    attributes: { ...attributes },
    addEventListener(type, handler) { handlers.set(type, handler); },
    fire(type, event = {}) { return handlers.get(type)?.(event); },
    getAttribute(name) { return this.attributes[name] ?? null; },
    setAttribute(name, value) { this.attributes[name] = value; },
    removeAttribute(name) { delete this.attributes[name]; },
    closest() { return null; },
  };
}

function mount(initialHash = '') {
  const slides = sections.map(([source, id]) => ({
    ...element(), id,
    querySelector() { return { textContent: plainText(source.match(/<h[12][^>]*>([\s\S]*?)<\/h[12]>/)[1]) }; },
  }));
  const ids = ['previous', 'next', 'fullscreen', 'counter', 'progress', 'announcement', 'notice'];
  const nodes = Object.fromEntries(ids.map(id => [id, element()]));
  const details = Array.from({ length: 6 }, () => element({ name: 'benchmark' }));
  const window = element();
  const document = {
    ...element(), title: '', fullscreenElement: null,
    querySelectorAll(selector) { return selector === '.slide' ? slides : details; },
    getElementById(id) { return nodes[id]; },
    documentElement: {
      async requestFullscreen() { document.fullscreenElement = this; document.fire('fullscreenchange'); },
    },
    async exitFullscreen() { this.fullscreenElement = null; this.fire('fullscreenchange'); },
  };
  let hash = initialHash;
  const location = {
    get hash() { return hash; },
    set hash(value) { hash = value.startsWith('#') ? value : '#' + value; window.fire('hashchange'); },
  };
  const timers = new Map();
  runInNewContext(script, {
    document, window, location,
    setTimeout(fn) { timers.set(1, fn); return 1; },
    clearTimeout(id) { timers.delete(id); },
  });
  const key = (value, overrides = {}) => {
    const event = { key: value, target: element(), preventDefault() { this.prevented = true; }, ...overrides };
    document.fire('keydown', event);
    return event;
  };
  const active = () => slides.filter(slide => !slide.hidden).map(slide => slide.id);
  return { slides, nodes, details, window, document, location, key, active, timers };
}

test('the opening explains intent, all six benchmark tasks exist, and recommendations close the deck', () => {
  assert.equal(sections.length, 9);
  assert.equal(sections[0][1], 'intent');
  assert.equal(sections.at(-1)[1], 'recommendations');
  assert.equal((html.match(/class="benchmark" name="benchmark"/g) ?? []).length, 6);
  for (const label of ['DeepSWE', 'zstd', 'mailauth', 'Stripe', 'K8s', 'Ruff']) assert.ok(html.includes(label), label);
  assert.ok(html.includes('阶段性结果'));
  assert.ok(html.includes('仍在运行'));
});

test('navigation starts at the first slide and clamps both endpoints', () => {
  const app = mount();
  assert.deepEqual(app.active(), ['intent']);
  assert.equal(app.nodes.previous.disabled, true);
  app.nodes.previous.fire('click');
  assert.deepEqual(app.active(), ['intent']);
  app.nodes.next.fire('click');
  assert.deepEqual(app.active(), ['benchmarks']);
  app.key('End');
  assert.deepEqual(app.active(), ['recommendations']);
  assert.equal(app.nodes.next.disabled, true);
  app.nodes.next.fire('click');
  assert.deepEqual(app.active(), ['recommendations']);
  app.key('Home');
  assert.deepEqual(app.active(), ['intent']);
});

test('deep links and history changes update title, counters, visible slide and progress together', () => {
  const app = mount('#marathon');
  assert.deepEqual(app.active(), ['marathon']);
  assert.equal(app.nodes.counter.textContent, '05 / 09');
  assert.equal(app.nodes.progress.value, 5);
  assert.equal(app.nodes.progress.max, 9);
  assert.match(app.document.title, /完整验收/);
  assert.match(app.nodes.announcement.textContent, /第 5 页/);
  app.location.hash = '#mailauth';
  assert.deepEqual(app.active(), ['mailauth']);
  assert.equal(app.nodes.progress.value, 4);
  app.location.hash = '#invalid';
  assert.deepEqual(app.active(), ['intent']);
});

test('all presentation keys work and navigation resets the destination scroll', () => {
  const app = mount();
  app.slides[1].scrollTop = 170;
  assert.equal(app.key('ArrowRight').prevented, true);
  assert.deepEqual(app.active(), ['benchmarks']);
  assert.equal(app.slides[1].scrollTop, 0);
  app.key('PageDown');
  assert.deepEqual(app.active(), ['deepswe']);
  app.key(' ');
  assert.deepEqual(app.active(), ['mailauth']);
  app.key('PageUp');
  app.key('ArrowLeft');
  assert.deepEqual(app.active(), ['benchmarks']);
});

test('keyboard shortcuts do not hijack controls, editable fields or browser shortcuts', () => {
  const app = mount('#benchmarks');
  for (const overrides of [
    { target: { closest: () => ({}) } },
    { ctrlKey: true }, { metaKey: true }, { altKey: true }, { defaultPrevented: true },
  ]) {
    assert.equal(app.key('ArrowRight', overrides).prevented, undefined);
    assert.deepEqual(app.active(), ['benchmarks']);
  }
  assert.equal(app.key('Escape').prevented, undefined);
});

test('fullscreen controls track both entering and leaving fullscreen', async () => {
  const app = mount();
  await app.nodes.fullscreen.fire('click');
  assert.ok(app.document.fullscreenElement);
  assert.equal(app.nodes.fullscreen.textContent, '退出全屏');
  assert.equal(app.nodes.fullscreen.getAttribute('aria-label'), '退出全屏');
  await app.nodes.fullscreen.fire('click');
  assert.equal(app.document.fullscreenElement, null);
  assert.equal(app.nodes.fullscreen.textContent, '全屏');
});

test('denied fullscreen produces a visible recoverable message rather than an unhandled rejection', async () => {
  const app = mount();
  app.document.documentElement.requestFullscreen = async () => { throw new Error('not allowed'); };
  await app.nodes.fullscreen.fire('click');
  assert.equal(app.nodes.notice.hidden, false);
  assert.match(app.nodes.notice.textContent, /浏览器菜单/);
  app.timers.get(1)();
  assert.equal(app.nodes.notice.hidden, true);
});

test('printing expands mutually exclusive details and restores the original expansion state', () => {
  const app = mount('#benchmarks');
  app.details[2].open = true;
  app.window.fire('beforeprint');
  assert.ok(app.details.every(detail => detail.open));
  assert.ok(app.details.every(detail => detail.getAttribute('name') === null));
  app.window.fire('afterprint');
  assert.deepEqual(app.details.map(detail => detail.open), [false, false, true, false, false, false]);
  assert.ok(app.details.every(detail => detail.getAttribute('name') === 'benchmark'));
  assert.deepEqual(app.active(), ['benchmarks']);
});

test('all internal navigation and local evidence links resolve; the deck has no remote assets', () => {
  const slideIds = sections.map(([, id]) => id);
  for (const [, href] of html.matchAll(/href="([^"]+)"/g)) {
    if (href.startsWith('#')) assert.ok(slideIds.includes(href.slice(1)), href);
    else assert.ok(existsSync(new URL(href, import.meta.url)), href);
  }
  assert.doesNotMatch(html, /(?:src|href)="https?:/);
  assert.doesNotMatch(script, /\b(?:fetch|XMLHttpRequest|WebSocket|setInterval|localStorage)\b/);
});

test('reported comparisons retain the correct metrics, units and unfinished-run boundary', () => {
  const deep = sections.find(([, id]) => id === 'deepswe')[0];
  const mail = sections.find(([, id]) => id === 'mailauth')[0];
  const marathon = sections.find(([, id]) => id === 'marathon')[0];
  const ruff = sections.find(([, id]) => id === 'ruff')[0];
  assert.match(deep, /输出 token/);
  assert.match(deep, /15 \/ 29/);
  assert.match(deep, /12 \/ 29/);
  assert.match(deep, /43\/43/);
  assert.match(mail, /694 \/ 702/);
  assert.match(mail, /684 \/ 702/);
  assert.match(mail, /701 \/ 702/);
  assert.match(mail, /702 \/ 702/);
  assert.match(marathon, /1,407 \/ 1,407/);
  assert.match(marathon, /1,405 \/ 1,407/);
  assert.match(marathon, /时间仍需权衡/);
  assert.match(ruff, /309,442,360/);
  assert.match(ruff, /首段不构成 AUTO 的胜出结果/);
  assert.equal(Math.round((325.84 - 173) / 325.84 * 100), 47);
});
