import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, FilePlus2, FileQuestion, FolderOpen, Globe, ImageIcon, LoaderCircle, Pause, Play, RefreshCw, SearchX, ShieldAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openExternalUrl, resolveWorkspaceFileLink, workspaceFilePreviewUrl } from '@/api';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useOverflowTooltip } from '@/hooks/useOverflowTooltip';
import { cn } from '@/lib/utils';
import { useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import type { FileWorkspaceLayoutVm, WorkspaceDirectoryEntryVm, WorkspaceFileLocatorVm } from '@/types';
import { isExternalUrlHref, isLocalFileHref } from '@/lib/file-link';
import { composerWorkspaceFileRefFromLocator } from '@/lib/workspace-file-reference';
import { useWorkspaceFileReferenceCommands, useWorkspaceFileReferencePresentation } from '../workspace-file-reference-bridge';
import { isBrowserDocumentPath } from '../browser/web-target';
import { openLocalDocumentInBrowser } from '../browser/open-web-target';
import { resolveWorkspacePanelWidthFromLayout } from '../workspace-layout';
import { useWorkspaceResponsiveState } from '../use-workspace-responsive-state';
import {
  fileWorkspaceResourceKey,
  useRightWorkspace,
  useRightWorkspaceCommands,
  type FileWorkspaceResource,
  type RightWorkspaceResource,
} from '../right-workspace-context';
import { fileContentStore, useFileContentEntry } from './file-content-store';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';
import { OpenWithSystemAppButton } from './OpenWithSystemAppButton';
import { fileExplorerStore, type FileTreeEntryMutation } from './file-explorer-store';
import { remapWorkspacePath, workspacePathIsWithin } from './workspace-path';
import { WorkspaceFileEditor, type EditorViewportAnchor } from './WorkspaceFileEditor';
import { markdownImageSources, markdownRemoteImageHosts } from './markdown-image-preview';
import { trustRemoteImageHosts, useRemoteImageTrust } from '@/lib/remote-image-trust-store';
import { isMarkdownDocumentPath } from './markdown-document';
import { markdownHasTableImages } from './markdown-live-preview';
import { WorkspaceFileTree } from './WorkspaceFileTree';
import { WorkspaceRootUnavailable } from '../WorkspaceRootUnavailable';
import { workspaceRootKey, workspaceRootRef } from '@/lib/workspace-root';

interface FileWorkspacePanelProps {
  resource: Extract<RightWorkspaceResource, { kind: 'file' | 'file-browser' }>;
  layout: FileWorkspaceLayoutVm;
}

/** Single-line text that reveals its full value on hover, only when it is actually truncated. */
function TruncatedText({ text, className, contentClassName }: { text: string; className?: string; contentClassName?: string }) {
  const { valueRef, tooltipOpen, showTooltipIfOverflowing, hideTooltip, handleTooltipOpenChange } = useOverflowTooltip();
  return (
    <Tooltip open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
      <TooltipTrigger asChild>
        <span
          ref={valueRef}
          className={cn('truncate', className)}
          onPointerEnter={showTooltipIfOverflowing}
          onPointerLeave={hideTooltip}
        >
          {text}
        </span>
      </TooltipTrigger>
      <TooltipContent className={cn('max-w-[420px]', contentClassName)}>{text}</TooltipContent>
    </Tooltip>
  );
}

function fileResourceFromEntry(
  resource: FileWorkspacePanelProps['resource'],
  workspacePath: string | null,
  entry: WorkspaceDirectoryEntryVm,
): FileWorkspaceResource {
  return {
    kind: 'file',
    key: fileWorkspaceResourceKey(resource.projectId, entry.canonicalPath),
    scopeKey: resource.scopeKey,
    projectId: resource.projectId,
    workspacePath,
    title: entry.name,
    description: entry.relativePath,
    attention: false,
    locator: {
      projectId: resource.projectId,
      canonicalPath: entry.canonicalPath,
      relativePath: entry.relativePath,
      scope: 'workspace',
    },
    target: null,
    targetRevision: 0,
  };
}

/**
 * Where the open file lives after a tree mutation: `undefined` when unaffected,
 * `null` when it was removed, or the entry it moved to with its parent.
 */
export function selectedFileAfterEntryMutation(
  locator: WorkspaceFileLocatorVm,
  mutation: FileTreeEntryMutation,
): WorkspaceDirectoryEntryVm | null | undefined {
  const affected = mutation.kind === 'removed' ? mutation.entry : mutation.from;
  if (!workspacePathIsWithin(locator.canonicalPath, affected.canonicalPath)) return undefined;
  if (mutation.kind === 'removed') return null;
  const canonicalPath = remapWorkspacePath(locator.canonicalPath, mutation.from.canonicalPath, mutation.to.canonicalPath);
  const relativePath = remapWorkspacePath(locator.relativePath ?? '', mutation.from.relativePath, mutation.to.relativePath);
  return {
    name: relativePath.slice(relativePath.lastIndexOf('/') + 1),
    relativePath,
    canonicalPath,
    kind: 'file',
    hasChildren: false,
    byteLength: null,
    modifiedAtNs: null,
  };
}

export function FileWorkspacePanel({ resource, layout }: FileWorkspacePanelProps) {
  const workspace = useRightWorkspace();
  if (resource.kind === 'file-browser' && resource.root.kind === 'unavailable') {
    return (
      <WorkspaceRootUnavailable
        root={resource.root}
        onBrowseMain={() => void workspace.openResource({ ...resource, browseMain: true })}
      />
    );
  }
  const workspacePath = resource.kind === 'file-browser' ? resource.root.workspacePath : resource.workspacePath;
  // Each file root has its own watch, tree and scroll state.
  return <FileWorkspaceRootPanel key={workspaceRootKey(resource.projectId, workspacePath)} resource={resource} workspacePath={workspacePath} layout={layout} />;
}

function FileWorkspaceRootPanel({ resource, workspacePath, layout }: FileWorkspacePanelProps & { workspacePath: string | null }) {
  const workspace = useRightWorkspace();
  const selected = resource.kind === 'file' ? resource : (resource.selectedFile ?? null);
  const activationFileKey = useRef(selected?.key ?? null);
  const root = useMemo(() => workspaceRootRef(resource.projectId, workspacePath), [resource.projectId, workspacePath]);

  useEffect(() => {
    let active = true;
    const unsubscribe = fileContentStore.subscribeChanges((event) => fileExplorerStore.applyFileChange(event));
    void (async () => {
      await fileContentStore.startRootWatch(root);
      if (!active) return;
      await Promise.all([
        fileExplorerStore.reconcile(root),
        activationFileKey.current ? fileContentStore.reconcile(activationFileKey.current) : Promise.resolve(),
      ]);
    })().catch(() => undefined);
    return () => {
      active = false;
      unsubscribe();
      void fileContentStore.stopRootWatch(root).catch(() => undefined);
    };
  }, [root]);

  const openFile = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    workspace.openResource(fileResourceFromEntry(resource, root.workspacePath, entry));
  }, [resource, root.workspacePath, workspace.openResource]);

  // A file that follows a rename is still the same file; only a new selection reveals the content view.
  const followedFileKey = useRef<string | null>(null);
  const [revealFileKey, setRevealFileKey] = useState(selected?.key ?? null);
  useEffect(() => {
    const key = selected?.key ?? null;
    if (key !== null && key === followedFileKey.current) return;
    followedFileKey.current = null;
    setRevealFileKey(key);
  }, [selected?.key]);

  const latestRef = useRef({ resource, root, selected, openResource: workspace.openResource, closeTab: workspace.closeTab });
  latestRef.current = { resource, root, selected, openResource: workspace.openResource, closeTab: workspace.closeTab };
  useEffect(() => fileExplorerStore.subscribeEntryMutations((mutation) => {
    const { resource: current, root: currentRoot, selected: file, openResource, closeTab } = latestRef.current;
    if (
      workspaceRootKey(mutation.root.projectId, mutation.root.workspacePath)
        !== workspaceRootKey(currentRoot.projectId, currentRoot.workspacePath)
      || !file
    ) return;
    const next = selectedFileAfterEntryMutation(file.locator, mutation);
    if (next === undefined) return;
    if (next) {
      if (current.kind === 'file') void closeTab(current.key);
      const followed = fileResourceFromEntry(current, currentRoot.workspacePath, next);
      followedFileKey.current = followed.key;
      void openResource(followed);
    } else if (current.kind === 'file-browser') {
      void openResource({ ...current, selectedFile: null });
    } else {
      void closeTab(current.key);
    }
  }), []);

  const content = selected ? <FileContent key={selected.key} resource={selected} /> : <FileEmptyState />;
  const tree = (
    <WorkspaceFileTree
      root={root}
      selectedPath={selected?.locator.canonicalPath ?? null}
      onOpenFile={openFile}
    />
  );
  return <FileWorkspaceSplitLayout layout={layout} hasFile={Boolean(selected)} revealFileKey={revealFileKey} content={content} tree={tree} treeWidth={fileExplorerStore.snapshot(root).treeWidth} onTreeWidthChange={(width) => fileExplorerStore.setTreeWidth(root, width)} />;
}

