import { Chunk } from '@codemirror/merge';
import { Text } from '@codemirror/state';
import { describe, expect, it } from 'vitest';

import { formatDiffSelection, formatWholeFileDiff } from '@/lib/diff-quote';

const doc = (...lines: string[]) => Text.of(lines);
const numbered = (count: number, prefix = 'l') => Array.from({ length: count }, (_, index) => `${prefix}${index + 1}`);

function diff(before: Text, after: Text) {
  return { a: before, b: after, chunks: Chunk.build(before, after) };
}

describe('whole-file diff quote', () => {
  it('writes git-style hunks and merges changes whose context overlaps', () => {
    const { a, b, chunks } = diff(
      doc(...numbered(10)),
      doc('l1', 'l2', 'l3', 'l4', 'L5', 'l6', 'l7', 'l8', 'new', 'l9', 'l10'),
    );

    expect(formatWholeFileDiff(a, b, chunks)).toBe([
      '@@ -2,9 +2,10 @@',
      ' l2', ' l3', ' l4', '-l5', '+L5', ' l6', ' l7', ' l8', '+new', ' l9', ' l10',
    ].join('\n'));
  });

  it('splits distant changes into separate hunks', () => {
    const before = numbered(20);
    const after = [...before];
    after[1] = 'L2';
    after[17] = 'L18';
    const { a, b, chunks } = diff(doc(...before), doc(...after));

    const hunks = formatWholeFileDiff(a, b, chunks).split('\n').filter((line) => line.startsWith('@@'));
    expect(hunks).toEqual(['@@ -1,5 +1,5 @@', '@@ -15,6 +15,6 @@']);
  });
});

describe('diff selection quote', () => {
  const changed = diff(
    doc(...numbered(10)),
    doc('l1', 'l2', 'l3', 'l4', 'L5', 'l6', 'l7', 'l8', 'new', 'l9', 'l10'),
  );

  it('widens a selected changed line to its chunk including the removed line', () => {
    expect(formatDiffSelection(changed.a, changed.b, changed.chunks, { fromLine: 5, toLine: 5 })?.text)
      .toBe('@@ -5 +5 @@\n-l5\n+L5');
  });

  it('keeps the original line numbers for unchanged lines after an insertion', () => {
    expect(formatDiffSelection(changed.a, changed.b, changed.chunks, { fromLine: 7, toLine: 7 })?.text)
      .toBe('@@ -7 +7 @@\n l7');
    expect(formatDiffSelection(changed.a, changed.b, changed.chunks, { fromLine: 10, toLine: 10 })?.text)
      .toBe('@@ -9 +10 @@\n l9');
  });

  it('carries a removal sitting inside the selection and quotes a removed block on its own', () => {
    const removed = diff(doc('x1', 'x2', 'x3', 'x4', 'x5'), doc('x1', 'x2', 'x5'));

    expect(formatDiffSelection(removed.a, removed.b, removed.chunks, { fromLine: 2, toLine: 3 })?.text)
      .toBe('@@ -2,4 +2,2 @@\n x2\n-x3\n-x4\n x5');
    expect(formatDiffSelection(removed.a, removed.b, removed.chunks, { fromLine: 3, toLine: 2, deletedBlockLines: [3] }))
      .toEqual({ text: '@@ -3,2 +2,0 @@\n-x3\n-x4', fromLine: 3, toLine: 2 });
    expect(formatDiffSelection(removed.a, removed.b, removed.chunks, { fromLine: 1, toLine: 0, deletedBlockLines: [1] }))
      .toBeNull();
  });

  it('keeps a removed block the selection starts or ends inside', () => {
    const removed = diff(doc('x1', 'x2', 'x3', 'x4', 'x5'), doc('x1', 'x2', 'x5'));

    expect(formatDiffSelection(removed.a, removed.b, removed.chunks, { fromLine: 3, toLine: 3, deletedBlockLines: [3] })?.text)
      .toBe('@@ -3,3 +3 @@\n-x3\n-x4\n x5');
    expect(formatDiffSelection(removed.a, removed.b, removed.chunks, { fromLine: 1, toLine: 2, deletedBlockLines: [3] })?.text)
      .toBe('@@ -1,4 +1,2 @@\n x1\n x2\n-x3\n-x4');
  });
});
