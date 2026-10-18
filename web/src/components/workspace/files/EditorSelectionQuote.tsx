import type { EditorView } from '@codemirror/view';
import { useMemo } from 'react';

import { SelectionQuoteButton } from '@/components/conversation/SelectionQuoteButton';
import { useComposerQuoteCommand } from '@/components/workspace/workspace-file-reference-bridge';
import { readEditorLineSelection, readUnifiedDiffSelection } from '@/lib/editor-selection-quote';
import type { UserPromptQuoteSource } from '@/types';

export type DiffQuoteSourceBase = Omit<Extract<UserPromptQuoteSource, { kind: 'diff' }>, 'scope'>;

function useEditorRoot(view: EditorView | null) {
  return useMemo(() => ({ current: view?.dom ?? null }), [view]);
}

/** Selection quoting for a file shown in an editor; `label` names the file in the quote. */
export function EditorSelectionQuote({ view, label }: { view: EditorView | null; label: string | null | undefined }) {
  const quoteToComposer = useComposerQuoteCommand();
  const rootRef = useEditorRoot(view);
  if (!view || !label || !quoteToComposer) return null;
  return (
    <SelectionQuoteButton
      rootRef={rootRef}
      readSelection={() => readEditorLineSelection(view)}
      onQuote={(selection) => quoteToComposer({
        text: selection.text,
        source: { kind: 'file', label, startLine: selection.startLine, endLine: selection.endLine },
      })}
    />
  );
}

/** Selection quoting for a unified diff; a selection widens to the whole changes it touches. */
export function DiffSelectionQuote({ view, source }: { view: EditorView | null; source: DiffQuoteSourceBase | null | undefined }) {
  const quoteToComposer = useComposerQuoteCommand();
  const rootRef = useEditorRoot(view);
  if (!view || !source || !quoteToComposer) return null;
  return (
    <SelectionQuoteButton
      rootRef={rootRef}
      readSelection={() => readUnifiedDiffSelection(view)}
      onQuote={(fragment) => quoteToComposer({ text: fragment.text, source: { ...source, scope: 'selection' } })}
    />
  );
}