export function FileWorkspaceSplitLayout({ layout, hasFile, revealFileKey, content, tree, treeWidth, onTreeWidthChange }: { layout: FileWorkspaceLayoutVm; hasFile: boolean; revealFileKey: string | null; content: React.ReactNode; tree: React.ReactNode; treeWidth: number | null; onTreeWidthChange: (width: number) => void }) {
  const { t } = useTranslation(); const { ref, responsiveState, currentWidth } = useWorkspaceResponsiveState(layout.splitMinWidth);
  const [compactView, setCompactView] = useState<'content' | 'tree'>(hasFile ? 'content' : 'tree');
  useEffect(() => { if (revealFileKey) setCompactView('content'); }, [revealFileKey]);
  const width = Math.min(layout.treeMaxWidth, Math.max(layout.treeMinWidth, treeWidth ?? layout.treeDefaultWidth)); const percent = Math.min(60, Math.max(20, responsiveState.widthAtTransition > 0 ? width / responsiveState.widthAtTransition * 100 : 38));
  return <div ref={ref} className="flex min-h-0 flex-1 flex-col" data-file-workspace-panel="true">{!responsiveState.split ? <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border/50 px-2"><Button size="sm" variant={compactView === 'content' ? 'secondary' : 'ghost'} className="h-7 text-xs" onClick={() => setCompactView('content')} disabled={!hasFile}>{t('workspace.filesPanel.file')}</Button><Button size="sm" variant={compactView === 'tree' ? 'secondary' : 'ghost'} className="h-7 text-xs" onClick={() => setCompactView('tree')}>{t('workspace.filesPanel.directory')}</Button></div> : null}<div className="min-h-0 flex-1">{responsiveState.split ? <ResizablePanelGroup orientation="horizontal" className="h-full" onLayoutChanged={(panelLayout, meta) => { if (!meta.isUserInteraction) return; const next = resolveWorkspacePanelWidthFromLayout({ layout: panelLayout, panelId: 'file-tree', groupWidth: currentWidth(), minWidth: layout.treeMinWidth, maxWidth: layout.treeMaxWidth }); if (next != null) onTreeWidthChange(next); }}><ResizablePanel id="file-content" defaultSize={`${100 - percent}%`} minSize={280} className="min-w-0">{content}</ResizablePanel><ResizableHandle className="bg-border/50" /><ResizablePanel id="file-tree" defaultSize={`${percent}%`} minSize={layout.treeMinWidth} maxSize={layout.treeMaxWidth} className="min-w-0">{tree}</ResizablePanel></ResizablePanelGroup> : compactView === 'content' && hasFile ? content : tree}</div></div>;
}

function FileEmptyState() {
  const { t } = useTranslation();
  return (
    <div className="flex h-full min-h-56 items-center justify-center px-6 text-center">
      <div className="max-w-64 text-muted-foreground">
        <FolderOpen className="mx-auto mb-3 size-8 stroke-[1.4]" />
        <p className="text-sm font-medium text-foreground">{t('workspace.filesPanel.openFile')}</p>
        <p className="mt-1 text-xs leading-5">{t('workspace.filesPanel.chooseFromTree')}</p>
      </div>
    </div>
  );
}

export function FileContent({ resource }: { resource: FileWorkspaceResource }) {
  const { t } = useTranslation();
  const workspace = useRightWorkspaceCommands();
  const entry = useFileContentEntry(resource.key);
  const [locationAdjusted, setLocationAdjusted] = useState(false);
  const browserDocument = isBrowserDocumentPath(resource.locator.canonicalPath);
  const openDocumentInBrowser = useCallback(async () => {
    if (!browserDocument || !workspace.scopeKey) return;
    if (!await fileContentStore.flush(resource.key)) return;
    await openLocalDocumentInBrowser(resource.locator.canonicalPath, {
      root: workspaceRootRef(resource.projectId, resource.workspacePath),
      scopeKey: workspace.scopeKey,
      openResource: workspace.openResource,
      browserTitle: t('workspace.browser.title'),
    });
  }, [browserDocument, resource.key, resource.locator.canonicalPath, resource.projectId, resource.workspacePath, t, workspace.openResource, workspace.scopeKey]);

  useEffect(() => {
    void fileContentStore.load(resource);
  }, [resource.key]);
  useEffect(() => setLocationAdjusted(false), [resource.targetRevision]);
  const path = resource.locator.scope === 'external'
    ? resource.locator.canonicalPath
    : (resource.locator.relativePath ?? resource.locator.canonicalPath);
  const svgSource = entry.snapshot?.kind === 'text'
    && resource.locator.canonicalPath.toLowerCase().endsWith('.svg');
  const referenceCommands = useWorkspaceFileReferenceCommands();
  const referencePresentation = useWorkspaceFileReferencePresentation();
  const workspaceReference = referenceCommands?.available ? composerWorkspaceFileRefFromLocator(resource.locator) : null;

  return (
    <article className="flex h-full min-h-0 flex-col bg-background" aria-label={resource.title}>
      <header className="flex min-h-10 shrink-0 items-center gap-2 border-b border-border/50 px-3 py-1.5">
        <div className="min-w-0 flex-1">
          <p className="truncate text-xs font-medium text-foreground">{resource.title}</p>
          <TruncatedText text={path} className="block text-ui-micro text-muted-foreground" contentClassName="break-all" />
        </div>
        {svgSource ? (
          <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => void (async () => {
            if (await fileContentStore.flush(resource.key)) {
              await fileContentStore.reload(resource.key, false);
            }
          })()}>
            {t('workspace.filesPanel.viewPreview')}
          </Button>
        ) : null}
        {entry.saveState.kind === 'scheduled' ? <span className="text-ui-micro text-muted-foreground">{t('workspace.filesPanel.pendingSave')}</span> : null}
        {entry.saveState.kind === 'saving' ? <span className="flex items-center gap-1 text-ui-micro text-muted-foreground"><LoaderCircle className="size-3 animate-spin" />{t('workspace.filesPanel.saving')}</span> : null}
        {entry.saveState.kind === 'clean' && entry.status === 'ready' && entry.snapshot?.kind === 'text' ? <span className="text-ui-micro text-muted-foreground">{t('workspace.filesPanel.saved')}</span> : null}
        {locationAdjusted ? <span className="text-ui-micro text-amber-600 dark:text-amber-400">{t('workspace.filesPanel.locationAdjusted')}</span> : null}
        {workspaceReference ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="size-7"
                onClick={() => referenceCommands?.addWorkspaceFileRef(workspaceReference, referencePresentation)}
                aria-label={t('workspace.filesPanel.referenceToConversation')}
                data-file-reference-to-conversation="true"
              >
                <FilePlus2 className="size-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t('workspace.filesPanel.referenceToConversation')}</TooltipContent>
          </Tooltip>
        ) : null}
      </header>
      {entry.status === 'idle' || entry.status === 'loading' ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{t('workspace.filesPanel.loadingFile')}</div>
      ) : entry.status === 'error' ? (
        <FileError resource={resource} errorCode={entry.errorCode} />
      ) : entry.saveState.kind === 'conflict' ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <ConflictBanner resource={resource} />
          <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} onOpenInBrowser={browserDocument ? openDocumentInBrowser : undefined} />
        </div>
      ) : entry.saveState.kind === 'error' ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <SaveErrorBanner resource={resource} errorCode={entry.saveState.errorCode} />
          <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} onOpenInBrowser={browserDocument ? openDocumentInBrowser : undefined} />
        </div>
      ) : <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} onOpenInBrowser={browserDocument ? openDocumentInBrowser : undefined} />}
    </article>
  );
}

