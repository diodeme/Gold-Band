// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createSceneEngine } from '../../marketing/site/Replay';
import { SceneManifestSchema } from '../../marketing/site/replay-model';

it('shows the recorded open mobile review Sheet when seeking while paused', () => {
  Object.defineProperty(HTMLIFrameElement.prototype, 'sandbox', { configurable: true, get() { return (this.getAttribute('sandbox') ?? '').split(/\s+/).filter(Boolean); } });
  const directory = process.env.SITE_AFTER_ASSETS || 'marketing/site/media/after';
  const manifest = SceneManifestSchema.parse(JSON.parse(readFileSync(resolve(directory, 'manifest.json'), 'utf8')));
  const asset = manifest.mobileAssets!.find(a => a.language === 'en' && a.theme === 'dark')!;
  const recording = JSON.parse(readFileSync(resolve(directory, asset.eventsUrl.slice('/media/after/'.length)), 'utf8'));
  const root = document.createElement('div'); document.body.append(root);
  const engine = createSceneEngine(recording.events, asset, root);
  root.querySelector('iframe')!.contentWindow!.scrollTo = () => {};
  try {
    const point = asset.checkpoints.find(p => p.stepId === 'workspace-edit')!;
    engine.pause(point.startMs + (point.endMs-point.startMs)*0.1);
    const doc = root.querySelector('iframe')!.contentDocument!;
    const sheet = doc.querySelector('[data-slot="sheet-content"][data-state="open"]')!;
    expect(sheet.textContent).toContain('Keep reviews local.');
    const pauseRules = Array.from(doc.styleSheets).flatMap(sheet => Array.from(sheet.cssRules))
      .filter((rule): rule is CSSStyleRule => 'selectorText' in rule && rule.cssText.includes('animation: none'));
    expect(pauseRules.some(rule => sheet.matches(rule.selectorText))).toBe(true);
  } finally { engine.destroy(); root.remove(); delete (HTMLIFrameElement.prototype as Partial<HTMLIFrameElement>).sandbox; }
});
