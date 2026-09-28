/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { EditorView } from '@codemirror/view';
import { EditorView as EditorViewClass } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ReadonlyUnifiedDiff } from '@/components/workspace/files/ReadonlyUnifiedDiff';

let cleanup = async () => {};
afterEach(async () => {
  await cleanup();
  vi.unstubAllGlobals();
});

/** `a / -old / +new / c`: the removed block renders before line 2, at position 2. */
const BEFORE = 'a\nold\nc';
const AFTER = 'a\nnew\nc';
const REMOVED_BLOCK_POS = 2;
const LAST_LINE_POS = 6;

async function mountDiff() {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  cleanup = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  let view: EditorView | null = null;
  await act(async () => root.render(
    <ReadonlyUnifiedDiff
      comparison={{ path: 'a.txt', before: { content: BEFORE }, after: { content: AFTER } } as never}
      ariaLabel="diff"
      onCreateEditor={(created) => { view = created; }}
    />,
  ));
  return view!;
}

/**
 * The selection the editor makes for a drag pressed at `anchor` and moved over `over`. jsdom has no
 * layout, so coordinates resolve through the stubbed `posAtCoords` (press first, then pointer).
 */
function drag(view: EditorView, { anchor, pointer = anchor, over, detail = 1 }: {
  anchor: number;
  pointer?: number;
  over: Element;
  detail?: number;
}) {
  view.posAtCoords = vi.fn().mockReturnValueOnce(anchor).mockReturnValue(pointer) as never;
  const press = new MouseEvent('mousedown', { button: 0, detail });
  const style = view.state.facet(EditorViewClass.mouseSelectionStyle)
    .map((makeStyle) => makeStyle(view, press))
    .find((candidate) => candidate != null);
  if (!style) return null;
  const move = new MouseEvent('mousemove');
  Object.defineProperty(move, 'target', { value: over });
  const { anchor: from, head } = style.get(move, false, false).main;
  return { anchor: from, head };
}

describe('read-only unified diff mouse selection', () => {
  it('selects the whole removed block when a drag moves into it', async () => {
    const view = await mountDiff();
    const removedLine = view.dom.querySelector('.cm-deletedChunk .cm-deletedLine')!;

    // Down from line 1: the head lands after the block (start of the line it precedes).
    expect(drag(view, { anchor: 0, over: removedLine })).toEqual({ anchor: 0, head: REMOVED_BLOCK_POS });
    // Up from the last line: the head lands before the block (end of the line above it).
    expect(drag(view, { anchor: LAST_LINE_POS, over: removedLine }))
      .toEqual({ anchor: LAST_LINE_POS, head: REMOVED_BLOCK_POS - 1 });
  });

  it('follows the pointer over document lines and leaves word and line selection to the editor', async () => {
    const view = await mountDiff();
    const lastLine = [...view.dom.querySelectorAll('.cm-line')].at(-1)!;

    expect(drag(view, { anchor: 0, pointer: 5, over: lastLine })).toEqual({ anchor: 0, head: 5 });
    expect(drag(view, { anchor: 0, over: lastLine, detail: 2 })).toBeNull();
  });
});
