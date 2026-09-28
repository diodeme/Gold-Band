import { useEffect, useMemo, useState, type Ref } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorSelection, EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { getChunks, unifiedMergeView } from '@codemirror/merge';
import { DELETED_BLOCK_SELECTOR } from '@/lib/editor-selection-quote';
import type { FileComparisonVm, GitFileComparisonVm } from '@/types';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { DiffSelectionQuote, type DiffQuoteSourceBase } from './EditorSelectionQuote';

export const DIFF_VIEW_SCAN_LIMIT = 10_000;
export const DIFF_VIEW_TIMEOUT_MS = 300;

type ReadonlyComparisonVm = Pick<FileComparisonVm | GitFileComparisonVm, 'path' | 'before' | 'after'>;

/** Drag head for `event`: over a removed block, the side of it away from `anchor`. */
function dragHead(view: EditorView, event: MouseEvent, anchor: number) {
  const block = event.target instanceof Element ? event.target.closest(DELETED_BLOCK_SELECTOR) : null;
  if (!block || !view.contentDOM.contains(block)) return view.posAtCoords({ x: event.clientX, y: event.clientY }, false);
  // The block renders before the line at its position: that line starts after it, the previous line ends before it.
  const blockPos = view.posAtDOM(block);
  return anchor < blockPos ? blockPos : Math.max(0, blockPos - 1);
}

/**
 * Removed lines are widgets with no document positions, so a drag from a document line would stop
 * at their edge. A drag reaching one selects the whole block; a drag starting inside one stays with
 * the browser's selection. Word and line selection keep the editor default.
 */
const removedBlockMouseSelection = EditorView.mouseSelectionStyle.of((view, press) => {
  if (press.button !== 0 || press.detail > 1) return null;
  let anchor = view.posAtCoords({ x: press.clientX, y: press.clientY }, false);
  let extendAnchor = view.state.selection.main.anchor;
  return {
    get(event, extend) {
      const from = extend ? extendAnchor : anchor;
      return EditorSelection.single(from, dragHead(view, event, from));
    },
    update(update) {
      if (!update.docChanged) return;
      anchor = update.changes.mapPos(anchor);
      extendAnchor = update.changes.mapPos(extendAnchor);
    },
  };
});

export function ReadonlyUnifiedDiff({
  comparison,
  editorRef,
  ariaLabel,
  onCreateEditor,
  onChunksChange,
  quoteSource = null,
}: {
  comparison: ReadonlyComparisonVm;
  editorRef?: Ref<ReactCodeMirrorRef>;
  ariaLabel: string;
  onCreateEditor?: (view: EditorView) => void;
  onChunksChange?: (count: number) => void;
  /** Where selection quotes come from; without it the diff offers no quoting. */
  quoteSource?: DiffQuoteSourceBase | null;
}) {
  const [language, setLanguage] = useState<Extension | null>(null);
  const [view, setView] = useState<EditorView | null>(null);

  useEffect(() => {
    let cancelled = false;
    void loadWorkspaceLanguageForPath(comparison.path).then((extension) => {
      if (!cancelled) setLanguage(extension);
    });
    return () => { cancelled = true; };
  }, [comparison.path]);

  const before = comparison.before?.content ?? '';
  const after = comparison.after?.content ?? '';
  const extensions = useMemo(() => {
    const next: Extension[] = [
      basicSetup({ lineNumbers: false, foldGutter: false, drawSelection: false }),
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping,
      workspaceEditorTheme,
      workspaceSyntaxHighlighting,
      removedBlockMouseSelection,
    ];
    if (language) next.push(language);
    next.push(unifiedMergeView({
      original: before,
      highlightChanges: true,
      gutter: true,
      mergeControls: false,
      collapseUnchanged: { margin: 3, minSize: 8 },
      diffConfig: { scanLimit: DIFF_VIEW_SCAN_LIMIT, timeout: DIFF_VIEW_TIMEOUT_MS },
    }));
    return next;
  }, [before, language]);

  return (
    <>
      <CodeMirror
        ref={editorRef}
        value={after}
        height="100%"
        width="100%"
        theme="none"
        basicSetup={false}
        editable={false}
        extensions={extensions}
        onCreateEditor={(created) => {
          setView(created);
          onCreateEditor?.(created);
        }}
        onUpdate={(update) => {
          const changed = update.docChanged || update.transactions.some((transaction) => transaction.reconfigured);
          if (!onChunksChange || !changed) return;
          onChunksChange(getChunks(update.state)?.chunks.length ?? 0);
        }}
        className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-editor]:h-full [&_.cm-editor]:max-w-full [&_.cm-scroller]:max-w-full [&_.cm-scroller]:overflow-y-auto [&_.cm-scroller]:overflow-x-hidden"
        aria-label={ariaLabel}
      />
      <DiffSelectionQuote view={view} source={quoteSource} />
    </>
  );
}
