// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createSceneEngine } from '../../marketing/site/Replay';

it('retains settings and conversation DOM across real viewport changes', () => {
  const directory = process.env.SITE_PERSONALIZE_ASSETS || 'marketing/site/media/personalize';
  const manifest = JSON.parse(readFileSync(resolve(directory, 'manifest.json'), 'utf8'));
  const asset = manifest.mobileAssets[0];
  const events = JSON.parse(readFileSync(resolve(directory, 'zh-dark-mobile/events.json'), 'utf8')).events;
  Object.defineProperty(HTMLIFrameElement.prototype, 'sandbox', { configurable: true, get() { return (this.getAttribute('sandbox') ?? '').split(/\s+/).filter(Boolean); } });
  const root = document.createElement('div'); document.body.append(root);
  const engine = createSceneEngine(events, asset, root);
  root.querySelector('iframe')!.contentWindow!.scrollTo = () => {};
  try {
    const fontFrame = asset.camera.mobile[4];
    engine.pause(fontFrame.timeMs + fontFrame.transitionMs + 100);
    const fontDoc = root.querySelector('iframe')!.contentDocument!;
    const menu = fontDoc.querySelector('[data-slot="popover-content"][data-state="open"]')!;
    expect(menu?.textContent).toContain('Georgia');
    const rules = Array.from(fontDoc.styleSheets).flatMap(sheet => Array.from(sheet.cssRules))
      .filter((rule): rule is CSSStyleRule => 'selectorText' in rule && rule.cssText.includes('animation: none'));
    expect(rules.some(rule => menu.matches(rule.selectorText))).toBe(true);
    const point = asset.checkpoints.find((p: {stepId:string}) => p.stepId === 'layout-medium');
    engine.pause((point.startMs + point.endMs) / 2);
    const doc = root.querySelector('iframe')!.contentDocument!;
    expect(doc.querySelector('[data-turn-file-changes-card]')).not.toBeNull();
    expect(doc.body.textContent).toContain('配置与文档已更新');
  } finally { engine.destroy(); root.remove(); delete (HTMLIFrameElement.prototype as Partial<HTMLIFrameElement>).sandbox; }
});
