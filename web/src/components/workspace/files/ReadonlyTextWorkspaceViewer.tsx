import { useEffect, useMemo, useState } from 'react';
import CodeMirror, { basicSetup } from '@uiw/react-codemirror';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { useTranslation } from 'react-i18next';

import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { isMarkdownDocumentPath } from './markdown-document';
import { ReadonlyMarkdownWorkspaceViewer } from './ReadonlyMarkdownWorkspaceViewer';

interface ReadonlyTextWorkspaceViewerProps {
  documentKey: string;
  name: string;
  value: string;
}

export function ReadonlyTextWorkspaceViewer({
  documentKey,
  name,
  value,
}: ReadonlyTextWorkspaceViewerProps) {
  const { t } = useTranslation();
  const markdown = isMarkdownDocumentPath(name);
  const [language, setLanguage] = useState<Extension | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (markdown) {
      setLanguage(null);
      return () => { cancelled = true; };
    }
    void loadWorkspaceLanguageForPath(name).then((extension) => {
      if (!cancelled) setLanguage(extension);
    });
    return () => { cancelled = true; };
  }, [markdown, name]);

  const extensions = useMemo(() => [
    basicSetup({ lineNumbers: false, foldGutter: false }),
    lineNumbers(),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    EditorView.lineWrapping,
    workspaceEditorTheme,
    workspaceSyntaxHighlighting,
    ...(language ? [language] : []),
  ], [language]);

  if (markdown) {
    return <ReadonlyMarkdownWorkspaceViewer documentKey={documentKey} value={value} />;
  }

  return (
    <CodeMirror
      value={value}
      height="100%"
      theme="none"
      basicSetup={false}
      editable={false}
      extensions={extensions}
      className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-content]:min-w-0 [&_.cm-editor]:h-full [&_.cm-editor]:min-w-0 [&_.cm-line]:break-words [&_.cm-scroller]:min-w-0 [&_.cm-scroller]:overflow-auto"
      aria-label={t('turnFiles.assetViewer')}
    />
  );
}
