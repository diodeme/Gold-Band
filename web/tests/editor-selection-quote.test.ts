// @vitest-environment jsdom

import { unifiedMergeView } from '@codemirror/merge';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { afterEach, describe, expect, it } from 'vitest';

import { diffQuoteSource } from '@/components/workspace/files/TurnFileWorkspacePanel';
import { readEditorLineSelection, readUnifiedDiffSelection } from '@/lib/editor-selection-quote';

const views: EditorView[] = [];

// jsdom has no layout; the readers only pass the rect through to the quote button.
Range.prototype.getBoundingClientRect ??= () => new DOMRect();

afterEach(() => {
  views.splice(0).forEach((view) => view.destroy());
  document.body.replaceChildren();
  window.getSelection()?.removeAllRanges();
});

function mount(doc: string, original?: string) {
  const parent = document.createElement('div');
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        ...(original === undefined ? [] : [unifiedMergeView({ original, mergeControls: false })]),
      ],
    }),
  });
  views.push(view);
  return view;
}

function lineText(view: EditorView, line: number) {
  const element = Array.from(view.contentDOM.querySelectorAll('.cm-line'))
    .find((node) => !node.closest('.cm-deletedChunk') && view.state.doc.lineAt(view.posAtDOM(node)).number === line);
  return element!.firstChild ?? element!;
}

function select(startNode: Node, startOffset: number, endNode: Node, endOffset: number) {
  const range = document.createRange();
  range.setStart(startNode, startOffset);
  range.setEnd(endNode, endOffset);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
}

describe('editor selection quote readers', () => {
  it('reads selected source text and its line range from a read-only editor', () => {
    const view = mount('one\ntwo\nthree');
    select(lineText(view, 2), 1, lineText(view, 3), 2);

    expect(readEditorLineSelection(view)?.value).toEqual({ text: 'wo\nth', startLine: 2, endLine: 3 });
  });

  it('ignores selections outside the editor', () => {
    const view = mount('one');
    const outside = document.createElement('p');
    outside.textContent = 'outside';
    document.body.append(outside);
    select(outside.firstChild!, 0, outside.firstChild!, 3);

    expect(readEditorLineSelection(view)).toBeNull();
  });

  it('quotes a removed block selected on its own as a removal-only hunk', () => {
    const view = mount('x1\nx2\nx5', 'x1\nx2\nx3\nx4\nx5');
    const deleted = view.dom.querySelector('.cm-deletedChunk');
    expect(deleted).not.toBeNull();
    const deletedText = deleted!.querySelector('.cm-deletedLine')!;
    select(deletedText, 0, deletedText, deletedText.childNodes.length);

    expect(readUnifiedDiffSelection(view)?.value.text).toBe('@@ -3,2 +2,0 @@\n-x3\n-x4');
  });

  it('quotes the whole change when only the removed half of a modified chunk is selected', () => {
    const view = mount('{\n  "a": 1,\n  "b": 2\n}', '{\n  "a": 1\n}');
    const deletedText = view.dom.querySelector('.cm-deletedChunk .cm-deletedLine')!;
    select(deletedText, 0, deletedText, deletedText.childNodes.length);

    expect(readUnifiedDiffSelection(view)?.value.text).toBe('@@ -2 +2,2 @@\n-  "a": 1\n+  "a": 1,\n+  "b": 2');
  });

  it('keeps a removed block when a selection starts on a line above it and ends inside it', () => {
    const view = mount('x1\nx2\nx5', 'x1\nx2\nx3\nx4\nx5');
    const deletedText = view.dom.querySelector('.cm-deletedChunk .cm-deletedLine')!;
    select(lineText(view, 2), 0, deletedText, deletedText.childNodes.length);

    expect(readUnifiedDiffSelection(view)?.value.text).toBe('@@ -2,3 +2 @@\n x2\n-x3\n-x4');
  });
});

describe('diff quote source', () => {
  it('names the Git change or agent turn a diff belongs to', () => {
    expect(diffQuoteSource('a.ts', null)).toEqual({ kind: 'diff', path: 'a.ts', origin: 'agentTurn', revision: null });
    expect(diffQuoteSource('a.ts', { kind: 'workspace', path: 'a.ts', area: 'staged' }).origin).toBe('workingTreeStaged');
    expect(diffQuoteSource('a.ts', { kind: 'commit', path: 'a.ts', afterOid: '0123456789abcdef0123' }).revision).toBe('0123456789ab');
    expect(diffQuoteSource('a.ts', {
      kind: 'github-pr', host: 'github.com', repository: 'o/r', prNumber: 7, baseOid: 'b', headOid: 'h', path: 'a.ts',
    })).toEqual({ kind: 'diff', path: 'a.ts', origin: 'pullRequest', revision: 'o/r#7' });
  });
});
