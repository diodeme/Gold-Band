/** @vitest-environment jsdom */
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { loadMarkdownLanguageExtension, loadMarkdownPreviewExtensions } from '@/components/workspace/files/markdown-live-preview';
import {
  markdownImagePreview,
  markdownImageSources,
  markdownRemoteImageHosts,
  type MarkdownImagePreviewConfig,
} from '@/components/workspace/files/markdown-image-preview';

const ALERT_TITLES = { note: '说明', tip: '提示', important: '重要', warning: '警告', caution: '注意' };
const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

/** Same extension stack as the live-preview editor: Atomic preview plus our decorations. */
async function livePreview(doc: string, config: Partial<MarkdownImagePreviewConfig> = {}) {
  const [language, preview] = await Promise.all([
    loadMarkdownLanguageExtension(),
    loadMarkdownPreviewExtensions(() => undefined, true),
  ]);
  const parent = document.createElement('div');
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [language, ...preview, markdownImagePreview({ images: new Map(), alertTitles: ALERT_TITLES, ...config })],
    }),
  });
  views.push(view);
  return view;
}

describe('Markdown live preview with Atomic inline preview', () => {
  it('keeps the remote image link visible instead of Atomic hiding the whole image line', async () => {
    const view = await livePreview('Text\n\n![image.png](https://static.dion.blue/a.png)\n\nMore');

    const link = view.dom.querySelector<HTMLAnchorElement>('.cm-gold-band-markdown-remote-image-link');
    expect(link?.textContent).toBe('image.png');
    expect(view.dom.querySelector('.cm-gold-band-markdown-remote-image')).toBeNull();
  });

  it('renders images from trusted hosts and opens the image or its outer link on click', async () => {
    const onLinkClick = vi.fn();
    const view = await livePreview(
      '![shot](https://Static.Dion.Blue/a.png)\n\n[![badge](https://static.dion.blue/b.svg)](https://example.com/repo)\n\n![other](https://other.example/c.png)',
      { trustedImageHosts: new Set(['static.dion.blue']), onLinkClick },
    );

    const images = [...view.dom.querySelectorAll<HTMLImageElement>('img.cm-gold-band-markdown-remote-image')];
    expect(images.map((image) => image.alt)).toEqual(['shot', 'badge']);
    expect(images[0]?.referrerPolicy).toBe('no-referrer');
    for (const image of images) image.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onLinkClick.mock.calls.map(([href]) => href)).toEqual([
      'https://Static.Dion.Blue/a.png',
      'https://example.com/repo',
    ]);
    expect(view.dom.querySelector('.cm-gold-band-markdown-remote-image-link')?.textContent).toBe('other');
  });

  it('renders the five GitHub alert types with localized titles', async () => {
    const view = await livePreview('> [!WARNING]\n> 放弃更改无法撤销\n\n> [!tip]\n> body');

    const titles = [...view.dom.querySelectorAll('.cm-gold-band-markdown-alert-title')].map((title) => title.textContent);
    expect(titles).toEqual(['警告', '提示']);
    expect(view.dom.querySelectorAll('.cm-gold-band-markdown-alert-warning')).toHaveLength(2);
    expect(view.dom.querySelectorAll('.cm-gold-band-markdown-alert-tip')).toHaveLength(2);
    expect(view.dom.textContent).not.toContain('[!WARNING]');
  });

  it('shows non-standard alert markers and other undefined references as plain bracketed text', async () => {
    const view = await livePreview('> [!attention] 放弃更改无法撤销\n> 此操作不会进入系统回收站。\n\nSee [1] and [docs].\n\n[docs]: https://example.com');

    expect(view.dom.querySelector('.cm-gold-band-markdown-alert')).toBeNull();
    const plain = [...view.dom.querySelectorAll('.cm-gold-band-markdown-plain-reference')].map((node) => node.textContent);
    expect(plain).toEqual(['!attention', '1']);
    expect(view.dom.textContent).toContain('[!attention] 放弃更改无法撤销');
    expect(view.dom.textContent).toContain('See [1] and docs.');
  });
});

describe('Markdown image source scanning', () => {
  it('separates local sources from remote image hosts', () => {
    const markdown = '![a](local.png) ![b](https://A.example/x.png) <img src="//b.example/y.png"> ![c](https://a.example/z.png)';
    expect(markdownImageSources(markdown)).toEqual(['local.png']);
    expect(markdownRemoteImageHosts(markdown)).toEqual(['a.example', 'a.example', 'b.example']);
  });
});
