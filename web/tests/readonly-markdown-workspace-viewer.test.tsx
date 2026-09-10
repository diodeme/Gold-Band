/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import { fileContentStore } from '@/components/workspace/files/file-content-store';
import type { WorkspaceFileSnapshotVm } from '@/types';
const imageApi = vi.hoisted(() => ({ resolveMarkdownImage: vi.fn(), releaseWorkspaceFilePreview: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@/api', async importOriginal => ({ ...await importOriginal<object>(), ...imageApi }));

vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: (props: {
    documentKey: string;
    editable: boolean;
    highlight: boolean;
    markdownMode: string | null;
    markdownLivePreviewAvailable: boolean;
    onMarkdownModeChange?: (mode: 'live-preview' | 'source') => void;
    markdownImages?: ReadonlyMap<string, { kind: string }>;
    onMarkdownLinkClick?: (href: string) => void;
  }) => (
    <div
      data-testid="readonly-markdown-editor"
      data-document-key={props.documentKey}
      data-editable={String(props.editable)}
      data-highlight={String(props.highlight)}
      data-markdown-mode={props.markdownMode}
      data-live-preview-available={String(props.markdownLivePreviewAvailable)}
      data-image-state={props.markdownImages?.get('figure.png')?.kind}
    >
      <button type="button" onClick={() => props.onMarkdownModeChange?.('source')}>source</button>
      <button type="button" onClick={() => props.onMarkdownLinkClick?.('report.md')}>link</button>
    </div>
  ),
}));

import { ReadonlyMarkdownWorkspaceViewer } from '@/components/workspace/files/ReadonlyMarkdownWorkspaceViewer';
import { MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('read-only Markdown workspace viewer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
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

  it('resolves local images from the supplied readonly snapshot and releases them on unmount', async () => {
    imageApi.resolveMarkdownImage.mockResolvedValue({ kind: 'ready', canonicalPath: '/run/figure.png', mimeType: 'image/png', width: 80, height: 90, animated: false,
      previewGrant: { token: 'image-token', expiresAtMs: String(Date.now() + 300_000) } });
    const snapshot: WorkspaceFileSnapshotVm = { kind: 'text', name: 'report.md', locator: { projectId: 'project', canonicalPath: '/run/report.md', relativePath: 'report.md', scope: 'workspace' },
      revision: { contentHash: 'hash', byteLength: 20, modifiedAtNs: '0' }, externalAccessGrant: null, content: '![figure](figure.png)', encoding: 'utf-8', language: 'markdown', lineEnding: 'lf', editable: false, limitationCode: null };
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<ReadonlyMarkdownWorkspaceViewer documentKey="run:images" value={snapshot.content} fileSnapshot={snapshot} />));
      expect(imageApi.resolveMarkdownImage).toHaveBeenCalledWith(expect.objectContaining({ projectId: 'project', markdownCanonicalPath: '/run/report.md', rawSrc: 'figure.png' }));
      expect(container.querySelector<HTMLElement>('[data-testid="readonly-markdown-editor"]')?.dataset.imageState).toBe('ready');
    } finally { await act(async () => root.unmount()); container.remove(); }
    expect(imageApi.releaseWorkspaceFilePreview).toHaveBeenCalledWith('image-token');
  });

  it('routes local links through the workspace handler and displays its failure', async () => {
    const openLocalFile = vi.fn().mockResolvedValue({ status: 'error', error: { code: 'workspace-file.not-found', params: {} } });
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<MarkdownResourceLinkProvider handler={{ openLocalFile }}><ReadonlyMarkdownWorkspaceViewer documentKey="run:links" value="[report](report.md)" /></MarkdownResourceLinkProvider>));
      await act(async () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'link')!.click());
      expect(openLocalFile).toHaveBeenCalledWith('report.md', undefined);
      expect(container.querySelector('[role="alert"]')).not.toBeNull();
    } finally { await act(async () => root.unmount()); container.remove(); }
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
