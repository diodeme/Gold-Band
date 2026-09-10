import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-scene-review-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve('.codex-temp/site-scene-verification');
mkdirSync(output, { recursive: true });
function browser(...args) {
  const path = resolve(output, 'response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45_000, windowsHide: true }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const report = [];
try {
  browser('open', origin);
  for (const theme of (process.env.SITE_THEMES?.split(',') || ['dark', 'light'])) for (const language of ['zh', 'en']) for (const chapter of (process.env.SITE_SCENES?.split(',') || ['before', 'during', 'after', 'personalize'])) {
    assert(['dark', 'light'].includes(theme));
    evaluate(`localStorage.setItem('gold-band.site.appearance.v1',JSON.stringify({version:1,theme:'${theme}'}))`);
    const source = readFileSync(`marketing/site/media/${language}-${chapter}${theme === 'light' ? '-light' : ''}.json`, 'utf8');
    const recording = JSON.parse(source);
    const sourceSha256 = createHash('sha256').update(source).digest('hex');
    const targets = recording.events.filter(event => event.type === 5 && event.data.tag === 'site-shot')
      .map(event => event.data.payload.id).filter(id => !['opening', 'closing'].includes(id));
    assert(targets.length, `${chapter} has no storyboard checkpoints`);
    for (const width of (process.env.SITE_WIDTHS?.split(',').map(Number) || [1440, 390, 320])) {
      browser('set', 'viewport', String(width), width < 500 ? '844' : '880');
      browser('open', `${origin}/${language}/?scene-review=${process.pid}#${chapter}`);
      browser('wait', '--fn', `document.querySelector('[data-chapter-media="${chapter}"] [data-player-state]')?.dataset.playerState === 'ready'`);
      const asset = evaluate(`performance.getEntriesByType('resource').map(entry => entry.name)
        .find(name => name.includes('/${language}-${chapter}-') && name.endsWith('.json'))`);
      assert(asset, 'Production recording request is missing');
      const response = await fetch(asset);
      assert(response.ok, 'Production recording is unavailable');
      assert.equal(createHash('sha256').update(Buffer.from(await response.arrayBuffer())).digest('hex'), sourceSha256, 'Rebuild the site before inspecting changed media');
      const pending = new Set(targets);
      const captures = [];
      const deadline = Date.now() + 150_000;
      while (pending.size && Date.now() < deadline) {
        const state = evaluate(`(() => {
          const player = document.querySelector('.replayer-wrapper');
          const doc = player?.querySelector('iframe')?.contentDocument;
          if (player?.dataset.cameraState === 'stable' && ${JSON.stringify([...pending])}.includes(player?.dataset.checkpoint)) {
            document.querySelector('.replay-toggle').click();
          }
          let avatarContext = null;
          let skillSync = null;
          let outputContract = null;
          let avatarMenu = null;
          let workflowInteraction = null;
          if (doc && ['question', 'permission'].includes(player?.dataset.checkpoint)) {
            const question = player.dataset.checkpoint === 'question';
            const button = [...doc.querySelectorAll('button')].find(element =>
              (question ? ['Enable file snapshots', '启用文件变更快照'] : ['Allow once', '允许一次']).includes(element.textContent.trim()));
            const control = question ? button?.closest('[data-slot="card"]') : button?.parentElement?.parentElement;
            const rect = control?.getBoundingClientRect();
            const plane = player.getBoundingClientRect();
            const stage = player.closest('.recording').getBoundingClientRect();
            const scale = new DOMMatrix(getComputedStyle(player).transform).a;
            const coveringDialogs = rect ? [...doc.querySelectorAll('[role="dialog"][data-state="open"]')].filter(dialog => {
              const bounds = dialog.getBoundingClientRect();
              return !dialog.contains(control) && bounds.left < rect.right && bounds.right > rect.left
                && bounds.top < rect.bottom && bounds.bottom > rect.top;
            }).length : 0;
            workflowInteraction = { scale, coveringDialogs, contained: Boolean(rect && plane.left + rect.left * scale >= stage.left - 1
              && plane.left + rect.right * scale <= stage.right + 1 && plane.top + rect.top * scale >= stage.top - 1
              && plane.top + rect.bottom * scale <= stage.bottom + 1) };
          }
          if (doc && player?.dataset.checkpoint === 'avatar-option') {
            const menu = doc.querySelector('[role="menu"][data-state="open"]');
            const plane = player.getBoundingClientRect();
            const stage = player.closest('.recording').getBoundingClientRect();
            const scale = new DOMMatrix(getComputedStyle(player).transform).a;
            const rect = menu?.getBoundingClientRect();
            avatarMenu = { scale, items: menu?.querySelectorAll('[role="menuitem"]').length ?? 0,
              contained: Boolean(rect && plane.left + rect.left * scale >= stage.left - 1
                && plane.left + rect.right * scale <= stage.right + 1 && plane.top + rect.top * scale >= stage.top - 1
                && plane.top + rect.bottom * scale <= stage.bottom + 1) };
          }
          if (doc && ['json-contract', 'success-expression'].includes(player?.dataset.checkpoint)) {
            const expression = [...doc.querySelectorAll('input')].find(input => input.value === '$.result == true');
            const schema = [...doc.querySelectorAll('textarea')].find(input => input.value.includes('"result"') && input.value.includes('boolean'));
            const fields = { expression: expression?.parentElement, schema: schema?.parentElement.parentElement };
            const targets = innerWidth >= 1024 ? Object.values(fields) : [fields[player.dataset.checkpoint === 'json-contract' ? 'schema' : 'expression']];
            const plane = player.getBoundingClientRect();
            const stage = player.closest('.recording').getBoundingClientRect();
            const scale = new DOMMatrix(getComputedStyle(player).transform).a;
            outputContract = { scale, contained: targets.every(field => {
              if (!field) return false;
              const rect = field.getBoundingClientRect();
              return plane.left + rect.left * scale >= stage.left - 1 && plane.left + rect.right * scale <= stage.right + 1
                && plane.top + rect.top * scale >= stage.top - 1 && plane.top + rect.bottom * scale <= stage.bottom + 1;
            }) };
          }
          if (doc && player?.dataset.checkpoint === 'skill-sync') {
            const icons = [...doc.querySelectorAll('[data-testid="skill-agent-overflow"] img')];
            const plane = player.getBoundingClientRect();
            const stage = player.closest('.recording').getBoundingClientRect();
            const scale = new DOMMatrix(getComputedStyle(player).transform).a;
            skillSync = { count: icons.length, scale, contained: icons.length > 2 && icons.every(icon => {
              const rect = icon.getBoundingClientRect();
              return plane.left + rect.left * scale >= stage.left - 1 && plane.left + rect.right * scale <= stage.right + 1
                && plane.top + rect.top * scale >= stage.top - 1 && plane.top + rect.bottom * scale <= stage.bottom + 1;
            }) };
          }
          if (doc && player?.dataset.checkpoint === 'conversation-avatar') {
            const avatar = [...doc.querySelectorAll('#workspace-center [data-slot="avatar"] img')].filter(image => image.src.startsWith('data:image/png')).at(-1);
            const reply = [...doc.querySelectorAll('p')].find(element => /^(Configuration|配置与文档)/.test(element.textContent));
            if (avatar && reply) {
              const range = doc.createRange(); range.selectNodeContents(reply);
              const bounds = [avatar.getBoundingClientRect(), range.getBoundingClientRect()];
              const plane = player.getBoundingClientRect();
              const stage = player.closest('.recording').getBoundingClientRect();
              const scale = new DOMMatrix(getComputedStyle(player).transform).a;
              avatarContext = { scale, contained: bounds.every(rect => plane.left + rect.left * scale >= stage.left - 1
                && plane.left + rect.right * scale <= stage.right + 1 && plane.top + rect.top * scale >= stage.top - 1
                && plane.top + rect.bottom * scale <= stage.bottom + 1) };
            }
          }
          return { checkpoint: player?.dataset.checkpoint, camera: player?.dataset.cameraState,
            avatarContext, avatarMenu, skillSync, outputContract, workflowInteraction,
            background: getComputedStyle(document.querySelector('.recording[data-player-state="ready"]')).backgroundColor,
            overflow: document.documentElement.scrollWidth > innerWidth,
            textLength: doc?.body.innerText.length ?? 0,
            brokenImages: doc ? [...doc.images].filter(image => (image.getAttribute('src') || image.getAttribute('srcset')) && image.complete && !image.naturalWidth).length : 0 };
        })()`);
        assert(!state.overflow, `${language}/${width}/${chapter} overflows`);
        assert(!['transparent', 'rgba(0, 0, 0, 0)'].includes(state.background), 'Ready replay exposes the poster');
        if (state.camera === 'stable' && pending.has(state.checkpoint)) {
          assert(state.textLength > 50, 'Blank replay');
          assert.equal(state.brokenImages, 0, 'Broken replay image');
          if (['question', 'permission'].includes(state.checkpoint)) {
            assert(state.workflowInteraction?.contained, 'Workflow interaction control is clipped');
            assert.equal(state.workflowInteraction.coveringDialogs, 0, 'Workflow interaction is covered by a dialog');
            assert(state.workflowInteraction.scale >= 0.8, 'Workflow interaction control is too small to read');
          }
          if (state.checkpoint === 'avatar-option') {
            assert(state.avatarMenu?.items >= 2, 'Avatar choices are missing');
            assert(state.avatarMenu.contained, 'Avatar menu is clipped');
            assert(state.avatarMenu.scale >= 0.8, 'Avatar choices are too small to read');
          }
          if (['json-contract', 'success-expression'].includes(state.checkpoint)) {
            assert(state.outputContract?.contained, 'Output configuration field is clipped');
            assert(state.outputContract.scale >= 0.8, 'Output configuration field is too small to read');
          }
          if (state.checkpoint === 'conversation-avatar') {
            assert(state.avatarContext?.contained, 'Avatar camera clips its associated message');
            assert(state.avatarContext.scale >= 0.8, 'Avatar message is too small to read');
          }
          if (state.checkpoint === 'skill-sync') {
            assert(state.skillSync?.contained, 'Skill camera clips its source or synchronization icons');
            assert(state.skillSync.scale >= 0.8, 'Skill synchronization icons are too small to read');
          }
          const path = resolve(output, `${language}-${theme}-${width}-${chapter}-${state.checkpoint}.png`);
          browser('screenshot', '', path);
          captures.push({ ...state, path });
          pending.delete(state.checkpoint);
          evaluate("document.querySelector('.replay-toggle').click()");
        }
        browser('wait', '150');
      }
      assert.equal(pending.size, 0, `Unseen checkpoints: ${[...pending].join(', ')}`);
      report.push({ language, theme, chapter, width, sourceSha256, captures, visualReview: 'pending' });
      writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
      console.log(JSON.stringify({ language, theme, chapter, width, captures: captures.length }));
    }
  }
} finally { browser('close'); }