function FileSnapshotContent({
  resource,
  onLocationAdjusted,
  onOpenInBrowser,
}: {
  resource: FileWorkspaceResource;
  onLocationAdjusted?: (adjusted: boolean) => void;
  onOpenInBrowser?: () => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const markdownResourceLinkHandler = useMarkdownResourceLinkHandler();
  const entry = useFileContentEntry(resource.key);
  const persistEditorState = useCallback((state: unknown, viewportAnchor?: EditorViewportAnchor | null, scrollTop?: number) => {
    fileContentStore.persistEditorState(resource.key, state, entry.contentRevision, viewportAnchor, scrollTop);
  }, [entry.contentRevision, resource.key]);
  const snapshot = entry.snapshot;
  const markdown = snapshot?.kind === 'text' && isMarkdownDocumentPath(resource.locator.canonicalPath);
  const markdownLivePreviewAvailable = snapshot?.kind === 'text'
    ? fileContentStore.canUseMarkdownLivePreview(snapshot.content.length)
    : false;
  const markdownMode = markdown
    ? (markdownLivePreviewAvailable ? fileContentStore.markdownMode(resource.key) : 'source')
    : null;
  const markdownSources = useMemo(
    () => snapshot?.kind === 'text' && markdown ? markdownImageSources(snapshot.content) : [],
    [markdown, snapshot?.kind === 'text' ? snapshot.content : null],
  );
  const markdownImages = fileContentStore.markdownImages(resource.key);
  const markdownTableHasImages = snapshot?.kind === 'text' && markdown
    ? markdownHasTableImages(snapshot.content)
    : false;
  const handleMarkdownImagePreviewError = useCallback((_rawSrc: string, failedToken: string) => {
    void fileContentStore.refreshMarkdownImages(resource.key, failedToken);
  }, [resource.key]);
  const handleMarkdownLinkClick = useCallback((href: string) => {
    if (isLocalFileHref(href)) {
      if (markdownResourceLinkHandler) {
        void markdownResourceLinkHandler.openLocalFile(href, resource.locator.canonicalPath);
      }
      return;
    }
    if (isExternalUrlHref(href)) {
      if (markdownResourceLinkHandler?.openWebUrl && !href.startsWith('mailto:') && !href.startsWith('tel:')) {
        void markdownResourceLinkHandler.openWebUrl(href);
        return;
      }
      void openExternalUrl(href);
    }
  }, [markdownResourceLinkHandler, resource.locator.canonicalPath]);
  const approvalCount = [...markdownImages.values()].filter((image) => image.kind === 'approvalRequired').length;
  const remoteImageHosts = useMemo(
    () => snapshot?.kind === 'text' && markdown ? markdownRemoteImageHosts(snapshot.content) : [],
    [markdown, snapshot?.kind === 'text' ? snapshot.content : null],
  );
  const remoteImageTrust = useRemoteImageTrust();
  const untrustedRemoteImages = useMemo(() => {
    const trusted = new Set(remoteImageTrust.trustedHosts);
    const untrusted = remoteImageHosts.filter((host) => !trusted.has(host));
    return { count: untrusted.length, hosts: [...new Set(untrusted)] };
  }, [remoteImageHosts, remoteImageTrust]);
  const remoteImagesMessage = t('workspace.filesPanel.remoteMarkdownImages', {
    count: untrustedRemoteImages.count,
    hosts: untrustedRemoteImages.hosts.join(t('workspace.filesPanel.remoteMarkdownImageHostSeparator')),
  });
  const [remoteImageTrustState, setRemoteImageTrustState] = useState<'idle' | 'pending' | 'failed'>('idle');
  const loadRemoteImages = useCallback(async () => {
    setRemoteImageTrustState('pending');
    try {
      await trustRemoteImageHosts(untrustedRemoteImages.hosts);
      setRemoteImageTrustState('idle');
    } catch {
      setRemoteImageTrustState('failed');
    }
  }, [untrustedRemoteImages.hosts]);
  useEffect(() => {
    if (!markdown) return;
    void fileContentStore.syncMarkdownImages(
      resource.key,
      markdownMode === 'live-preview' ? markdownSources : [],
    );
  }, [markdown, markdownMode, markdownSources, resource.key]);
  useEffect(() => {
    if (!markdown || markdownMode !== 'live-preview') return;
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        fileContentStore.ensureMarkdownImagePreviews(resource.key);
      }
    };
    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => document.removeEventListener('visibilitychange', handleVisibilityChange);
  }, [markdown, markdownMode, resource.key]);
  if (!snapshot) return null;
  if (snapshot.kind === 'text') {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        {approvalCount > 0 ? (
          <div className="flex shrink-0 items-center gap-2 border-b border-amber-500/25 bg-amber-500/8 px-3 py-2 text-xs">
            <ShieldAlert className="size-3.5 text-amber-600 dark:text-amber-400" />
            <span className="min-w-0 flex-1">{t('workspace.filesPanel.externalMarkdownImages', { count: approvalCount })}</span>
            <Button size="sm" variant="outline" className="h-7" onClick={() => void fileContentStore.approveMarkdownImages(resource.key)}>
              {t('workspace.filesPanel.loadExternalMarkdownImages')}
            </Button>
          </div>
        ) : null}
        {markdownMode === 'live-preview' && untrustedRemoteImages.count > 0 ? (
          <div className="flex shrink-0 items-center gap-2 border-b border-border/60 px-3 py-2 text-xs" data-remote-markdown-images="true">
            <ImageIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <TruncatedText text={remoteImagesMessage} className="min-w-0 flex-1" contentClassName="break-words" />
            {remoteImageTrustState === 'failed' ? (
              <span className="shrink-0 text-destructive">{t('common.remoteImage.trustFailed')}</span>
            ) : null}
            <Button
              size="sm"
              variant="outline"
              className="h-7"
              disabled={remoteImageTrustState === 'pending'}
              onClick={() => void loadRemoteImages()}
            >
              {remoteImageTrustState === 'pending' ? <LoaderCircle className="size-3.5 animate-spin" /> : null}
              {t('common.remoteImage.load')}
            </Button>
          </div>
        ) : null}
        <div className="min-h-0 flex-1">
        <WorkspaceFileEditor
          key={`${resource.key}:${entry.contentRevision}`}
          documentKey={resource.key}
          value={snapshot.content}
          editable={snapshot.editable}
          language={snapshot.language}
          highlight={fileContentStore.shouldHighlight(snapshot.content.length)}
          contentRevision={entry.contentRevision}
          target={resource.target}
          targetRevision={resource.targetRevision}
          onChange={(content) => fileContentStore.updateText(resource.key, content)}
          onSave={() => void fileContentStore.flush(resource.key)}
          initialStateJson={fileContentStore.editorState(resource.key)}
          initialViewportAnchor={fileContentStore.editorViewport(resource.key)}
          initialViewportScrollTop={fileContentStore.editorScrollTop(resource.key)}
          initialConsumedLocationRevision={fileContentStore.consumedLocationTarget(resource.key)}
          onConsumeLocationTarget={(revision) => fileContentStore.consumeLocationTarget(resource.key, revision)}
          onPersistState={persistEditorState}
          onLocationAdjusted={onLocationAdjusted}
          markdownMode={markdownMode}
          markdownLivePreviewAvailable={markdownLivePreviewAvailable}
          onMarkdownModeChange={(mode) => fileContentStore.setMarkdownMode(resource.key, mode)}
          quoteLabel={resource.locator.relativePath ?? resource.locator.canonicalPath}
          markdownImages={markdownImages}
          markdownHasTableImages={markdownTableHasImages}
          onMarkdownImagePreviewError={handleMarkdownImagePreviewError}
          onMarkdownLinkClick={handleMarkdownLinkClick}
          onOpenInBrowser={onOpenInBrowser}
        />
        </div>
      </div>
    );
  }
  if (snapshot.kind === 'image') return <ImagePreview key={resource.key} resource={resource} onOpenInBrowser={onOpenInBrowser} />;
  return <UnsupportedFile resource={resource} />;
}

