/** @vitest-environment jsdom */

import React, { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { search, searchKeymap } from '@codemirror/search';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ConversationSearchDialog } from '@/components/conversation/ConversationSearchDialog';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { TooltipProvider } from '@/components/ui/tooltip';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/api', () => ({
  searchConversationTasks: vi.fn(async () => []),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const roots: Root[] = [];
const editors: EditorView[] = [];
const originalScrollIntoView = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollIntoView');

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, value: () => {} });
});

afterEach(async () => {
  for (const editor of editors.splice(0)) editor.destroy();
  for (const root of roots.splice(0)) await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  if (originalScrollIntoView) Object.defineProperty(Element.prototype, 'scrollIntoView', originalScrollIntoView);
  else Reflect.deleteProperty(Element.prototype, 'scrollIntoView');
});

async function renderSearch({ initialOpen = false, otherDialog = false, otherOverlay }: {
  initialOpen?: boolean;
  otherDialog?: boolean;
  otherOverlay?: React.ReactNode;
} = {}) {
  const onOpenChange = vi.fn();
  function Harness() {
    const [open, setOpen] = useState(initialOpen);
    return (
      <TooltipProvider>
        <textarea aria-label="composer" />
        <ConversationSearchDialog
          open={open}
          onOpenChange={(next) => {
            onOpenChange(next);
            setOpen(next);
          }}
          onSelectResult={() => {}}
        />
        {otherDialog && (
          <Dialog open>
            <DialogContent aria-describedby={undefined}>
              <DialogTitle>Other dialog</DialogTitle>
              <input aria-label="other dialog input" />
            </DialogContent>
          </Dialog>
        )}
        {otherOverlay}
      </TooltipProvider>
    );
  }
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  roots.push(root);
  await act(async () => root.render(<Harness />));
  return { root, container, onOpenChange };
}

function searchInput() {
  return document.querySelector<HTMLInputElement>('input[placeholder="conversation.search.placeholder"]');
}

async function pressSearch(target: EventTarget, init: KeyboardEventInit = { ctrlKey: true }) {
  const event = new KeyboardEvent('keydown', {
    key: 'k',
    code: 'KeyK',
    bubbles: true,
    cancelable: true,
    ...init,
  });
  await act(async () => { target.dispatchEvent(event); });
  return event;
}

