import { useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import { ComposerHistoryReader, type ComposerHistorySource, type HistoryCursor } from '@/lib/composer-history';

interface Options {
  scope: string;
  source: ComposerHistorySource | null;
  input: string;
  occupied: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}

export function useComposerHistory(options: Options) {
  const latest = useRef(options);
  latest.current = options;
  const reader = useRef<ComposerHistoryReader | null>(null);
  const cursor = useRef<HistoryCursor | null>(null);
  const revision = useRef(0);
  const pending = useRef(false);
  const expected = useRef(options.input);
  const composing = useRef(false);
  const caret = useRef<{ element: HTMLTextAreaElement; direction: 'older' | 'newer' } | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  function reset() {
    revision.current += 1;
    reader.current = null;
    cursor.current = null;
    caret.current = null;
    pending.current = false;
    setBusy(false);
    setBrowsing(false);
    setError(false);
  }

  useLayoutEffect(() => {
    reset();
    return () => { revision.current += 1; };
  }, [options.scope, options.source]);

  useLayoutEffect(() => {
    if (options.disabled || options.occupied || options.input !== expected.current) reset();
    expected.current = options.input;
    if (caret.current) {
      const { element, direction } = caret.current;
      const position = direction === 'older' && element.value.includes('\n') ? 0 : element.value.length;
      element.setSelectionRange(position, position);
      caret.current = null;
    }
  }, [options.input, options.occupied, options.disabled, busy]);

  function onChange(value: string) {
    reset();
    expected.current = value;
    options.onChange(value);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.defaultPrevented) return;
    if (composing.current || event.nativeEvent.isComposing || event.keyCode === 229) {
      // Prompt-kit submits Enter after this handler unless it is consumed.
      if (event.key === 'Enter') event.preventDefault();
      return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key === 'Escape' && (cursor.current || pending.current)) {
      event.preventDefault();
      onChange('');
      return;
    }
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    if (!options.source || options.disabled || options.occupied) return;
    const element = event.currentTarget;
    if (element.selectionStart !== element.selectionEnd) return;
    const direction = event.key === 'ArrowUp' ? 'older' : 'newer';
    if (!cursor.current && (options.input.length > 0 || direction === 'newer')) return;
    if (cursor.current && element.value.includes('\n')
      && element.selectionStart !== (direction === 'older' ? 0 : element.value.length)) return;
    event.preventDefault();
    if (pending.current) return;
    reader.current ??= new ComposerHistoryReader(options.source);
    const request = ++revision.current;
    const snapshot = options;
    const active = () => request === revision.current
      && latest.current.scope === snapshot.scope
      && latest.current.input === expected.current
      && !latest.current.disabled && !latest.current.occupied;
    pending.current = true;
    setBusy(true);
    setError(false);
    void reader.current.move(cursor.current, direction, active).then((result) => {
      if (!active()) return;
      cursor.current = result?.cursor ?? null;
      expected.current = result?.text ?? '';
      if (!result) reader.current = null;
      caret.current = { element, direction };
      setBrowsing(result !== null);
      latest.current.onChange(expected.current);
    }).catch(() => {
      if (!active()) return;
      // Keep the displayed text as a normal draft; retry begins from an empty draft.
      reset();
      setError(true);
    }).finally(() => {
      if (request !== revision.current) return;
      pending.current = false;
      setBusy(false);
    });
  }

  return {
    browsing, busy, error, onChange, onKeyDown, reset,
    onCompositionStart: () => { composing.current = true; reset(); },
    onCompositionEnd: () => { composing.current = false; },
    isComposing: () => composing.current,
  };
}
