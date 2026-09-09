// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { createSceneEngine } from '../../marketing/site/Replay';
import { SceneManifestSchema } from '../../marketing/site/replay-model';

it('rebuilds the actual task input and mode row after a cross-page before seek', () => {
  // JSDOM does not expose iframe.sandbox; reflect its attribute for rrweb's guard.
  Object.defineProperty(HTMLIFrameElement.prototype, 'sandbox', { configurable: true, get() { return (this.getAttribute('sandbox') ?? '').split(/\s+/).filter(Boolean); } });
  const manifest = SceneManifestSchema.parse(JSON.parse(readFileSync('marketing/site/media/before/manifest.json', 'utf8')));
  const asset = manifest.assets.find(a => a.language === 'zh' && a.theme === 'dark')!;
  const recording = JSON.parse(readFileSync(`marketing/site${asset.eventsUrl}`, 'utf8'));
  const root = document.createElement('div'); document.body.append(root);
  const engine = createSceneEngine(recording.events, asset, root);
  root.querySelector('iframe')!.contentWindow!.scrollTo = () => {};
  try {
    const checkpoint = asset.checkpoints.find(p => p.stepId === 'workflow-ready')!;
    engine.pause(checkpoint.startMs + (checkpoint.endMs - checkpoint.startMs) * 0.2);
    const doc = root.querySelector('iframe')!.contentDocument!;
    expect(Array.from(doc.querySelectorAll('textarea')).map(node => node.value)).toContain('请完善工作区配置，并补充使用说明。');
    expect(doc.querySelector('[role="tab"][data-state="active"]')?.textContent).toBe('工作流');
    const direct = asset.checkpoints.find(p => p.stepId === 'direct')!;
    engine.pause(direct.startMs + (direct.endMs - direct.startMs) * 0.2);
    const menu = doc.querySelector('[data-slot="dropdown-menu-content"]')!;
    const pauseRules = Array.from(doc.styleSheets).flatMap(sheet => Array.from(sheet.cssRules))
      .filter((rule): rule is CSSStyleRule => 'selectorText' in rule && rule.cssText.includes('animation: none'));
    expect(pauseRules.some(rule => menu.matches(rule.selectorText))).toBe(true);
  } finally { engine.destroy(); root.remove(); delete (HTMLIFrameElement.prototype as Partial<HTMLIFrameElement>).sandbox; }
});