describe('conversation search keyboard entry', () => {
  it.each([
    ['Ctrl+K', { ctrlKey: true }],
    ['Cmd+K', { metaKey: true }],
  ] as const)('opens and focuses the existing search with %s from the composer', async (_label, modifier) => {
    const { container, onOpenChange } = await renderSearch();
    const composer = container.querySelector('textarea')!;
    composer.focus();

    const event = await pressSearch(composer, modifier);

    expect(searchInput()).not.toBeNull();
    expect(document.activeElement).toBe(searchInput());
    expect(event.defaultPrevented).toBe(true);
    expect(onOpenChange).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('refocuses the open search without clearing its query or reopening the dialog', async () => {
    const { onOpenChange } = await renderSearch({ initialOpen: true });
    const input = searchInput()!;
    const setInputValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
    await act(async () => {
      setInputValue.call(input, 'existing query');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const closeButton = document.querySelector<HTMLButtonElement>('[data-slot="dialog-close"]')!;
    closeButton.focus();

    const event = await pressSearch(closeButton);

    expect(document.activeElement).toBe(input);
    expect(input.value).toBe('existing query');
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);
    expect(event.defaultPrevented).toBe(true);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('closes with Escape and returns keyboard focus to the initiating composer', async () => {
    const { container } = await renderSearch();
    const composer = container.querySelector('textarea')!;
    composer.focus();
    await pressSearch(composer);
    expect(searchInput()).not.toBeNull();

    await act(async () => {
      searchInput()!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });

    expect(searchInput()).toBeNull();
    await vi.waitFor(() => expect(document.activeElement).toBe(composer));
  });

  it.each([
    ['plain K', { ctrlKey: false }],
    ['Ctrl+F', { ctrlKey: true, key: 'f', code: 'KeyF' }],
    ['Cmd+F', { metaKey: true, key: 'f', code: 'KeyF' }],
    ['Ctrl+Shift+K', { ctrlKey: true, shiftKey: true, key: 'K' }],
    ['Ctrl+Alt+K', { ctrlKey: true, altKey: true }],
    ['IME composition', { ctrlKey: true, isComposing: true }],
  ] as const)('does not capture %s', async (_label, modifiers) => {
    const { container, onOpenChange } = await renderSearch();
    const event = await pressSearch(container.querySelector('textarea')!, modifiers);

    expect(searchInput()).toBeNull();
    expect(event.defaultPrevented).toBe(false);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('does not override a shortcut already handled by the focused control', async () => {
    const { container, onOpenChange } = await renderSearch();
    const composer = container.querySelector('textarea')!;
    composer.addEventListener('keydown', (event) => event.preventDefault(), { once: true });

    const event = await pressSearch(composer);

    expect(event.defaultPrevented).toBe(true);
    expect(searchInput()).toBeNull();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  function mountEditor(container: HTMLElement) {
    const editor = new EditorView({
      parent: container,
      state: EditorState.create({
        doc: 'Find inside this file',
        extensions: [search(), keymap.of(searchKeymap)],
      }),
    });
    editors.push(editor);
    editor.focus();
    return editor;
  }

  it('leaves Ctrl+F to CodeMirror file search', async () => {
    const { container, onOpenChange } = await renderSearch();
    const editor = mountEditor(container);

    const event = await pressSearch(editor.contentDOM, { ctrlKey: true, key: 'f', code: 'KeyF' });

    expect(event.defaultPrevented).toBe(true);
    expect(container.querySelector('.cm-search')).not.toBeNull();
    expect(searchInput()).toBeNull();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('opens conversation search with Ctrl+K from a focused CodeMirror editor', async () => {
    const { container, onOpenChange } = await renderSearch();
    const editor = mountEditor(container);

    const event = await pressSearch(editor.contentDOM);

    expect(event.defaultPrevented).toBe(true);
    expect(container.querySelector('.cm-search')).toBeNull();
    expect(document.activeElement).toBe(searchInput());
    expect(onOpenChange).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('does not stack search over another modal dialog', async () => {
    const { onOpenChange } = await renderSearch({ otherDialog: true });
    const input = document.querySelector<HTMLInputElement>('input[aria-label="other dialog input"]')!;
    input.focus();

    const event = await pressSearch(input);

    expect(searchInput()).toBeNull();
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(document.activeElement).toBe(input);
    expect(event.defaultPrevented).toBe(false);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it.each([
    ['menu', 'menuitem', (
      <DropdownMenu defaultOpen>
        <DropdownMenuTrigger>Help</DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem>Release notes</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    )],
    ['listbox', 'option', (
      <Select defaultOpen defaultValue="english">
        <SelectTrigger aria-label="Language"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="english">English</SelectItem>
          <SelectItem value="chinese">中文</SelectItem>
        </SelectContent>
      </Select>
    )],
  ] as const)('does not stack search over a focused %s', async (role, itemRole, otherOverlay) => {
    const { onOpenChange } = await renderSearch({ otherOverlay });
    const overlay = document.querySelector<HTMLElement>(`[role="${role}"]`)!;
    const item = overlay.querySelector<HTMLElement>(`[role="${itemRole}"]`)!;
    await act(async () => item.focus());
    expect(document.activeElement).toBe(item);

    const event = await pressSearch(item);

    expect(searchInput()).toBeNull();
    expect(document.activeElement).toBe(item);
    expect(overlay.closest('[aria-hidden="true"]')).toBeNull();
    expect(event.defaultPrevented).toBe(false);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('removes the shortcut listener when its owner unmounts', async () => {
    const { root, onOpenChange } = await renderSearch();
    await act(async () => root.unmount());
    roots.splice(roots.indexOf(root), 1);

    const event = await pressSearch(window);

    expect(event.defaultPrevented).toBe(false);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it('handles each shortcut once after unmounting and remounting', async () => {
    const previous = await renderSearch();
    await act(async () => previous.root.unmount());
    roots.splice(roots.indexOf(previous.root), 1);
    const current = await renderSearch();

    await pressSearch(current.container.querySelector('textarea')!);

    expect(searchInput()).not.toBeNull();
    expect(document.activeElement).toBe(searchInput());
    expect(current.onOpenChange).toHaveBeenCalledExactlyOnceWith(true);
    expect(previous.onOpenChange).not.toHaveBeenCalled();
  });
});
