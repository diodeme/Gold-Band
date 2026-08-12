import { syntaxTree } from '@codemirror/language';
import type { EditorState, Text } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';
import type { SyntaxNode } from '@lezer/common';

export interface MarkdownTableRange {
  from: number;
  to: number;
}

export interface MarkdownTableRowViewportAnchor {
  kind: 'markdown-table-row';
  rowIndex: number;
  rowProgress: number;
}

interface CapturedMarkdownTableRow {
  anchor: MarkdownTableRowViewportAnchor;
  position: number;
}

const clampProgress = (value: number) => Math.min(1, Math.max(0, value));

function tableRows(table: HTMLElement) {
  return Array.from(table.querySelectorAll<HTMLElement>('thead > tr, tbody > tr'));
}

function tableElementAtRange(view: EditorView, range: MarkdownTableRange) {
  if (typeof view.domAtPos !== 'function') return null;
  const { node, offset } = view.domAtPos(range.from, 1);
  if (!(node instanceof Element)) return null;
  const adjacent = node.childNodes[offset];
  if (adjacent instanceof HTMLElement && adjacent.matches('.cm-atomic-table')) return adjacent;
  if (adjacent instanceof Element) {
    const nested = adjacent.querySelector<HTMLElement>('.cm-atomic-table');
    if (nested) return nested;
  }
  return node.matches('.cm-atomic-table')
    ? node as HTMLElement
    : node.querySelector<HTMLElement>('.cm-atomic-table');
}

function sourceLineForTableRow(
  doc: Text,
  range: MarkdownTableRange,
  rowIndex: number,
) {
  const firstLine = doc.lineAt(range.from);
  const lastLine = doc.lineAt(Math.max(range.from, range.to - 1));
  const requestedLine = rowIndex <= 0
    ? firstLine.number
    : firstLine.number + rowIndex + 1;
  return doc.line(Math.min(lastLine.number, requestedLine));
}

export function markdownTableRowAnchorFromSource(
  doc: Text,
  range: MarkdownTableRange,
  position: number,
  rowProgress = 0,
): CapturedMarkdownTableRow {
  const firstLine = doc.lineAt(range.from);
  const sourceLine = doc.lineAt(Math.min(Math.max(range.from, position), Math.max(range.from, range.to - 1)));
  const relativeLine = sourceLine.number - firstLine.number;
  const rowIndex = relativeLine <= 1 ? 0 : relativeLine - 1;
  const progress = clampProgress(rowProgress);
  return {
    anchor: { kind: 'markdown-table-row', rowIndex, rowProgress: progress },
    position: sourceLine.from + Math.floor(sourceLine.length * progress),
  };
}

export function markdownTableRangeAt(state: EditorState, position: number): MarkdownTableRange | null {
  const resolvedPosition = Math.min(state.doc.length, Math.max(0, position));
  for (const side of [1, -1] as const) {
    let node: SyntaxNode | null = syntaxTree(state).resolveInner(resolvedPosition, side);
    while (node) {
      if (node.name === 'Table') return { from: node.from, to: node.to };
      node = node.parent;
    }
  }
  return null;
}

export function captureMarkdownTableRowViewport(
  view: EditorView,
  range: MarkdownTableRange,
  screenY: number,
): CapturedMarkdownTableRow | null {
  const table = tableElementAtRange(view, range);
  if (!table) return null;
  const rows = tableRows(table);
  if (rows.length === 0) return null;
  const matchingRow = rows.findIndex((row) => screenY < row.getBoundingClientRect().bottom);
  const rowIndex = matchingRow < 0 ? rows.length - 1 : matchingRow;
  const row = rows[Math.min(rows.length - 1, rowIndex)];
  if (!row) return null;
  const rect = row.getBoundingClientRect();
  const rowProgress = rect.height > 0 ? clampProgress((screenY - rect.top) / rect.height) : 0;
  const sourceLine = sourceLineForTableRow(view.state.doc, range, rowIndex);
  return {
    anchor: { kind: 'markdown-table-row', rowIndex, rowProgress },
    position: sourceLine.from + Math.floor(sourceLine.length * rowProgress),
  };
}

export function markdownTableRowScreenPoint(
  view: EditorView,
  range: MarkdownTableRange,
  anchor: MarkdownTableRowViewportAnchor,
) {
  const table = tableElementAtRange(view, range);
  if (!table) return null;
  const rows = tableRows(table);
  const row = rows[Math.min(rows.length - 1, Math.max(0, anchor.rowIndex))];
  if (!row) return null;
  const rect = row.getBoundingClientRect();
  return rect.top + rect.height * clampProgress(anchor.rowProgress);
}
