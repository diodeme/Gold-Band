/** @vitest-environment jsdom */
import { describe, expect, it } from 'vitest';
import { sharpenIconsIn } from '../../marketing/site/replay-icons';

const codex = '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24"><path fill="currentColor" d="M0 0"/></svg>';

describe('replay icon sharpening', () => {
  it('draws the recorded SVG instead of the 16px bitmap snapshot', async () => {
    document.body.innerHTML = '<span class="size-5"><img alt="" class="object-contain dark:invert size-5 scale-125" style="width:20px;height:20px" src="data:image/png;base64,aaaa" rrweb-original-src="/media/agent-icons/codex.svg"></span>';
    await sharpenIconsIn(document, async () => codex);
    const svg = document.querySelector('svg');
    expect(document.querySelector('img')).toBeNull();
    expect(svg?.getAttribute('viewBox')).toBe('0 0 24 24');
    expect(svg?.getAttribute('width')).toBeNull();
    expect(svg?.style.width).toBe('20px');
    expect(svg?.style.height).toBe('20px');
    expect(svg?.style.transform).toBe('none');
    expect(svg?.style.filter).toBe('none');
  });
});