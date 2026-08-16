import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { isLocalFileHref, Markdown, proxyLocalFileLinks } from '@/components/prompt-kit/markdown';
import { TooltipProvider } from '@/components/ui/tooltip';
import { isDocumentAnchorHref, isExternalUrlHref, parseLocalFileLinkTarget } from '@/lib/file-link';

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

  it('projects supported local file targets into compact visible locations', () => {
    expect(parseLocalFileLinkTarget('D:/repo/src/client.rs:2727')).toMatchObject({
      line: 2727,
      column: null,
      endLine: null,
      displayText: ':2727',
    });
    expect(parseLocalFileLinkTarget('D:\\repo\\src\\client.rs:2727:8')).toMatchObject({
      line: 2727,
      column: 8,
      endLine: null,
      displayText: ':2727:8',
    });
    expect(parseLocalFileLinkTarget('file:///D:/repo/src/client.rs#L10-L20')).toMatchObject({
      line: 10,
      column: null,
      endLine: 20,
      displayText: ':10-20',
    });
    expect(parseLocalFileLinkTarget('src/client.rs')).toBeNull();
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
    expect(renderedText(html)).toBe('实时内容');
    expect(html).toContain('data-sd-animate="true"');
  });

  it('uses Streamdown renderer tokens without block-local playback delays', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '**顺滑出现**\n\n第二段',
      streaming: true,
    }));

    expect(html).toContain('data-sd-animate="true"');
    expect(html).toContain('data-gb-stream-block="true"');
    expect(html).not.toContain('--sd-delay');
    expect(html).toContain('<strong');
    expect(renderedText(html)).toBe('顺滑出现第二段');
  });

  it('keeps Streamdown animation metadata disabled after streaming finishes', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '正在增长的内容',
    }));

    expect(html).not.toContain('data-sd-animate');
    expect(html).not.toContain('--sd-animation');
    expect(html).not.toContain('--sd-delay');
  });

  it('renders inline code as a body-sized label instead of a smaller monospace fragment', () => {
    const html = renderToStaticMarkup(createElement(Markdown, {
      children: '运行 `npm run themes:build` 后继续。',
    }));

    expect(html).toContain('font-sans text-[1em] font-normal leading-[inherit] tracking-normal');
    expect(html).toContain('rounded-md bg-gold-surface-high px-1.5 py-0.5');
    expect(html).not.toContain('bg-muted/50');
    expect(html).not.toContain('font-mono');
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

  it('does not leak renderer metadata into code DOM attributes', () => {
    const html = renderToStaticMarkup(createElement(
      TooltipProvider,
      null,
      createElement(Markdown, {
        children: '```ts\nconst value = 1;\n```',
      }),
    ));

    expect(html).toContain('const value = 1;');
    expect(html).toContain('data-streamdown="code-block"');
    expect(html).toContain('data-streamdown="code-block-header"');
    expect(html).toContain('data-language="ts"');
    expect(html).toContain('data-streamdown="code-block-copy-button"');
    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).toContain('aria-label="common.copyCode"');
    expect(html).not.toContain('title="common.copyCode"');
    expect(html).not.toContain('node=');
  });

  it('uses the project image adapter instead of Streamdown native-title image controls', () => {
    const html = renderToStaticMarkup(createElement(
      TooltipProvider,
      null,
      createElement(Markdown, {
        children: '<img src="https://example.com/preview.png" alt="Preview" width="64" />',
      }),
    ));

    expect(html).toContain('data-gb-markdown-image="true"');
    expect(html).toContain('aria-label="common.downloadImage"');
    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).not.toContain('title=');
    expect(html).not.toContain('data-streamdown="image-wrapper"');
  });
});
