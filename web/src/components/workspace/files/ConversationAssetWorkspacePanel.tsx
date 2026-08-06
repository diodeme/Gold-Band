import { useEffect, useMemo, useState } from 'react';
import CodeMirror, { basicSetup } from '@uiw/react-codemirror';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { FileText, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { showArtifact, showConversationAttachment, showConversationMessageAttachment } from '@/api';
import { imageSrcFromContent } from '@/lib/asset-preview';
import type { ContentVm } from '@/types';
import type { ConversationAssetWorkspaceResource } from '../right-workspace-context';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { isMarkdownDocumentPath } from './markdown-document';
import type { MarkdownEditorMode } from './file-content-store';

export function ConversationAssetWorkspacePanel({ resource }: { resource: ConversationAssetWorkspaceResource }) {
  const { t } = useTranslation();
  const [content, setContent] = useState<ContentVm | null>(null);
  const [language, setLanguage] = useState<Extension | null>(null);
  const [failed, setFailed] = useState(false);
  const [markdownMode, setMarkdownMode] = useState<MarkdownEditorMode>('live-preview');
  const markdown = isMarkdownDocumentPath(resource.name);

  useEffect(() => {
    let cancelled = false;
    setContent(null);
    setFailed(false);
    const { locator } = resource;
    const request = resource.assetKind === 'artifact'
      ? showArtifact(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, resource.name, locator.outerNodeId, locator.outerAttemptId)
      : resource.assetKind === 'input-attachment'
        ? showConversationAttachment(locator.projectId, locator.taskId, resource.name)
        : showConversationMessageAttachment(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, resource.name, resource.path ?? '', locator.outerNodeId, locator.outerAttemptId);
    void request
      .then((next) => { if (!cancelled) setContent(next); })
      .catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; };
  }, [resource]);

  useEffect(() => {
    let cancelled = false;
    if (markdown) {
      setLanguage(null);
      return () => { cancelled = true; };
    }
    void loadWorkspaceLanguageForPath(resource.name).then((extension) => { if (!cancelled) setLanguage(extension); });
    return () => { cancelled = true; };
  }, [markdown, resource.name]);

  useEffect(() => setMarkdownMode('live-preview'), [resource.key]);

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
  const imageSrc = imageSrcFromContent(content);

  if (failed) {
    return <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-destructive"><TriangleAlert className="size-4" />{t('turnFiles.assetLoadFailed')}</div>;
  }
  if (!content) {
    return <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">{t('turnFiles.loading')}</div>;
  }
  return (
    <section className="flex min-h-0 flex-1 flex-col" data-conversation-asset-workspace={resource.assetKind}>
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
        <FileText className="size-3.5 text-primary" />
        <span className="min-w-0 flex-1 truncate font-mono">{content.title || resource.name}</span>
        <span className="text-muted-foreground">{content.kind}</span>
      </header>
      <div className="min-h-0 flex-1 overflow-hidden">
        {imageSrc ? (
          <div className="flex size-full items-center justify-center overflow-auto bg-muted/10 p-4">
            <img src={imageSrc} alt={resource.name} className="max-h-full max-w-full object-contain" />
          </div>
        ) : markdown ? (
          <WorkspaceFileEditor
            documentKey={resource.key}
            value={content.content}
            editable={false}
            language="markdown"
            highlight
            contentRevision={0}
            target={null}
            targetRevision={0}
            onChange={() => undefined}
            onSave={() => undefined}
            initialStateJson={null}
            onPersistState={() => undefined}
            markdownMode={markdownMode}
            onMarkdownModeChange={setMarkdownMode}
          />
        ) : (
          <CodeMirror value={content.content} height="100%" theme="none" basicSetup={false} editable={false} extensions={extensions} className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-content]:min-w-0 [&_.cm-editor]:h-full [&_.cm-editor]:min-w-0 [&_.cm-line]:break-words [&_.cm-scroller]:min-w-0 [&_.cm-scroller]:overflow-auto" aria-label={t('turnFiles.assetViewer')} />
        )}
      </div>
    </section>
  );
}