function ImagePreview({ resource, onOpenInBrowser }: { resource: FileWorkspaceResource; onOpenInBrowser?: () => void | Promise<void> }) {
  const { t } = useTranslation();
  const entry = useFileContentEntry(resource.key);
  const snapshot = entry.snapshot?.kind === 'image' ? entry.snapshot : null;
  const initialViewState = useMemo(() => fileContentStore.imageViewState(resource.key), [resource.key]);
  const [animationPaused, setAnimationPaused] = useState(false);
  useEffect(() => {
    if (!snapshot?.animated) setAnimationPaused(false);
  }, [snapshot?.animated]);
  if (!snapshot) return null;
  return (
    <WorkspaceImageCanvas
      resourceKey={resource.key}
      src={workspaceFilePreviewUrl(snapshot.previewGrant.token, animationPaused)}
      alt={snapshot.name}
      imageSize={{ width: snapshot.width, height: snapshot.height }}
      initialViewState={initialViewState}
      onViewStateChange={(state) => fileContentStore.persistImageViewState(resource.key, state)}
      onError={() => void fileContentStore.reload(resource.key)}
      toolbarBefore={<>
        {snapshot.sourceEditable ? <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={() => void fileContentStore.reload(resource.key, true)}>{t('workspace.filesPanel.viewSource')}</Button> : null}
        {snapshot.animated ? (
          <Button
            size="icon"
            variant="ghost"
            className="size-7"
            onClick={() => setAnimationPaused((paused) => !paused)}
            aria-label={t(animationPaused ? 'workspace.filesPanel.playAnimation' : 'workspace.filesPanel.pauseAnimation')}
          >
            {animationPaused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
          </Button>
        ) : null}
      </>}
      toolbarAfter={<>
        {onOpenInBrowser ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button size="icon" variant="ghost" className="size-7" onClick={() => void onOpenInBrowser()} aria-label={t('workspace.filesPanel.openHtmlInBrowser')} data-image-open-in-browser="true"><Globe className="size-3.5" /></Button>
            </TooltipTrigger>
            <TooltipContent>{t('workspace.filesPanel.openHtmlInBrowser')}</TooltipContent>
          </Tooltip>
        ) : null}
        <OpenWithSystemAppButton resource={resource} variant="icon" />
      </>}
    />
  );
}

