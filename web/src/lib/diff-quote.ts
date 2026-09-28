import type { Text } from '@codemirror/state';

/** Line-aligned change between an original (A) and a changed (B) document, as produced by `@codemirror/merge`. */
export interface DiffQuoteChunk {
  fromA: number;
  toA: number;
  fromB: number;
  toB: number;
}

/** Unified-diff context lines around each change in a whole-file quote, matching `git diff`. */
export const DIFF_QUOTE_CONTEXT_LINES = 3;

interface ChunkLines {
  /** First changed line (1-based); for an empty side, the line the change sits before. */
  startA: number;
  countA: number;
  startB: number;
  countB: number;
}

export interface DiffQuoteFragment {
  text: string;
  /** B-side line range the fragment covers; `toLine < fromLine` when it only removes lines. */
  fromLine: number;
  toLine: number;
}

function lineAt(doc: Text, pos: number) {
  return pos > doc.length ? doc.lines + 1 : doc.lineAt(pos).number;
}

function chunkLines(a: Text, b: Text, chunk: DiffQuoteChunk): ChunkLines {
  const startA = lineAt(a, chunk.fromA);
  const startB = lineAt(b, chunk.fromB);
  return {
    startA,
    countA: chunk.toA > chunk.fromA ? lineAt(a, chunk.toA - 1) - startA + 1 : 0,
    startB,
    countB: chunk.toB > chunk.fromB ? lineAt(b, chunk.toB - 1) - startB + 1 : 0,
  };
}

/** Whether a chunk lies wholly before B line `line`. A pure deletion sits before its `startB`. */
function chunkBefore(chunk: ChunkLines, line: number) {
  return chunk.countB === 0 ? chunk.startB <= line : chunk.startB + chunk.countB <= line;
}

function headerRange(start: number, count: number) {
  // Unified diff names the line before an empty range.
  const first = count === 0 ? start - 1 : start;
  return count === 1 ? `${first}` : `${first},${count}`;
}

/**
 * One hunk over B lines `[fromLine, toLine]` writing `hunkChunks` in full; every other B
 * line in the range is unchanged context. `allChunks` locates the A side of the range.
 */
function formatHunk(
  a: Text,
  b: Text,
  allChunks: readonly ChunkLines[],
  hunkChunks: readonly ChunkLines[],
  fromLine: number,
  toLine: number,
) {
  const first = hunkChunks[0];
  const startA = first
    ? first.startA - (first.startB - fromLine)
    : fromLine + allChunks
      .filter((chunk) => chunkBefore(chunk, fromLine))
      .reduce((delta, chunk) => delta + chunk.countA - chunk.countB, 0);
  const body: string[] = [];
  let countA = 0;
  let countB = 0;
  let lineB = fromLine;
  const context = (until: number) => {
    for (; lineB < until; lineB += 1) {
      body.push(` ${b.line(lineB).text}`);
      countA += 1;
      countB += 1;
    }
  };
  for (const chunk of hunkChunks) {
    context(chunk.startB);
    for (let line = chunk.startA; line < chunk.startA + chunk.countA; line += 1) body.push(`-${a.line(line).text}`);
    for (let line = chunk.startB; line < chunk.startB + chunk.countB; line += 1) body.push(`+${b.line(line).text}`);
    countA += chunk.countA;
    countB += chunk.countB;
    lineB = chunk.startB + chunk.countB;
  }
  context(toLine + 1);
  return [`@@ -${headerRange(startA, countA)} +${headerRange(fromLine, countB)} @@`, ...body].join('\n');
}

/** Every change as `git diff` hunks: changes close together share a hunk with context around them. */
export function formatWholeFileDiff(
  a: Text,
  b: Text,
  chunks: readonly DiffQuoteChunk[],
  context = DIFF_QUOTE_CONTEXT_LINES,
) {
  const lines = chunks.map((chunk) => chunkLines(a, b, chunk));
  const groups: ChunkLines[][] = [];
  for (const chunk of lines) {
    const group = groups.at(-1);
    const last = group?.at(-1);
    if (group && last && chunk.startB - (last.startB + last.countB) <= context * 2) group.push(chunk);
    else groups.push([chunk]);
  }
  return groups.map((group) => {
    const first = group[0]!;
    const last = group.at(-1)!;
    const fromLine = Math.max(1, first.startB - context);
    const toLine = Math.min(b.lines, last.startB + last.countB - 1 + context);
    return formatHunk(a, b, lines, group, fromLine, toLine);
  }).join('\n');
}

/** Selected B lines; the removed blocks the selection covers sit before `deletedBlockLines`' lines. */
export interface DiffQuoteSelection {
  fromLine: number;
  /** `toLine < fromLine` when the selection covers only a removed block. */
  toLine: number;
  deletedBlockLines?: readonly number[];
}

/**
 * A diff fragment for the selected B lines. A change the selection touches is widened to its
 * whole chunk, so removed lines travel with the lines that replaced them. Removed blocks have no
 * B lines of their own, so the caller names the ones the selection covers.
 */
export function formatDiffSelection(
  a: Text,
  b: Text,
  chunks: readonly DiffQuoteChunk[],
  selection: DiffQuoteSelection,
): DiffQuoteFragment | null {
  const lines = chunks.map((chunk) => chunkLines(a, b, chunk));
  const deletedBlockLines = selection.deletedBlockLines ?? [];
  let { fromLine, toLine } = selection;
  const touched = lines.filter((chunk) => (chunk.countA > 0 && deletedBlockLines.includes(chunk.startB))
    || (chunk.countB === 0
      ? chunk.startB > fromLine && chunk.startB <= toLine
      : chunk.startB <= toLine && chunk.startB + chunk.countB - 1 >= fromLine));
  if (touched.length === 0 && toLine < fromLine) return null;
  for (const chunk of touched) {
    fromLine = Math.min(fromLine, chunk.startB);
    toLine = Math.max(toLine, chunk.startB + chunk.countB - 1);
  }
  return { text: formatHunk(a, b, lines, touched, fromLine, toLine), fromLine, toLine };
}
