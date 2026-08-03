/** @vitest-environment jsdom */

import React, { act } from 'react';
import { EditorView } from '@codemirror/view';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { WorkspaceFileEditor } from '@/components/workspace/files/WorkspaceFileEditor';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const originalClientHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 480 });
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
  it('applies a line target after the first editor view is mounted and measured', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onLocationAdjusted = vi.fn();
    try {
      await act(async () => {
        root.render(
          <WorkspaceFileEditor
            documentKey="document-a"
            value={'first\nsecond\ntarget\nfourth'}
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
      await act(async () => new Promise((resolve) => setTimeout(resolve, 120)));

      expect(onLocationAdjusted).toHaveBeenCalledOnce();
      expect(onLocationAdjusted).toHaveBeenCalledWith(false);
      expect(container.querySelectorAll('.cm-editor')).toHaveLength(1);
      const view = EditorView.findFromDOM(container.querySelector('.cm-editor') as HTMLElement);
      expect(view?.state.selection.main.anchor).toBe('first\nsecond\n'.length);
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
