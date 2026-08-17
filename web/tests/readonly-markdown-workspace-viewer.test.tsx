/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import { fileContentStore } from '@/components/workspace/files/file-content-store';

vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: (props: {
    documentKey: string;
    editable: boolean;
    highlight: boolean;
    markdownMode: string | null;
    markdownLivePreviewAvailable: boolean;
    onMarkdownModeChange?: (mode: 'live-preview' | 'source') => void;
  }) => (
    <div
      data-testid="readonly-markdown-editor"
      data-document-key={props.documentKey}
      data-editable={String(props.editable)}
      data-highlight={String(props.highlight)}
      data-markdown-mode={props.markdownMode}
      data-live-preview-available={String(props.markdownLivePreviewAvailable)}
    >
      <button type="button" onClick={() => props.onMarkdownModeChange?.('source')}>source</button>
    </div>
  ),
}));

import { ReadonlyMarkdownWorkspaceViewer } from '@/components/workspace/files/ReadonlyMarkdownWorkspaceViewer';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('read-only Markdown workspace viewer', () => {
  beforeEach(() => {
    fileContentStore.configure({
      ...FALLBACK_WORKSPACE_FILES,
      textHighlightMaxChars: 20,
      markdownLivePreviewMaxChars: 40,
    });
  });

  afterEach(() => {
    fileContentStore.configure(FALLBACK_WORKSPACE_FILES);
    document.body.replaceChildren();
  });

  it('owns one transient mode per document identity and always stays read-only', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ReadonlyMarkdownWorkspaceViewer documentKey="run:a.md" value="# A" />);
      });

      let editor = container.querySelector<HTMLElement>('[data-testid="readonly-markdown-editor"]');
      expect(editor?.dataset.editable).toBe('false');
      expect(editor?.dataset.markdownMode).toBe('live-preview');

      await act(async () => {
        editor?.querySelector<HTMLButtonElement>('button')?.click();
      });
      editor = container.querySelector<HTMLElement>('[data-testid="readonly-markdown-editor"]');
      expect(editor?.dataset.markdownMode).toBe('source');

      await act(async () => {
        root.render(<ReadonlyMarkdownWorkspaceViewer documentKey="run:b.md" value="# B" />);
      });
      editor = container.querySelector<HTMLElement>('[data-testid="readonly-markdown-editor"]');
      expect(editor?.dataset.documentKey).toBe('run:b.md');
      expect(editor?.dataset.markdownMode).toBe('live-preview');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('uses the shared size policy to avoid highlighting or previewing oversized documents', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ReadonlyMarkdownWorkspaceViewer documentKey="run:large.md" value={'#'.repeat(41)} />);
      });

      const editor = container.querySelector<HTMLElement>('[data-testid="readonly-markdown-editor"]');
      expect(editor?.dataset.highlight).toBe('false');
      expect(editor?.dataset.markdownMode).toBe('source');
      expect(editor?.dataset.livePreviewAvailable).toBe('false');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
