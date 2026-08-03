/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { loadMarkdownLivePreviewExtensions } from '@/components/workspace/files/markdown-live-preview';
import { markdownImagePreview, updateMarkdownImagePreview } from '@/components/workspace/files/markdown-image-preview';

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

function createView(doc: string, extensions: Parameters<typeof EditorState.create>[0]['extensions']) {
  const parent = document.createElement('div');
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({ doc, extensions }),
  });
  views.push(view);
  return view;
}

describe('Markdown preview DOM contract', () => {
  it('renders a normal GFM table from the real long-form todo document shape', async () => {
    const extensions = await loadMarkdownLivePreviewExtensions(() => undefined, true);
    const todo = readFileSync('docs/gold-band/开发计划/功能点todo列表.md', 'utf8');
    const view = createView(todo, extensions);

    expect(view.dom.querySelector('.cm-atomic-table table')).not.toBeNull();
    expect(view.dom.textContent).toContain('SKILL 多 Agent 实例级管理');
  });

  it('hides Markdown HTML comments while leaving fenced-code comments visible', () => {
    const view = createView(
      '<!-- README-I18N:START -->\n\n中文 | English\n\n<!-- README-I18N:END -->\n\n```md\n<!-- visible example -->\n```',
      [
        markdown({ base: markdownLanguage }),
        markdownImagePreview(new Map()),
      ],
    );

    expect(view.dom.textContent).not.toContain('README-I18N');
    expect(view.dom.textContent).toContain('visible example');
  });

  it('replaces a linked remote SVG badge with a normal link to the outer target', () => {
    const source = 'https://img.shields.io/badge/platform-Windows-blue.svg';
    const view = createView(
      `[![Platform](${source})](https://example.com)`,
      [
        markdown({ base: markdownLanguage }),
        markdownImagePreview(),
      ],
    );

    const link = view.dom.querySelector<HTMLAnchorElement>('.cm-gold-band-markdown-remote-image-link');
    expect(link?.textContent).toBe('Platform');
    expect(link?.href).toBe('https://example.com/');
    expect(view.dom.querySelector('img:not(.cm-widgetBuffer)')).toBeNull();
  });

  it('routes each README badge to its exact outer Markdown target', () => {
    const onLinkClick = vi.fn();
    const view = createView(
      [
        '[![GitHub Stars](https://img.shields.io/github/stars/diodeme/Gold-Band?style=flat-square&color=FFD700)](https://github.com/diodeme/Gold-Band/stargazers)',
        '[![License](https://img.shields.io/badge/license-AGPL--3.0-blue?style=flat-square)](LICENSE)',
        '[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square)](#)',
        '[![Downloads](https://img.shields.io/github/downloads/diodeme/Gold-Band/total?style=flat-square)](https://github.com/diodeme/Gold-Band/releases)',
      ].join('\n'),
      [
        markdown({ base: markdownLanguage }),
        markdownImagePreview(new Map(), undefined, onLinkClick),
      ],
    );

    const links = [...view.dom.querySelectorAll<HTMLAnchorElement>('.cm-gold-band-markdown-remote-image-link')];
    expect(links.map((link) => link.textContent)).toEqual(['GitHub Stars', 'License', 'Platform', 'Downloads']);
    for (const link of links) link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onLinkClick.mock.calls.map(([href]) => href)).toEqual([
      'https://github.com/diodeme/Gold-Band/stargazers',
      'LICENSE',
      '#',
      'https://github.com/diodeme/Gold-Band/releases',
    ]);
  });

  it('routes a relative linked badge through the workspace link handler', () => {
    const onLinkClick = vi.fn();
    const view = createView(
      '[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)',
      [
        markdown({ base: markdownLanguage }),
        markdownImagePreview(new Map(), undefined, onLinkClick),
      ],
    );

    const link = view.dom.querySelector<HTMLAnchorElement>('.cm-gold-band-markdown-remote-image-link');
    expect(link?.getAttribute('href')).toBeNull();
    expect(link?.dataset.href).toBe('LICENSE');
    link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onLinkClick).toHaveBeenCalledOnce();
    expect(onLinkClick).toHaveBeenCalledWith('LICENSE');
  });

  it('turns a standalone remote image into a link to the image URL', () => {
    const source = 'https://example.com/screenshot.png';
    const view = createView(`![Screenshot](${source})`, [
      markdown({ base: markdownLanguage }),
      markdownImagePreview(),
    ]);

    const link = view.dom.querySelector<HTMLAnchorElement>('.cm-gold-band-markdown-remote-image-link');
    expect(link?.textContent).toBe('Screenshot');
    expect(link?.href).toBe(source);
    expect(view.dom.querySelector('img:not(.cm-widgetBuffer)')).toBeNull();
  });

  it('updates local image tokens through a state effect without removing the table widget', async () => {
    const extensions = await loadMarkdownLivePreviewExtensions(() => undefined, true);
    const view = createView(
      '| Name | Value |\n| --- | --- |\n| table | remains |\n\n![Diagram](diagram.png)',
      [...extensions, markdownImagePreview()],
    );

    updateMarkdownImagePreview(view, new Map([['diagram.png', {
      kind: 'ready',
      rawSrc: 'diagram.png',
      canonicalPath: 'D:/repo/diagram.png',
      previewGrant: { token: 'preview-new', expiresAtMs: String(Date.now() + 300_000) },
      mimeType: 'image/png',
      width: 640,
      height: 360,
      animated: false,
    }]]));

    expect(view.dom.querySelector('.cm-atomic-table table')).not.toBeNull();
    expect(view.dom.querySelector('img:not(.cm-widgetBuffer)')).not.toBeNull();
  });

  it('reconfigures source and live preview on the same EditorView', async () => {
    const previewExtensions = await loadMarkdownLivePreviewExtensions(() => undefined, true);
    const sourceExtensions = [markdown({ base: markdownLanguage })];
    const mode = new Compartment();
    const view = createView(
      '| Name | Value |\n| --- | --- |\n| table | remains |\n\n![Diagram](diagram.png)',
      [mode.of([...previewExtensions, markdownImagePreview()])],
    );
    const originalView = view;

    expect(view.dom.querySelector('.cm-atomic-table table')).not.toBeNull();
    view.dispatch({ effects: mode.reconfigure(sourceExtensions) });
    expect(view.dom.querySelector('.cm-atomic-table table')).toBeNull();
    view.dispatch({ effects: mode.reconfigure([...previewExtensions, markdownImagePreview()]) });

    expect(view).toBe(originalView);
    expect(view.dom.querySelector('.cm-atomic-table table')).not.toBeNull();
  });
});
