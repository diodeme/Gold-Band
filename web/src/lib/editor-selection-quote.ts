import { getChunks, getOriginalDoc } from '@codemirror/merge';
import type { EditorView } from '@codemirror/view';

import type { QuotableSelection } from '@/components/conversation/SelectionQuoteButton';
import { formatDiffSelection, type DiffQuoteFragment } from '@/lib/diff-quote';

export interface EditorLineSelection {
  text: string;
  startLine: number;
  endLine: number;
}

const DELETED_BLOCK_SELECTOR = '.cm-deletedChunk';

/** The DOM selection when it lies inside the editor content; read-only views never hold focus. */
function contentSelectionRange(view: EditorView) {
  const selection = view.dom.ownerDocument.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0 || !selection.toString().trim()) return null;
  const range = selection.getRangeAt(0);
  return view.contentDOM.contains(range.commonAncestorContainer) ? range : null;
}

/** Last line a selection covers; an end at the start of a line leaves that line out. */
function endLineOf(view: EditorView, from: number, to: number) {
  const line = view.state.doc.lineAt(to);
  return to > from && to === line.from ? line.number - 1 : line.number;
}

/** The selected source text plus the 1-based lines it spans. */
export function readEditorLineSelection(view: EditorView): QuotableSelection<EditorLineSelection> | null {
  const range = contentSelectionRange(view);
  if (!range) return null;
  const from = view.posAtDOM(range.startContainer, range.startOffset);
  const to = view.posAtDOM(range.endContainer, range.endOffset);
  const text = view.state.sliceDoc(from, to);
  if (!text.trim()) return null;
  return {
    value: { text, startLine: view.state.doc.lineAt(from).number, endLine: endLineOf(view, from, to) },
    rect: range.getBoundingClientRect(),
  };
}

function deletedBlockLine(view: EditorView, node: Node) {
  const element = node instanceof Element ? node : node.parentElement;
  const block = element?.closest(DELETED_BLOCK_SELECTOR);
  // A removed block is a widget placed at the start of the line that follows it.
  return block ? view.state.doc.lineAt(view.posAtDOM(block)).number : null;
}

/**
 * The DOM selection inside a unified merge view as a diff fragment. Removed lines are widgets
 * outside the document, so the editor selection alone cannot tell whether they were selected.
 */
export function readUnifiedDiffSelection(view: EditorView): QuotableSelection<DiffQuoteFragment> | null {
  const range = contentSelectionRange(view);
  if (!range) return null;
  const chunks = getChunks(view.state)?.chunks;
  if (!chunks) return null;
  const from = view.posAtDOM(range.startContainer, range.startOffset);
  const to = view.posAtDOM(range.endContainer, range.endOffset);
  const startBlock = deletedBlockLine(view, range.startContainer);
  const endBlock = deletedBlockLine(view, range.endContainer);
  const fragment = formatDiffSelection(getOriginalDoc(view.state), view.state.doc, chunks, {
    fromLine: startBlock ?? view.state.doc.lineAt(from).number,
    toLine: endBlock != null ? endBlock - 1 : endLineOf(view, from, to),
    deletedBlockLines: [startBlock, endBlock].filter((line): line is number => line != null),
  });
  return fragment ? { value: fragment, rect: range.getBoundingClientRect() } : null;
}
