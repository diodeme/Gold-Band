/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { EditorView } from '@codemirror/view';
import { BlockType, EditorView as EditorViewClass } from '@codemirror/view';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ReadonlyUnifiedDiff } from '@/components/workspace/files/ReadonlyUnifiedDiff';

let cleanup = async () => {};
afterEach(async () => {
  await cleanup();
  vi.unstubAllGlobals();
});

/**
 * `a / -old / +new / c`: the removed block renders before line 2 (position 2). The chunk spans from
 * the end of line 1 (1) to the end of `new` (5); line 3 is `c` at 6..7.
 */
const BEFORE = 'a\nold\nc';
const AFTER = 'a\nnew\nc';
const CHUNK = { from: 1, to: 5 };
const LAST_LINE = { from: 6, to: 7 };

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

/** A pointer over document position `pos`, or over the removed block. */
type Pointer = number | 'removed';

/** jsdom has no layout: the removed block sits below y = 1000 and x names the document position. */
function pointerEvent(type: string, pointer: Pointer, detail = 1) {
  return new MouseEvent(type, {
    button: 0,
    detail,
    clientX: pointer === 'removed' ? 0 : pointer,
    clientY: pointer === 'removed' ? 1000 : 0,
  });
}

function stubLayout(view: EditorView) {
  view.posAtCoords = vi.fn(({ x }: { x: number }) => x) as never;
  view.elementAtHeight = vi.fn((height: number) => (height >= 1000
    ? { type: BlockType.WidgetBefore, from: 2, to: 2 }
    : { type: BlockType.Text, from: 0, to: 1 })) as never;
}

/** The selection the editor makes for a press at `press` dragged to `over`; null leaves it to the editor default. */
function drag(view: EditorView, press: Pointer, over: Pointer = press, detail = 1) {
  stubLayout(view);
  const style = view.state.facet(EditorViewClass.mouseSelectionStyle)
    .map((makeStyle) => makeStyle(view, pointerEvent('mousedown', press, detail)))
    .find((candidate) => candidate != null);
  if (!style) return null;
  const { anchor, head } = style.get(pointerEvent('mousemove', over), false, false).main;
  return { anchor, head };
}

/** Copy with the editor selection shown in the DOM, as the editor draws it. */
function copy(view: EditorView) {
  const range = document.createRange();
  range.selectNodeContents(view.contentDOM);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  const setData = vi.fn();
  const event = new Event('copy', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'clipboardData', { value: { setData, clearData: vi.fn() } });
  // A read-only editor has no focus, so the event targets the scroller.
  view.scrollDOM.dispatchEvent(event);
  return setData;
}

describe('read-only unified diff selection', () => {
  it('selects the whole change for a press inside the removed block, whatever the click count', async () => {
    const view = await mountDiff();

    expect(drag(view, 'removed')).toEqual({ anchor: CHUNK.from, head: CHUNK.to });
    expect(drag(view, 'removed', 'removed', 2)).toEqual({ anchor: CHUNK.from, head: CHUNK.to });
    expect(drag(view, 'removed', 'removed', 3)).toEqual({ anchor: CHUNK.from, head: CHUNK.to });
  });

  it('keeps the whole change when a drag from the removed block leaves it', async () => {
    const view = await mountDiff();

    expect(drag(view, 'removed', LAST_LINE.to)).toEqual({ anchor: CHUNK.from, head: LAST_LINE.to });
    expect(drag(view, 'removed', 0)).toEqual({ anchor: CHUNK.to, head: 0 });
  });

  it('snaps a drag reaching the removed block or the added lines to the whole change', async () => {
    const view = await mountDiff();

    expect(drag(view, 0, 'removed')).toEqual({ anchor: 0, head: CHUNK.to });
    expect(drag(view, LAST_LINE.to, 'removed')).toEqual({ anchor: LAST_LINE.to, head: CHUNK.from });
    expect(drag(view, LAST_LINE.to, 3)).toEqual({ anchor: LAST_LINE.to, head: CHUNK.from });
  });

  it('selects unchanged lines freely and leaves their word and line selection to the editor', async () => {
    const view = await mountDiff();

    expect(drag(view, LAST_LINE.from, LAST_LINE.to)).toEqual({ anchor: LAST_LINE.from, head: LAST_LINE.to });
    expect(drag(view, 0, 1)).toEqual({ anchor: 0, head: 1 });
    expect(drag(view, 0, 0, 2)).toBeNull();
  });

  it('snaps pointer selections made outside the drag style, such as a double click on an added word', async () => {
    const view = await mountDiff();

    view.dispatch({ selection: { anchor: 2, head: 5 }, userEvent: 'select.pointer' });
    expect(view.state.selection.main).toMatchObject({ anchor: CHUNK.from, head: CHUNK.to });

    view.dispatch({ selection: { anchor: 3, head: 4 } });
    expect(view.state.selection.main).toMatchObject({ anchor: 3, head: 4 });
  });

  it('copies a selection touching a change as the same diff fragment a quote gets', async () => {
    const view = await mountDiff();
    view.dispatch({ selection: { anchor: CHUNK.from, head: LAST_LINE.to } });

    expect(copy(view)).toHaveBeenCalledWith('text/plain', '@@ -2,2 +2,2 @@\n-old\n+new\n c');
  });

  it('leaves copying unchanged lines to the editor', async () => {
    const view = await mountDiff();
    view.dispatch({ selection: { anchor: LAST_LINE.from, head: LAST_LINE.to } });

    expect(copy(view)).not.toHaveBeenCalledWith('text/plain', expect.stringContaining('@@'));
  });
});
