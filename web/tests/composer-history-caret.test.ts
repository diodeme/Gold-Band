/** @vitest-environment jsdom */
import { afterEach, expect, it, vi } from 'vitest';
import { isComposerHistoryBoundary } from '@/lib/composer-history-caret';

const hosts: HTMLTextAreaElement[] = [];

function field(value: string, start: number, end = start) {
  const element = document.createElement('textarea');
  element.value = value;
  document.body.append(element);
  element.setSelectionRange(start, end);
  hosts.push(element);
  return element;
}

afterEach(() => {
  while (hosts.length) hosts.pop()?.remove();
});

it('keeps hard-line interiors for native movement without measuring caret geometry', () => {
  const caretTop = vi.fn(() => 0);
  expect(isComposerHistoryBoundary(field('line1\nline2', 8), 'older', { caretTop })).toBe(false);
  expect(isComposerHistoryBoundary(field('line1\nline2', 2), 'newer', { caretTop })).toBe(false);
  expect(caretTop).not.toHaveBeenCalled();
});

it('recalls history from the first hard line and restores down only on the last hard line', () => {
  expect(isComposerHistoryBoundary(field('line1\nline2', 2), 'older')).toBe(true);
  expect(isComposerHistoryBoundary(field('line1\nline2', 11), 'newer')).toBe(true);
  expect(isComposerHistoryBoundary(field('', 0), 'older')).toBe(true);
  expect(isComposerHistoryBoundary(field('hello', 3), 'older')).toBe(true);
});

it('uses visual-line geometry only for a wrapping first or last hard line', () => {
  const caretTop = vi.fn((_element: HTMLTextAreaElement, position: number) => (position < 10 ? 0 : 20));
  expect(isComposerHistoryBoundary(field('wrapped line without breaks', 4), 'older', { caretTop })).toBe(true);
  expect(isComposerHistoryBoundary(field('wrapped line without breaks', 18), 'older', { caretTop })).toBe(false);
  expect(isComposerHistoryBoundary(field('wrapped line without breaks', 4), 'newer', { caretTop })).toBe(false);
  expect(isComposerHistoryBoundary(field('wrapped line without breaks', 18), 'newer', { caretTop })).toBe(true);
  expect(caretTop).toHaveBeenCalled();
});

it('ignores an expanded selection', () => {
  expect(isComposerHistoryBoundary(field('hello', 0, 3), 'older')).toBe(false);
});
