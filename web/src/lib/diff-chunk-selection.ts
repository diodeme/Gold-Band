import { getChunks, getOriginalDoc, type Chunk } from '@codemirror/merge';
import {
  EditorSelection,
  EditorState,
  Transaction,
  type SelectionRange,
  type Text,
} from '@codemirror/state';
import { BlockType, EditorView, ViewPlugin } from '@codemirror/view';

import { formatDiffSelection, type DiffQuoteFragment } from '@/lib/diff-quote';

/**
 * Where a change sits in a unified merge view's document. Removed lines are a block widget with no
 * positions of their own, so a change reaches back over the line break before it: `[from, to]` is
 * what selecting the change covers, `[from, end)` the characters that belong to it.
 */
interface ChunkSpan {
  from: number;
  to: number;
  end: number;
}

function chunkSpan(chunk: Chunk, doc: Text): ChunkSpan {
  const removes = chunk.fromA < chunk.toA;
  const from = removes && chunk.fromB > 0 ? chunk.fromB - 1 : chunk.fromB;
  if (chunk.toB > chunk.fromB) {
    return { from, to: Math.min(chunk.toB - 1, doc.length), end: Math.min(chunk.toB, doc.length) };
  }
  if (chunk.fromB > 0) return { from, to: chunk.fromB, end: chunk.fromB };
  // Lines removed from the top have no line break before them; they travel with the first line.
  const firstLine = Math.min(doc.length, Math.max(doc.line(1).to, 1));
  return { from: 0, to: firstLine, end: firstLine };
}

function touches(span: ChunkSpan, from: number, to: number) {
  return from < span.end && to > span.from;
}

/** A non-empty range widened over every change it touches, keeping its direction. */
function snapRange(state: EditorState, range: SelectionRange) {
  const chunks = getChunks(state)?.chunks;
  if (!chunks || range.empty) return range;
  let { from, to } = range;
  for (const chunk of chunks) {
    const span = chunkSpan(chunk, state.doc);
    if (!touches(span, from, to)) continue;
    from = Math.min(from, span.from);
    to = Math.max(to, span.to);
  }
  if (from === range.from && to === range.to) return range;
  return range.head < range.anchor ? EditorSelection.range(to, from) : EditorSelection.range(from, to);
}

/** The change whose removed block is under the pointer. */
function removedChunkAt(view: EditorView, clientY: number) {
  const block = view.elementAtHeight(clientY - view.documentTop);
  if (block.type !== BlockType.WidgetBefore) return null;
  return getChunks(view.state)?.chunks
    .find((chunk) => chunk.fromA < chunk.toA && chunk.fromB === block.from) ?? null;
}

/** What the pointer at `event` selects: a removed block selects its whole change. */
function pointerRange(view: EditorView, event: MouseEvent) {
  const chunk = removedChunkAt(view, event.clientY);
  if (chunk) {
    const span = chunkSpan(chunk, view.state.doc);
    return { from: span.from, to: span.to };
  }
  const pos = view.posAtCoords({ x: event.clientX, y: event.clientY }, false);
  return { from: pos, to: pos };
}

/**
 * Removed lines have no document positions, so the browser's own selection inside them is lost the
 * next time the editor redraws its selection. The editor owns every press instead: one inside a
 * removed block, or a drag reaching one, selects the whole change. Word and line selection on
 * document lines keep the editor default.
 */
const mouseSelection = EditorView.mouseSelectionStyle.of((view, press) => {
  if (press.button !== 0) return null;
  if (press.detail > 1 && !removedChunkAt(view, press.clientY)) return null;
  let start = pointerRange(view, press);
  let extendFrom = view.state.selection.main.anchor;
  return {
    get(event, extend) {
      const origin = extend ? { from: extendFrom, to: extendFrom } : start;
      const current = pointerRange(view, event);
      const from = Math.min(origin.from, current.from);
      const to = Math.max(origin.to, current.to);
      const range = current.from < origin.from ? EditorSelection.range(to, from) : EditorSelection.range(from, to);
      return EditorSelection.create([snapRange(view.state, range)]);
    },
    update(update) {
      if (!update.docChanged) return;
      start = { from: update.changes.mapPos(start.from), to: update.changes.mapPos(start.to) };
      extendFrom = update.changes.mapPos(extendFrom);
    },
  };
});

/** Pointer selections the drag style does not make, such as a double click on an added word, snap too. */
const snapPointerSelection = EditorState.transactionFilter.of((tr) => {
  const pointer = tr.isUserEvent('select.pointer') || tr.annotation(Transaction.userEvent) === 'select';
  if (!tr.selection || tr.docChanged || !pointer) return tr;
  const main = snapRange(tr.startState, tr.selection.main);
  return main === tr.selection.main ? tr : [tr, { selection: EditorSelection.create([main]), sequential: true }];
});

/**
 * Copying a selection that touches a change yields the diff fragment a quote would, removed lines
 * included. A read-only editor never takes focus, so the copy event targets the scroller rather than
 * the content: listen on the whole editor.
 */
const copyDiffFragment = ViewPlugin.define((view) => {
  const copy = (event: ClipboardEvent) => {
    const selection = view.dom.ownerDocument.getSelection();
    if (!selection?.anchorNode || !view.contentDOM.contains(selection.anchorNode)) return;
    const fragment = diffSelectionFragment(view.state, { changesOnly: true });
    if (!fragment || !event.clipboardData) return;
    event.clipboardData.setData('text/plain', fragment.text);
    event.preventDefault();
  };
  view.dom.addEventListener('copy', copy);
  return { destroy: () => view.dom.removeEventListener('copy', copy) };
});

/** Selection for a read-only unified merge view: unchanged lines select freely, a change only as a whole. */
export const unifiedDiffSelection = [mouseSelection, snapPointerSelection, copyDiffFragment];

/**
 * The main selection of a unified merge view as a diff fragment. `changesOnly` gives null when it
 * touches no change.
 */
export function diffSelectionFragment(
  state: EditorState,
  { changesOnly = false } = {},
): DiffQuoteFragment | null {
  const chunks = getChunks(state)?.chunks;
  const { from, to } = state.selection.main;
  if (!chunks || from === to) return null;
  const { doc } = state;
  const touched = chunks.filter((chunk) => touches(chunkSpan(chunk, doc), from, to));
  if (changesOnly && touched.length === 0) return null;
  const start = doc.lineAt(from);
  const end = doc.lineAt(to);
  return formatDiffSelection(getOriginalDoc(state), doc, chunks, {
    // A selection from a line end starts on the next line; one ending at a line start stops before it.
    fromLine: from === start.to ? start.number + 1 : start.number,
    toLine: to === end.from ? end.number - 1 : end.number,
    deletedBlockLines: touched
      .filter((chunk) => chunk.fromA < chunk.toA)
      .map((chunk) => doc.lineAt(chunk.fromB).number),
  });
}
