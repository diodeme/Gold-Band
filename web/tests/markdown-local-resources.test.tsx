/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Markdown, MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';
import { TooltipProvider } from '@/components/ui/tooltip';

vi.mock('@/api', () => ({ openExternalUrl: vi.fn(async () => undefined) }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

async function render(content: string, openLocalFile = vi.fn(), streaming = false) {
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root?.render(
    <TooltipProvider>
      <MarkdownResourceLinkProvider handler={{ openLocalFile }}>
        <Markdown streaming={streaming}>{content}</Markdown>
      </MarkdownResourceLinkProvider>
    </TooltipProvider>,
  ));
  return container;
}

const source = 'docs/benchmark/2026-10-management-briefing/design-demos/previews/overview.png';

describe('Markdown local resource AST routing', () => {
  it('keeps the actual ACP relative image for controlled loading, not harden or the network', async () => {
    const container = await render(`![三方向预览](${source})`);
    expect(container.textContent).not.toContain('[Image blocked:');
    expect(container.querySelector('[data-gb-markdown-local-image]')).not.toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toContain('三方向预览');
  });

  it.each([false, true])('preserves the outer local link of an image (streaming=%s)', async (streaming) => {
    const openLocalFile = vi.fn();
    const target = 'docs/index.html?ov=grid#section';
    const container = await render(`[![三方向预览](${source})](${target})`, openLocalFile, streaming);
    expect(container.querySelector('[data-gb-markdown-local-image]')).not.toBeNull();
    await act(async () => container.querySelector('a')?.click());
    expect(openLocalFile).toHaveBeenCalledWith(target);
  });

  it('resolves reference-style images through the same AST route', async () => {
    const container = await render(`![三方向预览][overview]\n\n[overview]: ${source}`);
    expect(container.textContent).not.toContain('[Image blocked:');
    expect(container.querySelector('[data-gb-markdown-local-image]')).not.toBeNull();
  });

  it('does not rewrite local links written as inline or fenced code', async () => {
    const inline = '[example](docs/index.html?ov=grid)';
    const container = await render(`\`${inline}\`\n\n\`\`\`text\n${inline}\n\`\`\``);
    expect(container.querySelector('code')?.textContent).toBe(inline);
    expect(container.textContent).not.toContain('gold-band.local-file.invalid');
    expect(container.querySelector('a')).toBeNull();
  });
});
