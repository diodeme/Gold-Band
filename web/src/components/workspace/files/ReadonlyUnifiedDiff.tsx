import { useEffect, useMemo, useState, type Ref } from 'react';
import CodeMirror, { basicSetup, type ReactCodeMirrorRef } from '@uiw/react-codemirror';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { unifiedMergeView } from '@codemirror/merge';
import type { FileComparisonVm, GitFileComparisonVm } from '@/types';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';

export const DIFF_VIEW_SCAN_LIMIT = 10_000;
export const DIFF_VIEW_TIMEOUT_MS = 300;

type ReadonlyComparisonVm = Pick<FileComparisonVm | GitFileComparisonVm, 'path' | 'before' | 'after'>;

export function ReadonlyUnifiedDiff({
  comparison,
  editorRef,
  ariaLabel,
  onCreateEditor,
}: {
  comparison: ReadonlyComparisonVm;
  editorRef?: Ref<ReactCodeMirrorRef>;
  ariaLabel: string;
  onCreateEditor?: (view: EditorView) => void;
}) {
  const [language, setLanguage] = useState<Extension | null>(null);

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
    <CodeMirror
      ref={editorRef}
      value={after}
      height="100%"
      width="100%"
      theme="none"
      basicSetup={false}
      editable={false}
      extensions={extensions}
      onCreateEditor={onCreateEditor}
      className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-editor]:h-full [&_.cm-editor]:max-w-full [&_.cm-scroller]:max-w-full [&_.cm-scroller]:overflow-y-auto [&_.cm-scroller]:overflow-x-hidden"
      aria-label={ariaLabel}
    />
  );
}
