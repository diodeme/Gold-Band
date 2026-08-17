/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { HiddenPromptMessageContent } from '@/components/acp/HiddenPromptMessageContent';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('hidden prompt content links', () => {
  it('projects appended runtime context as an icon link above the visible user message', async () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    Object.defineProperty(Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onOpenSection = vi.fn();

    try {
      await act(async () => {
        root.render(<HiddenPromptMessageContent content={[
          '本次目标变更：你输出一个 hi 就好了',
          '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">',
          'runtime suspended',
          '</hidden>',
        ].join('\n')} onOpenSection={onOpenSection} />);
      });

      const contentRoot = container.firstElementChild;
      const link = contentRoot?.querySelector<HTMLButtonElement>('[data-hidden-prompt-link="true"]');
      expect(link?.querySelector('svg')).not.toBeNull();
      expect(link?.textContent).toContain('acp.hiddenRuntimeContext');
      expect(contentRoot?.querySelector('[data-slot="collapsible"]')).toBeNull();
      expect(contentRoot?.children[1]?.textContent).toContain('本次目标变更');

      await act(async () => link?.click());
      expect(onOpenSection).toHaveBeenCalledWith({
        sourceIndex: 1,
        title: 'Gold Band runtime context',
        label: 'acp.hiddenRuntimeContext',
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('opens each hidden section by its stable parsed part index without inline expansion', async () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    Object.defineProperty(Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    });

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onOpenSection = vi.fn();
    const content = [
      '<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">',
      'system detail',
      '</hidden>',
      '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">',
      'runtime detail',
      '</hidden>',
      '# Requirement',
      'verify disclosure scrolling',
    ].join('\n');

    try {
      await act(async () => {
        root.render(<HiddenPromptMessageContent content={content} onOpenSection={onOpenSection} />);
      });

      const links = container.querySelectorAll<HTMLButtonElement>('[data-hidden-prompt-link="true"]');
      expect(links).toHaveLength(2);

      await act(async () => {
        links[1]?.click();
      });
      expect(onOpenSection).toHaveBeenCalledWith({
        sourceIndex: 2,
        title: 'Gold Band runtime context',
        label: 'acp.hiddenRuntimeContext',
      });
      expect(container.textContent).not.toContain('runtime detail');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not render a link for a hidden section marked show=false', async () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    Object.defineProperty(Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<HiddenPromptMessageContent content={'用户消息\n<hidden data-gold-band-hidden="true" show="false" title="Gold Band runtime control">resume</hidden>'} />);
      });

      expect(container.querySelector('[data-hidden-prompt-link="true"]')).toBeNull();
      expect(container.textContent).toContain('用户消息');
      expect(container.textContent).not.toContain('resume');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
