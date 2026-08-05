import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { isLocalFileHref, Markdown, proxyLocalFileLinks } from '@/components/prompt-kit/markdown';
import { isDocumentAnchorHref, isExternalUrlHref } from '@/lib/file-link';
import {
  advanceStreamingMarkdownPresentation,
  createStreamingMarkdownPresentation,
  normalizeStreamingMarkdownPrefix,
  streamingMarkdownPresentationText,
  syncStreamingMarkdownPresentation,
} from '@/lib/streaming-markdown';

function renderedText(html: string) {
  return html.replace(/<[^>]+>/g, '');
}

describe('prompt-kit Markdown', () => {
  it('classifies supported local file links without taking over web links', () => {
    expect(isLocalFileHref('D:/repo/src/client.rs:2727')).toBe(true);
    expect(isLocalFileHref('D:\\repo\\src\\client.rs:2727:8')).toBe(true);
    expect(isLocalFileHref('file:///D:/repo/src/client.rs#L10-L20')).toBe(true);
    expect(isLocalFileHref('src/client.rs:3302')).toBe(true);
    expect(isLocalFileHref('https://example.com/client.rs:12')).toBe(false);
    expect(isLocalFileHref('mailto:dev@example.com')).toBe(false);
    expect(isExternalUrlHref('https://github.com/diodeme/Gold-Band/releases')).toBe(true);
    expect(isDocumentAnchorHref('#')).toBe(true);
  });

  it('proxies only local Markdown destinations through the safe render URL', () => {
    const proxied = proxyLocalFileLinks('[file](D:/repo/client.rs:10) [web](https://example.com) ![image](D:/repo/a.png)');
    expect(proxied).toContain('https://gold-band.local-file.invalid/?href=');
    expect(proxied).toContain('[web](https://example.com)');
    expect(proxied).toContain('![image](D:/repo/a.png)');
  });
  it('renders complete Markdown in static mode', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '**完成内容**',
    }));

    expect(html).toContain('<strong');
    expect(html).toContain('完成内容');
  });

  it('repairs incomplete Markdown while streaming', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '**实时内容',
      streaming: true,
    }));

    expect(html).toContain('<strong');
    expect(renderedText(html)).toBe('实');
  });

  it('only puts the paced visible Markdown prefix into layout while streaming', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '**顺滑出现**\n\n第二段',
      streaming: true,
    }));

    expect(html).not.toContain('data-sd-animate');
    expect(html).not.toContain('--sd-delay');
    expect(html).toContain('<strong');
    expect(renderedText(html)).toBe('顺');
    expect(html).not.toContain('第二段');
  });

  it('keeps Streamdown animation metadata disabled after streaming finishes', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '正在增长的内容',
    }));

    expect(html).not.toContain('data-sd-animate');
    expect(html).not.toContain('--sd-animation');
    expect(html).not.toContain('--sd-delay');
  });

  it('keeps incomplete Markdown control suffixes out of the visible draft', () => {
    expect(normalizeStreamingMarkdownPrefix('**')).toBe('');
    expect(normalizeStreamingMarkdownPrefix('**内容*')).toBe('**内容');
    expect(normalizeStreamingMarkdownPrefix('```ts\nconst value = 1;\n``')).toBe(
      '```ts\nconst value = 1;\n',
    );
    expect(normalizeStreamingMarkdownPrefix('[链接](https://exam')).toBe('链接');
  });

  it('renders backend-separated thought blocks without rewriting content', () => {
    const thought = '**Designing routes.**\n\n**Planning branches.**';
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: thought,
    }));

    expect(html.match(/<strong/g)).toHaveLength(2);
    expect(html.match(/<p/g)).toHaveLength(2);
    expect(renderedText(html)).toBe('Designing routes.\nPlanning branches.');
  });

  it('paces cumulative snapshots independently and converges to canonical Markdown', () => {
    const canonical = '**顺滑出现**\n\n第二段';
    let presentation = createStreamingMarkdownPresentation(canonical, true);

    expect(streamingMarkdownPresentationText(presentation, true)).toBe('**顺');
    expect(presentation.offset).toBeLessThan(canonical.length);

    while (presentation.offset < presentation.canonical.length) {
      presentation = advanceStreamingMarkdownPresentation(presentation, 32);
    }
    presentation = syncStreamingMarkdownPresentation(presentation, canonical, false);

    expect(streamingMarkdownPresentationText(presentation, false)).toBe(canonical);
  });

  it('shows the complete Markdown immediately when a live stream settles', () => {
    const canonical = `\u4e2d${'a'.repeat(90)}`;
    const streamingPresentation = createStreamingMarkdownPresentation(
      canonical,
      true,
    );

    const finishedPresentation = syncStreamingMarkdownPresentation(
      streamingPresentation,
      canonical,
      false,
    );

    expect(finishedPresentation.offset).toBe(canonical.length);
    expect(finishedPresentation.carry).toBe(0);
    expect(streamingMarkdownPresentationText(finishedPresentation, false)).toBe(
      canonical,
    );
  });

  it('never skips a large backlog while the response is still streaming', () => {
    const canonical = `\u4e2d${'a'.repeat(500)}`;
    const streamingPresentation = createStreamingMarkdownPresentation(
      canonical,
      true,
    );

    const syncedPresentation = syncStreamingMarkdownPresentation(
      streamingPresentation,
      canonical,
      true,
    );

    expect(syncedPresentation).toBe(streamingPresentation);
    expect(syncedPresentation.offset).toBeLessThan(canonical.length);
  });

  it('does not leak renderer metadata into code DOM attributes', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '```ts\nconst value = 1;\n```',
    }));

    expect(html).toContain('const value = 1;');
    expect(html).not.toContain('node=');
  });
});
