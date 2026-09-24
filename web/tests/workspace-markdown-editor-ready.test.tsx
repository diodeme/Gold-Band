/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';

const loaders = vi.hoisted(() => ({
  language: () => new Promise(() => undefined),
  preview: () => new Promise(() => undefined),
}));

vi.mock('@/components/workspace/files/markdown-live-preview', () => ({
  loadMarkdownLanguageExtension: () => loaders.language(),
  loadMarkdownPreviewExtensions: () => loaders.preview(),
}));

import { WorkspaceFileEditor } from '@/components/workspace/files/WorkspaceFileEditor';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

function renderEditor(onMarkdownModeChange: (mode: 'live-preview' | 'source') => void) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  const props = {
    documentKey: 'markdown-ready',
    value: '# Ready',
    editable: false,
    language: 'markdown',
    highlight: true,
    contentRevision: 1,
    target: null,
    targetRevision: 0,
    onChange: () => undefined,
    onSave: () => undefined,
    initialStateJson: null,
    onPersistState: () => undefined,
    markdownMode: 'live-preview' as const,
    markdownLivePreviewAvailable: true,
    onMarkdownModeChange,
  };
  return { container, root, props };
}

describe('Markdown editor availability', () => {
  it('waits for preview extensions and then opens in live preview', async () => {
    let resolveLanguage: (extension: never[]) => void = () => undefined;
    let resolvePreview: (extensions: never[]) => void = () => undefined;
    loaders.language = () => new Promise((resolve) => { resolveLanguage = resolve; });
    loaders.preview = () => new Promise((resolve) => { resolvePreview = resolve; });
    const onMarkdownModeChange = vi.fn();
    const { container, root, props } = renderEditor(onMarkdownModeChange);
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} />
        </TooltipProvider>,
      ));
      expect(container.querySelector('[aria-label="workspace-markdown-loading"]')).not.toBeNull();
      expect(container.querySelector('.cm-lineNumbers')).toBeNull();
      await act(async () => {
        resolveLanguage([]);
        resolvePreview([]);
      });
      expect(container.querySelector('[aria-label="workspace-markdown-loading"]')).toBeNull();
      expect(container.querySelector('.workspace-markdown-live-preview')).not.toBeNull();
      expect(container.querySelector('.cm-lineNumbers')).toBeNull();
      const toggle = container.querySelector<HTMLButtonElement>('[data-markdown-mode-toggle="true"]');
      expect(toggle?.getAttribute('aria-label')).toContain('viewMarkdownSource');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not open a source editor while preview extensions are still loading', async () => {
    loaders.language = () => new Promise(() => undefined);
    loaders.preview = () => new Promise(() => undefined);
    const onMarkdownModeChange = vi.fn();
    const { container, root, props } = renderEditor(onMarkdownModeChange);
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} />
        </TooltipProvider>,
      ));
      expect(container.querySelector('[aria-label="workspace-markdown-loading"]')).not.toBeNull();
      expect(container.querySelector('.cm-editor')).toBeNull();
      expect(container.querySelector('.cm-lineNumbers')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the source-to-preview action enabled while preview extensions are loading', async () => {
    loaders.language = () => new Promise(() => undefined);
    loaders.preview = () => new Promise(() => undefined);
    const onMarkdownModeChange = vi.fn();
    const { container, root, props } = renderEditor(onMarkdownModeChange);
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} markdownMode="source" />
        </TooltipProvider>,
      ));
      const toggle = container.querySelector<HTMLButtonElement>('[data-markdown-mode-toggle="true"]');
      expect(toggle).not.toBeNull();
      expect(toggle?.disabled).toBe(false);
      await act(async () => toggle?.click());
      expect(onMarkdownModeChange).toHaveBeenCalledWith('live-preview');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('stays on the source editor when preview extensions fail', async () => {
    loaders.language = () => Promise.reject(new Error('language failed'));
    loaders.preview = () => Promise.reject(new Error('preview failed'));
    const onMarkdownModeChange = vi.fn();
    const view = renderEditor(onMarkdownModeChange);
    try {
      await act(async () => view.root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...view.props} />
        </TooltipProvider>,
      ));
      await act(async () => undefined);
      expect(view.container.querySelector('[aria-label="workspace-markdown-loading"]')).toBeNull();
      expect(view.container.querySelector('.cm-editor')).not.toBeNull();
      expect(view.container.querySelector('.workspace-markdown-live-preview')).toBeNull();
    } finally {
      await act(async () => view.root.unmount());
    }
  });
});
