/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Markdown, MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => document.body.replaceChildren());

describe('Markdown local file link routing', () => {
  it('routes a local path to the workspace handler without opening a new browser target', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const openLocalFile = vi.fn();
    try {
      await act(async () => root.render(
        <MarkdownResourceLinkProvider handler={{ openLocalFile }}>
          <Markdown>{'[client.rs](D:/repo/src/client.rs:2727)'}</Markdown>
        </MarkdownResourceLinkProvider>,
      ));
      const link = container.querySelector<HTMLAnchorElement>('a');
      expect(link, container.innerHTML).not.toBeNull();
      expect(link?.target).toBe('');
      expect(link?.className).toContain('bg-muted/45');
      expect(link?.className).not.toContain('border-gold-running');
      expect(link?.querySelector('svg')?.getAttribute('class')).toContain('text-gold-running');
      await act(async () => link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })));
      expect(openLocalFile).toHaveBeenCalledWith('D:/repo/src/client.rs:2727');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('routes extensionless workspace-relative files through the same handler', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const openLocalFile = vi.fn();
    try {
      await act(async () => root.render(
        <MarkdownResourceLinkProvider handler={{ openLocalFile }}>
          <Markdown>{'[Makefile](Makefile:12)'}</Markdown>
        </MarkdownResourceLinkProvider>,
      ));
      const link = container.querySelector<HTMLAnchorElement>('a');
      await act(async () => link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })));
      expect(openLocalFile).toHaveBeenCalledWith('Makefile:12');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('preserves external link behavior', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const openLocalFile = vi.fn();
    try {
      await act(async () => root.render(
        <MarkdownResourceLinkProvider handler={{ openLocalFile }}>
          <Markdown>{'[website](https://example.com/file.rs:12)'}</Markdown>
        </MarkdownResourceLinkProvider>,
      ));
      const link = container.querySelector<HTMLAnchorElement>('a');
      expect(link?.target).toBe('_blank');
      expect(link?.rel).toBe('noreferrer');
      expect(openLocalFile).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps a local link inert when no workspace handler is available', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<Markdown>{'[client](src/client.rs:10)'}</Markdown>));
      const link = container.querySelector<HTMLAnchorElement>('a');
      expect(link?.getAttribute('href')).toBeNull();
      expect(link?.getAttribute('aria-disabled')).toBe('true');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
