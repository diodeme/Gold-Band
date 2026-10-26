/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { PromptInput, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('prompt input scroll preservation', () => {
  it('resizes a long draft when the input width changes without another keystroke', async () => {
    let width = 640;
    const resizeCallbacks: (() => void)[] = [];
    const disconnect = vi.fn();
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: () => void) { resizeCallbacks.push(callback); }
      observe() {}
      disconnect = disconnect;
    });
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(() => ({ width }) as DOMRect);
    const measurement = vi.spyOn(HTMLTextAreaElement.prototype, 'scrollHeight', 'get')
      .mockReturnValue(600);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <PromptInput value={'long draft '.repeat(100)} maxHeight={null}>
          <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.inputScrollContainerClassName}>
            <PromptInputTextarea />
          </div>
        </PromptInput>,
      ));
      const textarea = container.querySelector('textarea')!;
      expect(textarea.style.height).toBe('600px');

      width = 320;
      measurement.mockReturnValue(1_200);
      await act(async () => resizeCallbacks.forEach((callback) => callback()));
      expect(textarea.style.height).toBe('1200px');

      measurement.mockClear();
      await act(async () => resizeCallbacks.forEach((callback) => callback()));
      expect(measurement).not.toHaveBeenCalled();

      width = 640;
      measurement.mockReturnValue(600);
      await act(async () => resizeCallbacks.forEach((callback) => callback()));
      expect(textarea.style.height).toBe('600px');
    } finally {
      await act(async () => root.unmount());
    }
    expect(disconnect).toHaveBeenCalledOnce();
  });

  it('keeps the measurement field out of editing, focus order and form submission', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onValueChange = vi.fn();
    try {
      await act(async () => root.render(
        <form>
          <PromptInput value="draft" onValueChange={onValueChange}>
            <PromptInputTextarea name="prompt" />
          </PromptInput>
        </form>,
      ));
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea:not([aria-hidden])')!;
      const measurement = container.querySelector<HTMLTextAreaElement>('textarea[aria-hidden="true"]')!;
      expect(measurement.readOnly).toBe(true);
      expect(measurement.tabIndex).toBe(-1);
      expect(measurement.style.visibility).toBe('hidden');
      expect([...new FormData(container.querySelector('form')!).entries()]).toEqual([['prompt', 'draft']]);
      textarea.focus();
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(textarea, 'draft+');
        textarea.dispatchEvent(new Event('input', { bubbles: true }));
      });
      expect(onValueChange).toHaveBeenCalledExactlyOnceWith('draft+');
      expect(document.activeElement).toBe(textarea);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('leaves explicitly sized textareas alone when autosize is disabled', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(
        <PromptInput value="draft">
          <PromptInputTextarea disableAutosize style={{ height: 96 }} />
        </PromptInput>,
      ));
      expect(container.querySelector('textarea')!.style.height).toBe('96px');
      expect(container.querySelectorAll('textarea')).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it.each([1_136, 400])('does not collapse the home input scroll viewport at scrollTop=%i while typing', async (initialScrollTop) => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const renderValue = async (value: string) => {
      await act(async () => {
        root.render(
          <PromptInput value={value} onValueChange={() => undefined} maxHeight={null}>
            <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.inputScrollContainerClassName}>
              <PromptInputTextarea className={CONVERSATION_HOME_COMPOSER_LAYOUT.textareaClassName} />
            </div>
          </PromptInput>,
        );
      });
    };

    try {
      await renderValue('long draft');
      const textarea = container.querySelector('textarea')!;
      const viewport = textarea.parentElement!.parentElement!;
      let scrollTop = initialScrollTop;
      let height = '1456px';
      Object.defineProperties(viewport, {
        clientHeight: { configurable: true, get: () => 320 },
        scrollHeight: { configurable: true, get: () => Number.parseFloat(height) || 48 },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => { scrollTop = value; },
        },
      });
      vi.spyOn(HTMLTextAreaElement.prototype, 'scrollHeight', 'get').mockReturnValue(1_456);
      Object.defineProperty(textarea.style, 'height', {
        configurable: true,
        get: () => height,
        set: (value: string) => {
          height = value;
          // Model native layout clamping: shrinking the content also shrinks its ancestor's range.
          scrollTop = Math.min(scrollTop, Math.max(0, viewport.scrollHeight - viewport.clientHeight));
        },
      });
      textarea.focus();
      textarea.setSelectionRange(textarea.value.length, textarea.value.length);

      await renderValue('long draft+');

      expect(viewport.scrollTop).toBe(initialScrollTop);
      expect(container.querySelector('textarea')).toBe(textarea);
      expect(document.activeElement).toBe(textarea);
      expect(textarea.selectionStart).toBe(textarea.value.length);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores the textarea scroll position around autosize measurement', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const renderValue = async (value: string) => {
      await act(async () => {
        root.render(
          <PromptInput value={value} onValueChange={() => undefined} maxHeight={320}>
            <PromptInputTextarea />
          </PromptInput>,
        );
      });
    };

    try {
      await renderValue('initial');
      const textarea = container.querySelector('textarea')!;
      let scrollTop = 1_136;
      let scrollHeight = 1_456;
      let styleHeight = '';
      vi.spyOn(HTMLTextAreaElement.prototype, 'scrollHeight', 'get').mockImplementation(() => scrollHeight);
      Object.defineProperties(textarea, {
        clientHeight: { configurable: true, get: () => 320 },
        scrollHeight: { configurable: true, get: () => scrollHeight },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => { scrollTop = value; },
        },
      });
      Object.defineProperty(textarea.style, 'height', {
        configurable: true,
        get: () => styleHeight,
        set: (value: string) => {
          styleHeight = value;
          // WebView2 can reset the textarea scroll position while measuring auto height.
          if (value === 'auto') scrollTop = 0;
        },
      });

      scrollHeight = 1_480;
      await renderValue('initial+');

      expect(scrollTop).toBe(1_160);

      scrollTop = 400;
      textarea.dispatchEvent(new Event('scroll', { bubbles: true }));
      scrollHeight = 1_520;
      await renderValue('initial++');

      expect(scrollTop).toBe(400);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
