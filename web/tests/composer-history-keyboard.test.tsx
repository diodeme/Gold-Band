/** @vitest-environment jsdom */
import React, { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useComposerHistory } from '@/hooks/useComposerHistory';
import { PromptInput, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import type { ComposerHistorySource, HistoryText } from '@/lib/composer-history';

let source: ComposerHistorySource;
let root: Root;
let host: HTMLDivElement;
let scope = 'session-a';
let occupied = false;
let disabled = false;
let current: ReturnType<typeof useComposerHistory>;
const submit = vi.fn();

function Harness() {
  const [input, setInput] = useState('');
  current = useComposerHistory({ scope, source, input, onChange: setInput, occupied, disabled });
  return <PromptInput value={input} onValueChange={current.onChange} onSubmit={submit}>
    <PromptInputTextarea onKeyDown={current.onKeyDown} onCompositionStart={current.onCompositionStart} onCompositionEnd={current.onCompositionEnd} />
  </PromptInput>;
}
const textarea = () => host.querySelector('textarea')!;
async function press(key: string, extra: KeyboardEventInit = {}) {
  let event!: KeyboardEvent;
  await act(async () => {
    event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra });
    textarea().dispatchEvent(event);
  });
  return event;
}
async function render() { await act(async () => root.render(<Harness />)); }

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  scope = 'session-a'; occupied = false; disabled = false;
  submit.mockReset();
  source = {
    list: vi.fn(async ({ cursor, direction }) => {
      const positions = [3, 2, 1].filter((position) => !cursor || (direction === 'older' ? position < cursor.position : position > cursor.position));
      if (direction === 'newer') positions.reverse();
      return { items: positions.map((position) => ({ cursor: { generation: 1, position, messageId: String(position) }, textBytes: 10 })),
        head: { generation: 1, position: 3, messageId: '3' }, nextCursor: null };
    }),
    text: vi.fn(async (cursor) => ({ cursor, text: `hello${cursor.position}` })),
  };
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
  await render();
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

it('loads lazily, wraps oldest to newest, and restores empty after newest', async () => {
  expect(source.list).not.toHaveBeenCalled();
  for (const expected of ['hello3', 'hello2', 'hello1', 'hello3']) {
    expect((await press('ArrowUp')).defaultPrevented).toBe(true);
    expect(textarea().value).toBe(expected);
  }
  await press('ArrowDown'); expect(textarea().value).toBe('');
  expect(current.browsing).toBe(false);
  await press('ArrowDown'); expect(textarea().value).toBe('');
  expect(submit).not.toHaveBeenCalled();
});

it('keeps edited text as a draft and Escape only clears a browsing draft', async () => {
  await press('ArrowUp');
  await act(async () => current.onChange('edited'));
  expect((await press('ArrowUp')).defaultPrevented).toBe(false);
  await press('Escape'); expect(textarea().value).toBe('edited');
  await act(async () => current.onChange(''));
  await press('ArrowUp'); await press('Escape'); expect(textarea().value).toBe('');
});

it('reserves selection, modifiers, IME, and multiline interior for native editing', async () => {
  await press('ArrowUp', { ctrlKey: true }); expect(textarea().value).toBe('');
  await press('ArrowUp', { isComposing: true }); expect(textarea().value).toBe('');
  await press('Enter', { isComposing: true }); expect(submit).not.toHaveBeenCalled();
  vi.mocked(source.text).mockImplementation(async (cursor) => ({ cursor, text: `line${cursor.position}\nend` }));
  await press('ArrowUp'); expect(textarea().selectionStart).toBe(0);
  textarea().setSelectionRange(2, 2);
  expect((await press('ArrowUp')).defaultPrevented).toBe(false);
  expect((await press('ArrowDown')).defaultPrevented).toBe(false);
  textarea().setSelectionRange(0, 3);
  expect((await press('ArrowUp')).defaultPrevented).toBe(false);
  textarea().setSelectionRange(0, 0);
  await press('ArrowUp'); expect(textarea().value).toBe('line2\nend');
  textarea().setSelectionRange(textarea().value.length, textarea().value.length);
  await press('ArrowDown'); expect(textarea().value).toBe('line3\nend');
  expect(textarea().selectionStart).toBe(textarea().value.length);
});

it.each(['edit', 'scope', 'attachment', 'disabled', 'escape', 'composition', 'submit'] as const)('ignores a pending result after %s', async (cause) => {
  let resolve!: (value: HistoryText) => void;
  vi.mocked(source.text).mockImplementation((cursor) => new Promise((done) => { resolve = (value) => done({ ...value, cursor }); }));
  await press('ArrowUp');
  await press('ArrowUp', { repeat: true }); expect(source.text).toHaveBeenCalledTimes(1);
  if (cause === 'edit') await act(async () => current.onChange('new draft'));
  if (cause === 'scope') { scope = 'session-b'; await render(); }
  if (cause === 'attachment') { occupied = true; await render(); }
  if (cause === 'disabled') { disabled = true; await render(); }
  if (cause === 'escape') await press('Escape');
  if (cause === 'composition') await act(async () => current.onCompositionStart());
  if (cause === 'submit') await act(async () => current.reset());
  await act(async () => resolve({ cursor: { generation: 1, position: 3, messageId: '3' }, text: 'late' }));
  expect(textarea().value).toBe(cause === 'edit' ? 'new draft' : '');
  expect(current.busy).toBe(false);
});

it('blocks recall with attachments or quotes and reports failures without replacing input', async () => {
  occupied = true; await render();
  await press('ArrowUp'); expect(source.list).not.toHaveBeenCalled();
  occupied = false; await render();
  vi.mocked(source.list).mockRejectedValueOnce({ code: 'acp.composer-history-stale' });
  await press('ArrowUp'); expect(current.error).toBe(true); expect(textarea().value).toBe('');
  await press('ArrowUp'); expect(textarea().value).toBe('hello3'); expect(current.error).toBe(false);
});
