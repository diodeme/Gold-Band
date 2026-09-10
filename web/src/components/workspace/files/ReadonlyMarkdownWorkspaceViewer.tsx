import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { openExternalUrl } from '@/api';
import { displayAppError } from '@/i18n';
import { isExternalUrlHref, isLocalFileHref } from '@/lib/file-link';
import type { WorkspaceFileSnapshotVm } from '@/types';
import { useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { FileContentStore, fileContentStore, type MarkdownEditorMode } from './file-content-store';
import { markdownImageSources } from './markdown-image-preview';
import { markdownHasTableImages } from './markdown-live-preview';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';

interface ReadonlyMarkdownWorkspaceViewerProps {
  documentKey: string;
  value: string;
  contentRevision?: number;
  onMarkdownLinkClick?: (href: string) => void;
  fileSnapshot?: Extract<WorkspaceFileSnapshotVm, { kind: 'text' }>;
}

const noop = () => undefined;

function ReadonlyMarkdownWorkspaceViewerSession({
  documentKey,
  value,
  contentRevision = 0,
  onMarkdownLinkClick,
  fileSnapshot,
}: ReadonlyMarkdownWorkspaceViewerProps) {
  const { t } = useTranslation();
  const linkHandler = useMarkdownResourceLinkHandler();
  const [linkError, setLinkError] = useState<string | null>(null);
  // One document reuses the file image lifecycle without subscribing to workspace writes.
  const [session] = useState(() => new FileContentStore());
  useSyncExternalStore(session.subscribe, () => session.snapshot(documentKey), () => session.snapshot(documentKey));
  const [requestedMode, setRequestedMode] = useState<MarkdownEditorMode>('live-preview');
  const livePreviewAvailable = fileContentStore.canUseMarkdownLivePreview(value.length);
  const markdownMode = livePreviewAvailable ? requestedMode : 'source';
  const sources = useMemo(() => fileSnapshot && markdownMode === 'live-preview' ? markdownImageSources(value) : [], [fileSnapshot, markdownMode, value]);
  useEffect(() => {
    if (!fileSnapshot) return;
    session.configure(fileContentStore.configuration);
    session.adoptReadonlySnapshot({ kind: 'file', key: documentKey, scopeKey: documentKey, projectId: fileSnapshot.locator.projectId,
      title: fileSnapshot.name, attention: false, locator: fileSnapshot.locator, target: null, targetRevision: 0 }, fileSnapshot);
    return () => { void session.release(documentKey); };
  }, [documentKey, fileSnapshot, session]);
  useEffect(() => { if (fileSnapshot) void session.syncMarkdownImages(documentKey, sources); }, [documentKey, fileSnapshot, session, sources]);
  useEffect(() => {
    const visible = () => { if (document.visibilityState === 'visible') session.ensureMarkdownImagePreviews(documentKey); };
    document.addEventListener('visibilitychange', visible);
    return () => document.removeEventListener('visibilitychange', visible);
  }, [documentKey, session]);
  const images = session.markdownImages(documentKey);
  const approvalCount = [...images.values()].filter(image => image.kind === 'approvalRequired').length;
  const onImageError = useCallback((_source: string, token: string) => { void session.refreshMarkdownImages(documentKey, token); }, [documentKey, session]);
  const onLink = useCallback((href: string) => {
    if (onMarkdownLinkClick) { onMarkdownLinkClick(href); return; }
    setLinkError(null);
    void (async () => {
      try {
        if (isLocalFileHref(href)) {
          if (!linkHandler) throw { code: 'workspace-file.project-not-found', params: {} };
          const result = await linkHandler.openLocalFile(href, fileSnapshot?.locator.canonicalPath);
          if (result?.status === 'error') throw result.error;
        } else if (isExternalUrlHref(href)) await openExternalUrl(href);
      } catch (reason) { setLinkError(displayAppError(t, reason)); }
    })();
  }, [fileSnapshot?.locator.canonicalPath, linkHandler, onMarkdownLinkClick, t]);

  return (
    <div className="flex h-full min-h-0 flex-col">
    {linkError && <Alert variant="destructive"><AlertDescription>{linkError}</AlertDescription></Alert>}
    {approvalCount > 0 && <div className="flex shrink-0 flex-wrap items-center gap-2 px-3 py-2 text-xs">
      <span>{t('workspace.filesPanel.externalMarkdownImages', { count: approvalCount })}</span>
      <Button size="sm" variant="outline" onClick={() => void session.approveMarkdownImages(documentKey)}>{t('workspace.filesPanel.loadExternalMarkdownImages')}</Button>
    </div>}
    <div className="min-h-0 flex-1"><WorkspaceFileEditor
      documentKey={documentKey}
      value={value}
      editable={false}
      language="markdown"
      highlight={fileContentStore.shouldHighlight(value.length)}
      contentRevision={contentRevision}
      target={null}
      targetRevision={0}
      onChange={noop}
      onSave={noop}
      initialStateJson={null}
      onPersistState={noop}
      markdownMode={markdownMode}
      markdownLivePreviewAvailable={livePreviewAvailable}
      onMarkdownModeChange={setRequestedMode}
      markdownImages={images}
      markdownHasTableImages={markdownHasTableImages(value)}
      onMarkdownImagePreviewError={onImageError}
      onMarkdownLinkClick={onLink}
    /></div></div>
  );
}

export function ReadonlyMarkdownWorkspaceViewer(props: ReadonlyMarkdownWorkspaceViewerProps) {
  return <ReadonlyMarkdownWorkspaceViewerSession key={props.documentKey} {...props} />;
}