function UnsupportedFile({ resource }: { resource: FileWorkspaceResource }) {
  const { t } = useTranslation();
  const entry = useFileContentEntry(resource.key);
  const snapshot = entry.snapshot?.kind === 'unsupported' ? entry.snapshot : null;
  if (!snapshot) return null;
  return (
    <div className="flex flex-1 items-center justify-center px-6 text-center">
      <div className="max-w-72">
        <FileQuestion className="mx-auto mb-3 size-8 text-muted-foreground" />
        <p className="text-sm font-medium">{t('workspace.filesPanel.unsupportedTitle')}</p>
        <p className="mt-1 text-xs leading-5 text-muted-foreground">{t(`workspace.filesPanel.limitations.${snapshot.limitationCode}`, snapshot.limitationCode)}</p>
        <div className="mt-4"><OpenWithSystemAppButton resource={resource} variant="button" /></div>
      </div>
    </div>
  );
}

function ConflictBanner({ resource }: { resource: FileWorkspaceResource }) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs">
      <AlertTriangle className="size-3.5 text-amber-600 dark:text-amber-400" />
      <span className="min-w-40 flex-1">{t('workspace.filesPanel.conflict')}</span>
      <Button size="sm" variant="ghost" className="h-7" onClick={() => void fileContentStore.reload(resource.key)}>{t('workspace.filesPanel.reloadDisk')}</Button>
      <Button size="sm" variant="outline" className="h-7" onClick={() => void fileContentStore.forceOverwrite(resource.key)}>{t('workspace.filesPanel.overwrite')}</Button>
    </div>
  );
}

