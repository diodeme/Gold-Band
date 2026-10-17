import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const executablePath = process.argv[2] || process.env.DECK_CHROMIUM_PATH;
const root = new URL('../index.html', import.meta.url).href;
const browser = await chromium.launch({ executablePath });
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  page.on('pageerror', error => errors.push(error.message));
  const slide = page.frameLocator('#frame');
  for (const width of [1920, 1280]) {
    await page.setViewportSize({ width, height: width * 9 / 16 });
    for (const [number, expected] of [
      [1, /单主会话（允许分发 subagent）/],
      [2, /中短任务用 Direct，长任务用编排/],
      [4, /mailauth：质量接近，AUTO 开销更高/],
      [6, /Claude Code 未完成就主动退出；\s*AUTO 无需额外闸门，持续推进/],
    ]) {
      await page.goto(root + '#' + number);
      await slide.locator('h1').waitFor();
      await slide.locator('body').evaluate(async () => { await document.fonts.ready; });
      assert.match(await slide.locator('main').innerText(), expected);
      const layout = await slide.locator('h1').evaluate(node => {
        const heading = node.getBoundingClientRect();
        const next = node.nextElementSibling.getBoundingClientRect();
        const range = document.createRange();
        range.selectNodeContents(node);
        return { bottom: heading.bottom, nextTop: next.top, right: Math.max(...[...range.getClientRects()].map(rect => rect.right)) };
      });
      assert.ok(layout.right <= 1824 && layout.bottom + 10 <= layout.nextTop, `${width}px slide ${number}: ${JSON.stringify(layout)}`);
      if (number === 1) {
        const fits = await slide.locator('.identity-mode').first().evaluate(node => {
          const range = document.createRange();
          range.selectNodeContents(node);
          const text = range.getBoundingClientRect();
          return text.right <= node.closest('.identity').getBoundingClientRect().right && text.bottom < document.querySelector('footer').getBoundingClientRect().top;
        });
        assert.ok(fits, 'The subagent qualification fits beside the Claude Code identity.');
      }
    }
  }
  console.log('PASS: four corrected statements are visible and their headings fit at desktop and laptop sizes.');
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.goto(root + '?ov=grid#2');
  await slide.locator('.benchmark').first().waitFor();
  await slide.locator('body').evaluate(async () => { await document.fonts.ready; });
  const first = slide.locator('.benchmark').first();
  await first.locator('summary').click();
  assert.equal(await first.evaluate(node => node.open), true);
  assert.ok(page.url().endsWith('#2'));
  await first.locator('summary').press('Space');
  assert.equal(await first.evaluate(node => node.open), false);
  assert.ok(page.url().endsWith('#2'));
  await first.locator('summary').press('Enter');
  assert.equal(await first.evaluate(node => node.open), true);
  await first.locator('summary').press('Escape');
  assert.equal(await first.evaluate(node => node.open), false);
  assert.ok(page.url().endsWith('#2'));
  console.log('PASS: native disclosure click, Space, Enter, Escape and keyboard ownership.');

  assert.equal(await slide.locator('.benchmark').count(), 5);
  assert.doesNotMatch(await slide.locator('main').innerText(), /stripe-clone/);
  for (let index = 0; index < 5; index++) {
    const detail = slide.locator('.benchmark').nth(index);
    await detail.locator('summary').click();
    assert.equal(await slide.locator('.benchmark[open]').count(), 1);
    const bounds = await detail.locator('.detail').evaluate(node => {
      const box = node.getBoundingClientRect();
      const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
      let textNode;
      let textBottom = box.top;
      while ((textNode = walker.nextNode())) {
        if (!textNode.textContent.trim()) continue;
        const range = document.createRange();
        range.selectNodeContents(textNode);
        for (const rect of range.getClientRects()) textBottom = Math.max(textBottom, rect.bottom);
      }
      return { left: box.left, top: box.top, right: box.right, bottom: box.bottom, textBottom, contentLimit: document.querySelector('.conclusion').getBoundingClientRect().top - 12 };
    });
    assert.ok(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= 1920 && bounds.bottom <= 1080, JSON.stringify(bounds));
    assert.ok(bounds.textBottom <= bounds.contentLimit, `Detail ${index} overlaps the conclusion: ${JSON.stringify(bounds)}`);
  }
  await slide.locator('.benchmark[open] summary').press('Escape');
  await slide.locator('h1').click();
  await page.keyboard.press('ArrowRight');
  await page.waitForURL('**#3');
  assert.equal(await page.locator('#frame').getAttribute('src'), 'slides/03-deepswe.html');
  console.log('PASS: all five exclusive details fit, and focused iframe navigation advances the host.');

  await page.goto(root + '#7');
  await slide.locator('.conclusion').waitFor();
  assert.equal(await slide.locator('footer').count(), 0);
  assert.doesNotMatch(await slide.locator('body').innerText(), /非已证实因果|来源：汇报 README/);
  assert.match(await slide.locator('.conclusion').innerText(), /继续执行的条件，与长期保留约束的能力，需要分别解决/);
  console.log('PASS: the context conclusion remains without the removed disclaimer.');

  await page.locator('#fullButton').click();
  await page.waitForFunction(() => Boolean(document.fullscreenElement));
  await page.locator('#fullButton').click();
  await page.waitForFunction(() => !document.fullscreenElement);
  console.log('PASS: enter and exit fullscreen.');

  await page.goto(root + '#6');
  await slide.locator('details summary').waitFor();
  await slide.locator('details summary').click();
  const panel = await slide.locator('.fault-panel').boundingBox();
  assert.ok(panel.y >= 0 && panel.y + panel.height <= 1080, JSON.stringify(panel));
  await page.screenshot({ path: new URL('../previews/06-expanded.png', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1') });
  await slide.locator('details summary').press('Escape');
  console.log('PASS: the Ruff evidence disclosure stays in the viewport.');

  await page.goto(root + '#0');
  await page.waitForURL('**#1');
  await page.goto(root + '#999');
  await page.waitForURL('**#9');
  await page.evaluate(async () => {
    window.postMessage({ type: 'gold-band-deck:navigate', key: 'Home' }, '*');
    await new Promise(requestAnimationFrame);
  });
  assert.ok(page.url().endsWith('#9'));
  await page.reload();
  assert.equal(await page.locator('#frame').getAttribute('src'), 'slides/09-recommendations.html');
  console.log('PASS: hash bounds, refresh position and rejection of unrelated window messages.');

  await page.setViewportSize({ width: 1280, height: 720 });
  await page.waitForFunction(() => Math.abs(document.querySelector('#stage').getBoundingClientRect().width - Math.min(innerWidth, innerHeight * 16 / 9)) < 1);
  const stage = await page.locator('#stage').boundingBox();
  assert.equal(Math.round(stage.width), 1280);
  assert.equal(Math.round(stage.height), 720);
  await page.locator('#overviewBtn').click();
  assert.equal(await page.locator('body').getAttribute('data-mode'), 'overview');
  await page.locator('#wall .card[data-idx="0"]').click();
  assert.equal(await page.locator('#frame').getAttribute('src'), 'slides/01-intent.html');
  assert.deepEqual(errors, []);
  console.log('PASS: laptop scaling, overview selection and zero page errors.');
} finally {
  await browser.close();
}
