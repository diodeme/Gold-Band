/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@uiw/react-codemirror', () => ({
  basicSetup: () => [],
  default: (props: { value: string }) => <div data-testid="readonly-code">{props.value}</div>,
}));

vi.mock('@/components/workspace/files/editor-extensions', () => ({
  loadWorkspaceLanguageForPath: async () => null,
  workspaceEditorTheme: [],
  workspaceSyntaxHighlighting: [],
}));

vi.mock('@/components/workspace/files/ReadonlyMarkdownWorkspaceViewer', () => ({
  ReadonlyMarkdownWorkspaceViewer: (props: { documentKey: string; value: string }) => (
    <div data-testid="readonly-markdown" data-document-key={props.documentKey}>{props.value}</div>
  ),
}));

import { ReadonlyTextWorkspaceViewer } from '@/components/workspace/files/ReadonlyTextWorkspaceViewer';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('read-only text workspace viewer', () => {
  afterEach(() => document.body.replaceChildren());

  it('routes Markdown through the shared render/source viewer', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ReadonlyTextWorkspaceViewer documentKey="draft:notes" name="notes.md" value="# Notes" />);
      });

      const markdown = container.querySelector<HTMLElement>('[data-testid="readonly-markdown"]');
      expect(markdown?.dataset.documentKey).toBe('draft:notes');
      expect(markdown?.textContent).toBe('# Notes');
      expect(container.querySelector('[data-testid="readonly-code"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps non-Markdown text in the shared read-only code viewer', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ReadonlyTextWorkspaceViewer documentKey="draft:notes" name="notes.txt" value="plain text" />);
      });

      expect(container.querySelector('[data-testid="readonly-markdown"]')).toBeNull();
      expect(container.querySelector('[data-testid="readonly-code"]')?.textContent).toBe('plain text');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