function SaveErrorBanner({ resource, errorCode }: { resource: FileWorkspaceResource; errorCode: string }) {
  const { t } = useTranslation();
  const reauthorize = async () => {
    if (resource.locator.scope !== 'external') return;
    const resolved = await resolveWorkspaceFileLink(workspaceRootRef(resource.projectId, resource.workspacePath), resource.locator.canonicalPath);
    if (resolved.externalAccessGrant) {
      await fileContentStore.reauthorize(resource.key, resolved.externalAccessGrant);
    }
  };
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-destructive/30 bg-destructive/8 px-3 py-2 text-xs">
      <AlertTriangle className="size-3.5 text-destructive" />
      <span className="min-w-40 flex-1">{t(`workspace.filesPanel.errors.${errorCode}`, errorCode)}</span>
      {resource.locator.scope === 'external' ? <Button size="sm" variant="ghost" className="h-7" onClick={() => void reauthorize()}>{t('workspace.filesPanel.reauthorize')}</Button> : null}
      <Button size="sm" variant="outline" className="h-7" onClick={() => void fileContentStore.retry(resource.key)}>{t('workspace.filesPanel.retry')}</Button>
    </div>
  );
}

function FileError({ resource, errorCode }: { resource: FileWorkspaceResource; errorCode: string | null }) {
  const { t } = useTranslation();
  const missing = errorCode === 'workspace-file.not-found';
  const reauthorize = async () => {
    const resolved = await resolveWorkspaceFileLink(workspaceRootRef(resource.projectId, resource.workspacePath), resource.locator.canonicalPath);
    fileContentStore.primeExternalGrant(
      resource.key,
      resource.projectId,
      resource.locator.canonicalPath,
      resolved.externalAccessGrant,
    );
    if (!resolved.externalAccessGrant || !await fileContentStore.reauthorize(resource.key, resolved.externalAccessGrant)) {
      await fileContentStore.load(resource, false, true);
    }
  };
  return (
    <div className="flex flex-1 items-center justify-center px-6 text-center">
      <div className="max-w-72">
        {missing ? <SearchX className="mx-auto mb-3 size-8 text-muted-foreground" /> : <AlertTriangle className="mx-auto mb-3 size-8 text-destructive" />}
        <p className="text-sm font-medium">{t(`workspace.filesPanel.errors.${errorCode}`, errorCode ?? '')}</p>
        <div className="mt-4 flex justify-center gap-2">
          {resource.locator.scope === 'external' ? (
            <Button size="sm" variant="outline" onClick={() => void reauthorize()}><RefreshCw className="size-3.5" />{t('workspace.filesPanel.reauthorize')}</Button>
          ) : (
            <Button size="sm" variant="outline" onClick={() => void fileContentStore.load(resource, false, true)}><RefreshCw className="size-3.5" />{t('workspace.filesPanel.retry')}</Button>
          )}
          <OpenWithSystemAppButton resource={resource} variant="button" />
        </div>
      </div>
    </div>
  );
}
