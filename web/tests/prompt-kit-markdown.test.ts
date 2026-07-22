import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Markdown } from '@/components/prompt-kit/markdown';
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

  it('only puts the paced visible prefix into layout while streaming', () => {
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

  it('keeps Streamdown animation metadata disabled during streaming', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '正在增长的内容',
      streaming: true,
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

  it('does not leak renderer metadata into code DOM attributes', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '```ts\nconst value = 1;\n```',
    }));

    expect(html).toContain('const value = 1;');
    expect(html).not.toContain('node=');
  });
});
