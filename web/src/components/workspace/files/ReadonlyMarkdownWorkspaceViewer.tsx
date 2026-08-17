import { useState } from 'react';
import { fileContentStore, type MarkdownEditorMode } from './file-content-store';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';

interface ReadonlyMarkdownWorkspaceViewerProps {
  documentKey: string;
  value: string;
  contentRevision?: number;
  onMarkdownLinkClick?: (href: string) => void;
}

const noop = () => undefined;

function ReadonlyMarkdownWorkspaceViewerSession({
  documentKey,
  value,
  contentRevision = 0,
  onMarkdownLinkClick,
}: ReadonlyMarkdownWorkspaceViewerProps) {
  const [requestedMode, setRequestedMode] = useState<MarkdownEditorMode>('live-preview');
  const livePreviewAvailable = fileContentStore.canUseMarkdownLivePreview(value.length);
  const markdownMode = livePreviewAvailable ? requestedMode : 'source';

  return (
    <WorkspaceFileEditor
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
      onMarkdownLinkClick={onMarkdownLinkClick}
    />
  );
}

export function ReadonlyMarkdownWorkspaceViewer(props: ReadonlyMarkdownWorkspaceViewerProps) {
  return <ReadonlyMarkdownWorkspaceViewerSession key={props.documentKey} {...props} />;
}
