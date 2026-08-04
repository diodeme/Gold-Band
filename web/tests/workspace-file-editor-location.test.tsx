/** @vitest-environment jsdom */

import React, { act } from 'react';
import { readFileSync } from 'node:fs';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  captureEditorViewportAnchor,
  retainWidgetViewportAnchor,
  restoreEditorViewportAnchor,
  scrollEditorViewportAnchor,
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
  it('does not reconfigure an unchanged editor policy after the view mounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const dispatch = vi.spyOn(EditorView.prototype, 'dispatch');
    const props = {
      documentKey: 'editor-policy-profile',
      value: 'content',
      language: 'text',
      highlight: false,
      contentRevision: 1,
      target: null,
      targetRevision: 0,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
    } as const;
    const editorPolicyDispatches = () => dispatch.mock.calls.filter(([spec]) => {
      if (Array.isArray(spec)) return false;
      const effect = (spec as { effects?: { value?: { extension?: unknown[] } } }).effects;
      return Array.isArray(effect?.value?.extension) && effect.value.extension.length === 2;
    });
    try {
      await act(async () => root.render(<WorkspaceFileEditor {...props} editable />));
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view).toBeDefined();
      expect(editorPolicyDispatches()).toHaveLength(0);

      await act(async () => root.render(<WorkspaceFileEditor {...props} editable={false} />));
      expect(EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement)).toBe(view);
      expect(editorPolicyDispatches()).toHaveLength(1);
      expect(view?.state.facet(EditorState.readOnly)).toBe(true);
      expect(view?.state.facet(EditorView.editable)).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not repeat the initial Markdown image profile dispatch', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const dispatch = vi.spyOn(EditorView.prototype, 'dispatch');
    const initialImages = new Map();
    const nextImages = new Map();
    const props = {
      documentKey: 'markdown-image-profile',
      value: '# Preview',
      editable: true,
      language: 'markdown',
      highlight: false,
      contentRevision: 1,
      target: null,
      targetRevision: 0,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
      markdownMode: 'live-preview' as const,
    };
    const imageProfileDispatches = () => dispatch.mock.calls.filter(([spec]) => {
      if (Array.isArray(spec)) return false;
      const effect = (spec as { effects?: { value?: { images?: unknown } } }).effects;
      return effect?.value?.images instanceof Map;
    });
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} markdownImages={initialImages} />
        </TooltipProvider>,
      ));
      await vi.waitFor(() => expect(container.querySelector('.cm-editor')).not.toBeNull(), { timeout: 5_000 });
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view).toBeDefined();
      expect(imageProfileDispatches()).toHaveLength(0);

      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} markdownImages={nextImages} />
        </TooltipProvider>,
      ));
      expect(EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement)).toBe(view);
      expect(imageProfileDispatches()).toHaveLength(1);
      expect(imageProfileDispatches()[0]?.[0]).toMatchObject({
        effects: { value: { images: nextImages } },
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores a rebuild anchor through the native CodeMirror scroll effect', () => {
    const parent = document.createElement('div');
    document.body.append(parent);
    const view = new EditorView({
      state: EditorState.create({ doc: 'first\nsecond\nthird' }),
      parent,
    });
    try {
      restoreEditorViewportAnchor(view, {
        position: view.state.doc.line(2).from,
        blockOffsetTop: -1.5,
        blockRange: { from: view.state.doc.line(2).from, to: view.state.doc.line(2).to },
        widgetRange: null,
        widgetAnchor: null,
      });

      expect(EditorView.scrollIntoView).toHaveBeenCalledWith(
        view.state.doc.line(2).from,
        { y: 'start', yMargin: 0 },
      );
    } finally {
      view.destroy();
    }
  });

  it('retains a table-row anchor when the source viewport remains on the same row', () => {
    const remembered = {
      position: 50,
      blockOffsetTop: 0,
      blockRange: { from: 10, to: 90 },
      widgetRange: { from: 10, to: 90 },
      widgetAnchor: { kind: 'markdown-table-row' as const, rowIndex: 2, rowProgress: 0.5 },
    };
    const view = {
      state: EditorState.create({ doc: 'x'.repeat(100) }),
      lineBlockAt: () => ({ height: 20 }),
    } as unknown as EditorView;

    expect(retainWidgetViewportAnchor(view, {
      position: 40,
      blockOffsetTop: -1.625,
      blockRange: { from: 40, to: 60 },
      widgetRange: null,
      widgetAnchor: null,
    }, remembered)).toBe(remembered);
    expect(retainWidgetViewportAnchor(view, {
      position: 61,
      blockOffsetTop: 0,
      blockRange: { from: 61, to: 70 },
      widgetRange: null,
      widgetAnchor: null,
    }, remembered)).not.toBe(remembered);
  });

  it('maps the viewport inside a replacement widget to its source range', () => {
    const elementAtHeight = vi.fn(() => ({
      from: 100,
      to: 200,
      top: 200,
      height: 600,
      widget: {},
    }));
    const anchor = captureEditorViewportAnchor({
      scrollDOM: { getBoundingClientRect: () => new DOMRect(0, 0, 800, 480) },
      documentTop: -500,
      elementAtHeight,
      state: { doc: { length: 1_000 } },
    } as unknown as EditorView);

    expect(elementAtHeight).toHaveBeenCalledWith(501);
    expect(anchor).toEqual({
      position: 150,
      blockOffsetTop: 0,
      blockRange: { from: 100, to: 200 },
      widgetRange: { from: 100, to: 200 },
      widgetAnchor: null,
    });
  });

  it('maps an Atomic table viewport to its semantic Markdown row', () => {
    const state = EditorState.create({
      doc: '# Title\n\n| H | V |\n| --- | --- |\n| one | first |\n| two | second row |',
    });
    const tableRange = {
      from: state.doc.line(3).from,
      to: state.doc.line(6).to,
    };
    const content = document.createElement('div');
    const atomicTable = document.createElement('div');
    atomicTable.className = 'cm-atomic-table';
    const table = document.createElement('table');
    const head = table.createTHead();
    head.insertRow();
    const body = table.createTBody();
    body.insertRow();
    body.insertRow();
    atomicTable.append(table);
    content.append(atomicTable);
    const rows = atomicTable.querySelectorAll('tr');
    Object.defineProperty(rows[0], 'getBoundingClientRect', {
      configurable: true,
      value: () => new DOMRect(0, -199, 800, 99),
    });
    Object.defineProperty(rows[1], 'getBoundingClientRect', {
      configurable: true,
      value: () => new DOMRect(0, -99, 800, 99),
    });
    Object.defineProperty(rows[2], 'getBoundingClientRect', {
      configurable: true,
      value: () => new DOMRect(0, 0, 800, 100),
    });
    const anchor = captureEditorViewportAnchor({
      scrollDOM: { getBoundingClientRect: () => new DOMRect(0, 0, 800, 480) },
      documentTop: -200,
      domAtPos: () => ({ node: content, offset: 0 }),
      elementAtHeight: () => ({
        ...tableRange,
        top: 0,
        height: 400,
        widget: {},
      }),
      state,
    } as unknown as EditorView);

    expect(anchor).toEqual({
      position: state.doc.line(6).from,
      blockOffsetTop: 0,
      blockRange: tableRange,
      widgetRange: tableRange,
      widgetAnchor: {
        kind: 'markdown-table-row',
        rowIndex: 2,
        rowProgress: 0.01,
      },
    });
  });

  it('restores widget progress from the measured viewport block instead of a stale position block', () => {
    const scrollDOM = {
      scrollTop: 100,
      scrollHeight: 2_000,
      clientHeight: 480,
      getBoundingClientRect: () => new DOMRect(0, 0, 800, 480),
    };
    const view = {
      state: { doc: { length: 1_000 } },
      scaleY: 1,
      documentTop: -100,
      scrollDOM,
      viewportLineBlocks: [{
        from: 100,
        to: 200,
        top: 200,
        height: 800,
        widget: {},
        type: 3,
      }],
      lineBlockAt: () => ({
        from: 100,
        to: 200,
        top: 20,
        height: 20,
        widget: {},
        type: 3,
      }),
    } as unknown as EditorView;

    scrollEditorViewportAnchor(view, {
      position: 150,
      blockOffsetTop: 0,
      blockRange: { from: 100, to: 200 },
      widgetRange: null,
      widgetAnchor: null,
    });

    expect(scrollDOM.scrollTop).toBe(600);
  });

  it('reconfigures preview-source-preview on one view after predecoding visible images', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const decode = vi.fn().mockResolvedValue(undefined);
    const originalDecode = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'decode');
    Object.defineProperty(HTMLImageElement.prototype, 'decode', { configurable: true, value: decode });
    const value = `![Diagram](diagram.png)\n\n${Array.from({ length: 100 }, (_, index) => `paragraph ${index + 1}`).join('\n\n')}`;
    const markdownImages = new Map([['diagram.png', {
      kind: 'ready' as const,
      rawSrc: 'diagram.png',
      canonicalPath: 'D:/repo/diagram.png',
      previewGrant: { token: 'preview-mode-roundtrip', expiresAtMs: String(Date.now() + 300_000) },
      mimeType: 'image/png',
      width: 640,
      height: 360,
      animated: false,
    }]]);
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
            markdownImages={markdownImages}
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
      const originalView = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);

      await switchMode();
      await vi.waitFor(() => expect(viewportRestoreCalls()).toHaveLength(1), { timeout: 5_000 });
      expect(EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement)).toBe(originalView);
      await switchMode();
      await vi.waitFor(() => expect(viewportRestoreCalls()).toHaveLength(2), { timeout: 5_000 });

      const viewportRestores = viewportRestoreCalls();
      expect(viewportRestores).toHaveLength(2);
      expect(viewportRestores[1]).toEqual(viewportRestores[0]);
      expect(decode).toHaveBeenCalledOnce();
      expect(EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement)).toBe(originalView);
      expect(container.querySelectorAll('.cm-editor')).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
      if (originalDecode) Object.defineProperty(HTMLImageElement.prototype, 'decode', originalDecode);
      else Reflect.deleteProperty(HTMLImageElement.prototype, 'decode');
    }
  });

  it('restores the real todo table after preview-source-preview mode changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const value = readFileSync('docs/gold-band/开发计划/功能点todo列表.md', 'utf8');
    function ModeHarness() {
      const [mode, setMode] = React.useState<'source' | 'live-preview'>('live-preview');
      return (
        <TooltipProvider>
          <WorkspaceFileEditor
            documentKey="todo-table-roundtrip"
            value={value}
            editable
            language="markdown"
            highlight
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
    try {
      await act(async () => root.render(<ModeHarness />));
      await vi.waitFor(() => expect(container.querySelector('.cm-atomic-table table')).not.toBeNull(), { timeout: 5_000 });
      const originalView = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);

      await switchMode();
      await vi.waitFor(() => expect(container.querySelector('.cm-atomic-table table')).toBeNull(), { timeout: 5_000 });
      await switchMode();
      await vi.waitFor(() => expect(container.querySelector('.cm-atomic-table table')).not.toBeNull(), { timeout: 5_000 });

      expect(EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement)).toBe(originalView);
      expect(container.textContent).toContain('SKILL 多 Agent 实例级管理');
    } finally {
      await act(async () => root.unmount());
    }
  }, 20_000);

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
