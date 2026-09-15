import { useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import { ComposerHistoryReader, type ComposerHistorySource, type HistoryCursor } from '@/lib/composer-history';

interface Options {
  scope: string;
  source: ComposerHistorySource | null;
  input: string;
  draftIdentity?: unknown;
  disabled: boolean;
  onChange: (value: string) => void;
  onCommitHistory?: (value: string) => void;
}

export function useComposerHistory(options: Options) {
  const latest = useRef(options);
  latest.current = options;
  const reader = useRef<ComposerHistoryReader | null>(null);
  const cursor = useRef<HistoryCursor | null>(null);
  const revision = useRef(0);
  const pending = useRef(false);
  const composing = useRef(false);
  const caret = useRef<{ element: HTMLTextAreaElement; direction: 'older' | 'newer' } | null>(null);
  const [historyText, setHistoryText] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  function reset() {
    revision.current += 1;
    reader.current = null;
    cursor.current = null;
    caret.current = null;
    pending.current = false;
    setHistoryText(null);
    setBusy(false);
    setBrowsing(false);
    setError(false);
  }

  useLayoutEffect(() => {
    reset();
    return () => { revision.current += 1; };
  }, [options.scope, options.source]);

  useLayoutEffect(() => {
    if (options.disabled) reset();
    if (caret.current) {
      const { element, direction } = caret.current;
      const position = direction === 'older' && element.value.includes('\n') ? 0 : element.value.length;
      element.setSelectionRange(position, position);
      caret.current = null;
    }
  }, [historyText, options.input, options.disabled, busy]);

  useLayoutEffect(() => {
    if (cursor.current || pending.current) reset();
  }, [options.input, options.draftIdentity]);

  function onChange(value: string) {
    const commitHistory = cursor.current !== null;
    reset();
    if (commitHistory) (options.onCommitHistory ?? options.onChange)(value);
    else options.onChange(value);
  }

  function commitHistory(): string | null {
    if (!cursor.current || historyText === null) {
      reset();
      return null;
    }
    const value = historyText;
    reset();
    return value;
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
      reset();
      return;
    }
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    if (!options.source || options.disabled) return;
    const element = event.currentTarget;
    if (element.selectionStart !== element.selectionEnd) return;
    const direction = event.key === 'ArrowUp' ? 'older' : 'newer';
    if (!cursor.current && direction === 'newer') {
      if (pending.current) { event.preventDefault(); reset(); }
      return;
    }
    if (element.value.includes('\n')
      && element.selectionStart !== (direction === 'older' ? 0 : element.value.length)) return;
    event.preventDefault();
    if (pending.current) return;
    reader.current ??= new ComposerHistoryReader(options.source);
    const request = ++revision.current;
    const snapshot = options;
    const active = () => request === revision.current
      && latest.current.scope === snapshot.scope
      && latest.current.input === snapshot.input
      && latest.current.draftIdentity === snapshot.draftIdentity
      && !latest.current.disabled;
    pending.current = true;
    setBusy(true);
    setError(false);
    void reader.current.move(cursor.current, direction, active).then((result) => {
      if (!active()) return;
      cursor.current = result?.cursor ?? null;
      setHistoryText(result?.text ?? null);
      if (!result) reader.current = null;
      caret.current = { element, direction };
      setBrowsing(result !== null);
    }).catch(() => {
      if (!active()) return;
      // Failed history reads return to the untouched canonical draft.
      reset();
      setError(true);
    }).finally(() => {
      if (request !== revision.current) return;
      pending.current = false;
      setBusy(false);
    });
  }

  return {
    value: historyText ?? options.input,
    browsing, busy, error, onChange, onKeyDown, reset, commitHistory,
    onCompositionStart: () => {
      composing.current = true;
      const value = commitHistory();
      if (value !== null) (options.onCommitHistory ?? options.onChange)(value);
    },
    onCompositionEnd: () => { composing.current = false; },
    isComposing: () => composing.current,
  };
}
