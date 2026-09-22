import { useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import { isComposerHistoryBoundary } from '@/lib/composer-history-caret';
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
  const caret = useRef<{ element: HTMLTextAreaElement; restoreDraft: boolean } | null>(null);
  const draftCaret = useRef<{ start: number; end: number } | null>(null);
  const [historyText, setHistoryText] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  function reset(restoreDraftFrom?: HTMLTextAreaElement) {
    revision.current += 1;
    reader.current = null;
    cursor.current = null;
    pending.current = false;
    caret.current = restoreDraftFrom
      ? { element: restoreDraftFrom, restoreDraft: true }
      : null;
    if (!restoreDraftFrom) draftCaret.current = null;
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
    const next = caret.current;
    if (!next) return;
    caret.current = null;
    const { element, restoreDraft } = next;
    const max = element.value.length;
    if (restoreDraft) {
      const saved = draftCaret.current;
      draftCaret.current = null;
      if (saved) {
        element.setSelectionRange(Math.min(saved.start, max), Math.min(saved.end, max));
        return;
      }
    }
    element.setSelectionRange(max, max);
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
      reset(event.currentTarget);
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
    if (!isComposerHistoryBoundary(element, direction)) return;
    event.preventDefault();
    if (pending.current) return;
    if (!cursor.current) {
      draftCaret.current = {
        start: element.selectionStart ?? 0,
        end: element.selectionEnd ?? 0,
      };
    }
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
      caret.current = { element, restoreDraft: result === null };
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
