/** @vitest-environment jsdom */

import React, { act } from 'react';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  restoreEditorViewportAnchor,
  WorkspaceFileEditor,
} from '@/components/workspace/files/WorkspaceFileEditor';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const originalClientHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 480 });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 800, 480));
  vi.spyOn(EditorView, 'scrollIntoView');
  Range.prototype.getClientRects = () => ({
    length: 0,
    item: () => null,
    [Symbol.iterator]: function* iterator() { return; },
  });
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 1, 18);
});

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
  if (originalClientHeight) Object.defineProperty(HTMLElement.prototype, 'clientHeight', originalClientHeight);
  else Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
});

describe('WorkspaceFileEditor target intent', () => {
  it('restores a rebuild anchor through the native CodeMirror scroll effect', () => {
    const parent = document.createElement('div');
    document.body.append(parent);
    const view = new EditorView({
      state: EditorState.create({ doc: 'first\nsecond\nthird' }),
      parent,
    });
    try {
      restoreEditorViewportAnchor(view, { position: view.state.doc.line(2).from, blockOffsetTop: -1.5 });

      expect(EditorView.scrollIntoView).toHaveBeenCalledWith(
        view.state.doc.line(2).from,
        { y: 'start', yMargin: 0 },
      );
    } finally {
      view.destroy();
    }
  });

  it('restores the same semantic viewport through preview-source-preview rebuilds', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const value = Array.from({ length: 100 }, (_, index) => `paragraph ${index + 1}`).join('\n\n');
    function ModeHarness() {
      const [mode, setMode] = React.useState<'source' | 'live-preview'>('live-preview');
      return (
        <TooltipProvider>
          <WorkspaceFileEditor
            documentKey="mode-roundtrip"
            value={value}
            editable
            language="markdown"
            highlight={false}
            contentRevision={1}
            target={null}
            targetRevision={0}
            onChange={() => undefined}
            onSave={() => undefined}
            initialStateJson={null}
            onPersistState={() => undefined}
            markdownMode={mode}
            onMarkdownModeChange={setMode}
          />
        </TooltipProvider>
      );
    }
    const switchMode = async () => {
      const button = container.querySelectorAll('button')[1];
      expect(button).toBeInstanceOf(HTMLButtonElement);
      await act(async () => button.dispatchEvent(new MouseEvent('click', { bubbles: true })));
    };
    const viewportRestoreCalls = () => vi.mocked(EditorView.scrollIntoView).mock.calls.filter(
      ([, options]) => options?.y === 'start',
    );
    try {
      await act(async () => root.render(<ModeHarness />));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 600)));

      await switchMode();
      await vi.waitFor(() => expect(viewportRestoreCalls()).toHaveLength(1), { timeout: 5_000 });
      await switchMode();
      await vi.waitFor(() => expect(viewportRestoreCalls()).toHaveLength(2), { timeout: 5_000 });

      const viewportRestores = viewportRestoreCalls();
      expect(viewportRestores).toHaveLength(2);
      expect(viewportRestores[1]).toEqual(viewportRestores[0]);
      expect(container.querySelectorAll('.cm-editor')).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('applies a CRLF document line target when the first editor view is created', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onLocationAdjusted = vi.fn();
    try {
      await act(async () => {
        root.render(
          <WorkspaceFileEditor
            documentKey="document-a"
            value={'first\r\nsecond\r\ntarget\r\nfourth'}
            editable
            language="text"
            highlight={false}
            contentRevision={1}
            target={{ line: 3, column: 1, endLine: null }}
            targetRevision={1}
            onChange={() => undefined}
            onSave={() => undefined}
            initialStateJson={null}
            onPersistState={() => undefined}
            onLocationAdjusted={onLocationAdjusted}
          />,
        );
      });
      await act(async () => new Promise((resolve) => setTimeout(resolve, 300)));

      expect(onLocationAdjusted).toHaveBeenCalledOnce();
      expect(onLocationAdjusted).toHaveBeenCalledWith(false);
      expect(container.querySelectorAll('.cm-editor')).toHaveLength(1);
      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view?.state.selection.main.anchor).toBe('first\nsecond\n'.length);
      expect(EditorView.scrollIntoView).toHaveBeenCalledWith(
        expect.objectContaining({ anchor: 'first\nsecond\n'.length }),
        { y: 'center' },
      );
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('applies a later line-link target to an already open file editor', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const value = Array.from({ length: 80 }, (_, index) => `line ${index + 1}`).join('\n');
    const sharedProps = {
      documentKey: 'readme',
      value,
      editable: true,
      language: 'markdown',
      highlight: false,
      contentRevision: 1,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
    } as const;
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...sharedProps} target={null} targetRevision={0} />
        </TooltipProvider>,
      ));
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor
            {...sharedProps}
            target={{ line: 47, column: null, endLine: null }}
            targetRevision={1}
          />
        </TooltipProvider>,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view?.state.selection.main.head).toBe(value.split('\n').slice(0, 46).join('\n').length + 1);
      expect(EditorView.scrollIntoView).toHaveBeenCalledWith(
        expect.objectContaining({ anchor: value.split('\n').slice(0, 46).join('\n').length + 1 }),
        { y: 'center' },
      );
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('replays the native line reveal when the same open-file link is clicked again', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const value = Array.from({ length: 80 }, (_, index) => `line ${index + 1}`).join('\n');
    const props = {
      documentKey: 'readme-repeat',
      value,
      editable: true,
      language: 'markdown',
      highlight: false,
      contentRevision: 1,
      target: { line: 47, column: null, endLine: null },
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
    } as const;
    try {
      await act(async () => root.render(<WorkspaceFileEditor {...props} targetRevision={1} />));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));
      const firstRevealCount = vi.mocked(EditorView.scrollIntoView).mock.calls.length;

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      if (view) view.scrollDOM.scrollTop = 0;
      await act(async () => root.render(<WorkspaceFileEditor {...props} targetRevision={2} />));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      expect(EditorView.scrollIntoView).toHaveBeenCalledTimes(firstRevealCount + 1);
      expect(view?.state.selection.main.head).toBe(view?.state.doc.line(47).from);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('applies a later line-link target while Markdown live preview is active', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onLocationAdjusted = vi.fn();
    const value = Array.from({ length: 80 }, (_, index) => `paragraph ${index + 1}`).join('\n\n');
    const sharedProps = {
      documentKey: 'readme-preview',
      value,
      editable: true,
      language: 'markdown',
      highlight: false,
      contentRevision: 1,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
      markdownMode: 'live-preview' as const,
      onLocationAdjusted,
    };
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...sharedProps} target={null} targetRevision={0} />
        </TooltipProvider>,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 600)));
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor
            {...sharedProps}
            target={{ line: 47, column: null, endLine: null }}
            targetRevision={1}
          />
        </TooltipProvider>,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 600)));

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(onLocationAdjusted).toHaveBeenCalledWith(false);
      expect(view?.state.selection.main.head).toBe(view?.state.doc.line(47).from);
      const firstRevealCount = vi.mocked(EditorView.scrollIntoView).mock.calls.length;

      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor
            {...sharedProps}
            target={{ line: 47, column: null, endLine: null }}
            targetRevision={2}
            onPersistState={() => undefined}
          />
        </TooltipProvider>,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 300)));

      const repeatedView = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(repeatedView).toBe(view);
      expect(EditorView.scrollIntoView).toHaveBeenCalledTimes(firstRevealCount + 1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('scopes consumed target revisions to the document identity', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const props = {
      editable: true,
      language: 'text',
      highlight: false,
      contentRevision: 1,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
    } as const;
    try {
      await act(async () => root.render(
        <WorkspaceFileEditor
          {...props}
          documentKey="document-a"
          value={'one\ntwo\nthree\nfour'}
          target={{ line: 4, column: 1, endLine: null }}
          targetRevision={8}
        />,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      await act(async () => root.render(
        <WorkspaceFileEditor
          {...props}
          documentKey="document-b"
          value={'alpha\nbeta\ngamma'}
          target={{ line: 2, column: 1, endLine: null }}
          targetRevision={1}
        />,
      ));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view?.state.doc.toString()).toBe('alpha\nbeta\ngamma');
      expect(view?.state.selection.main.anchor).toBe('alpha\n'.length);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
