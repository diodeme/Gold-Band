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
import {
  loadWorkspaceLanguageForPath,
  workspaceEditorTheme,
  workspaceSyntaxHighlighting,
} from './editor-extensions';
import { isMarkdownDocumentPath } from './markdown-document';
import { ReadonlyMarkdownWorkspaceViewer } from './ReadonlyMarkdownWorkspaceViewer';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';

export function ConversationAssetWorkspacePanel({ resource }: { resource: ConversationAssetWorkspaceResource }) {
  const { t } = useTranslation();
  const [content, setContent] = useState<ContentVm | null>(null);
  const [language, setLanguage] = useState<Extension | null>(null);
  const [failed, setFailed] = useState(false);
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
        <FileText className="size-3.5 text-foreground" />
        <span className="min-w-0 flex-1 truncate font-mono">{content.title || resource.name}</span>
        <span className="text-muted-foreground">{content.kind}</span>
      </header>
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {imageSrc ? (
          <WorkspaceImageCanvas src={imageSrc} alt={resource.name} />
        ) : markdown ? (
          <ReadonlyMarkdownWorkspaceViewer
            documentKey={resource.key}
            value={content.content}
          />
        ) : (
          <CodeMirror value={content.content} height="100%" theme="none" basicSetup={false} editable={false} extensions={extensions} className="h-full min-h-0 min-w-0 max-w-full overflow-hidden [&_.cm-content]:min-w-0 [&_.cm-editor]:h-full [&_.cm-editor]:min-w-0 [&_.cm-line]:break-words [&_.cm-scroller]:min-w-0 [&_.cm-scroller]:overflow-auto" aria-label={t('turnFiles.assetViewer')} />
        )}
      </div>
    </section>
  );
}
