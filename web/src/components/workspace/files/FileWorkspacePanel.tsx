import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ExternalLink, FileQuestion, FolderOpen, LoaderCircle, Maximize2, Pause, Play, RefreshCw, RotateCcw, SearchX, ShieldAlert, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openFileWithSystemApp, resolveWorkspaceFileLink, workspaceFilePreviewUrl } from '@/api';
import { Button } from '@/components/ui/button';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import type { FileWorkspaceLayoutVm, WorkspaceDirectoryEntryVm } from '@/types';
import { resolveWorkspacePanelWidthFromLayout } from '../workspace-layout';
import {
  fileWorkspaceResourceKey,
  useRightWorkspace,
  type FileWorkspaceResource,
  type RightWorkspaceResource,
} from '../right-workspace-context';
import { fileContentStore, useFileContentEntry } from './file-content-store';
import { fileExplorerStore } from './file-explorer-store';
import { WorkspaceFileEditor } from './WorkspaceFileEditor';
import { markdownImageSources } from './markdown-image-preview';
import { WorkspaceFileTree } from './WorkspaceFileTree';

interface FileWorkspacePanelProps {
  resource: Extract<RightWorkspaceResource, { kind: 'file' | 'file-browser' }>;
  layout: FileWorkspaceLayoutVm;
}

function usePanelWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(720);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => setWidth(Math.round(element.clientWidth));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return { ref, width };
}

function fileResourceFromEntry(resource: FileWorkspacePanelProps['resource'], entry: WorkspaceDirectoryEntryVm): FileWorkspaceResource {
  return {
    kind: 'file',
    key: fileWorkspaceResourceKey(resource.projectId, entry.canonicalPath),
    scopeKey: resource.scopeKey,
    projectId: resource.projectId,
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

export function FileWorkspacePanel({ resource, layout }: FileWorkspacePanelProps) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const selected = resource.kind === 'file' ? resource : null;
  const { ref, width } = usePanelWidth();
  const preferredWidthRequestedRef = useRef(false);
  const [compactView, setCompactView] = useState<'content' | 'tree'>(selected ? 'content' : 'tree');
  const split = width >= layout.splitMinWidth;

  useEffect(() => {
    if (preferredWidthRequestedRef.current) return;
    preferredWidthRequestedRef.current = true;
    if (workspace.width < layout.preferredWidth) workspace.setWidth(layout.preferredWidth);
  }, [layout.preferredWidth, workspace.setWidth, workspace.width]);

  useEffect(() => {
    if (selected) setCompactView('content');
  }, [selected?.key]);

  useEffect(() => {
    const unsubscribe = fileContentStore.subscribeChanges((event) => {
      if (event.projectId === resource.projectId) {
        fileExplorerStore.invalidate(resource.projectId, event.canonicalPath);
      }
    });
    void fileContentStore.startProjectWatch(resource.projectId);
    return () => {
      unsubscribe();
      void fileContentStore.stopProjectWatch(resource.projectId);
    };
  }, [resource.projectId]);

  const openFile = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    workspace.openResource(fileResourceFromEntry(resource, entry));
  }, [resource, workspace.openResource]);

  const content = selected ? <FileContent resource={selected} /> : <FileEmptyState />;
  const tree = (
    <WorkspaceFileTree
      projectId={resource.projectId}
      selectedPath={selected?.locator.canonicalPath ?? null}
      onOpenFile={openFile}
    />
  );
  const rememberedTreeWidth = fileExplorerStore.snapshot(resource.projectId).treeWidth;
  const treeWidth = Math.min(
    layout.treeMaxWidth,
    Math.max(layout.treeMinWidth, rememberedTreeWidth ?? layout.treeDefaultWidth),
  );
  const treePercent = Math.min(60, Math.max(20, width > 0 ? treeWidth / width * 100 : 38));

  return (
    <div ref={ref} className="flex min-h-0 flex-1 flex-col" data-file-workspace-panel="true">
      {!split ? (
        <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border/50 px-2">
          <Button size="sm" variant={compactView === 'content' ? 'secondary' : 'ghost'} className="h-7 text-xs" onClick={() => setCompactView('content')} disabled={!selected}>
            {t('workspace.filesPanel.file')}
          </Button>
          <Button size="sm" variant={compactView === 'tree' ? 'secondary' : 'ghost'} className="h-7 text-xs" onClick={() => setCompactView('tree')}>
            {t('workspace.filesPanel.directory')}
          </Button>
        </div>
      ) : null}
      <div className="min-h-0 flex-1">
        {split ? (
          <ResizablePanelGroup
            orientation="horizontal"
            className="h-full"
            onLayoutChanged={(panelLayout) => {
              const next = resolveWorkspacePanelWidthFromLayout({
                layout: panelLayout,
                panelId: 'file-tree',
                groupWidth: width,
                minWidth: layout.treeMinWidth,
                maxWidth: layout.treeMaxWidth,
              });
              if (next != null) fileExplorerStore.setTreeWidth(resource.projectId, next);
            }}
          >
            <ResizablePanel id="file-content" defaultSize={`${100 - treePercent}%`} minSize={300} className="min-w-0">{content}</ResizablePanel>
            <ResizableHandle className="bg-border/50" />
            <ResizablePanel
              id="file-tree"
              defaultSize={`${treePercent}%`}
              minSize={layout.treeMinWidth}
              maxSize={layout.treeMaxWidth}
              className="min-w-0"
            >
              {tree}
            </ResizablePanel>
          </ResizablePanelGroup>
        ) : compactView === 'content' && selected ? content : tree}
      </div>
    </div>
  );
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

function FileContent({ resource }: { resource: FileWorkspaceResource }) {
  const { t } = useTranslation();
  const entry = useFileContentEntry(resource.key);
  const [locationAdjusted, setLocationAdjusted] = useState(false);

  useEffect(() => {
    void fileContentStore.load(resource);
  }, [resource.key]);
  useEffect(() => setLocationAdjusted(false), [resource.targetRevision]);
  const path = resource.locator.scope === 'external'
    ? resource.locator.canonicalPath
    : (resource.locator.relativePath ?? resource.locator.canonicalPath);
  const svgSource = entry.snapshot?.kind === 'text'
    && resource.locator.canonicalPath.toLowerCase().endsWith('.svg');

  return (
    <article className="flex h-full min-h-0 flex-col bg-background" aria-label={resource.title}>
      <header className="flex min-h-10 shrink-0 items-center gap-2 border-b border-border/50 px-3 py-1.5">
        <div className="min-w-0 flex-1">
          <p className="truncate text-xs font-medium text-foreground">{resource.title}</p>
          <p className="truncate text-[10px] text-muted-foreground" title={path}>{path}</p>
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
        {entry.saveState.kind === 'scheduled' ? <span className="text-[10px] text-muted-foreground">{t('workspace.filesPanel.pendingSave')}</span> : null}
        {entry.saveState.kind === 'saving' ? <span className="flex items-center gap-1 text-[10px] text-muted-foreground"><LoaderCircle className="size-3 animate-spin" />{t('workspace.filesPanel.saving')}</span> : null}
        {entry.saveState.kind === 'clean' && entry.status === 'ready' && entry.snapshot?.kind === 'text' ? <span className="text-[10px] text-muted-foreground">{t('workspace.filesPanel.saved')}</span> : null}
        {locationAdjusted ? <span className="text-[10px] text-amber-600 dark:text-amber-400">{t('workspace.filesPanel.locationAdjusted')}</span> : null}
      </header>
      {entry.status === 'idle' || entry.status === 'loading' ? (
        <div className="flex flex-1 items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-4 animate-spin" />{t('workspace.filesPanel.loadingFile')}</div>
      ) : entry.status === 'error' ? (
        <FileError resource={resource} errorCode={entry.errorCode} />
      ) : entry.saveState.kind === 'conflict' ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <ConflictBanner resource={resource} />
          <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} />
        </div>
      ) : entry.saveState.kind === 'error' ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <SaveErrorBanner resource={resource} errorCode={entry.saveState.errorCode} />
          <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} />
        </div>
      ) : <FileSnapshotContent resource={resource} onLocationAdjusted={setLocationAdjusted} />}
    </article>
  );
}

function FileSnapshotContent({
  resource,
  onLocationAdjusted,
}: {
  resource: FileWorkspaceResource;
  onLocationAdjusted?: (adjusted: boolean) => void;
}) {
  const { t } = useTranslation();
  const entry = useFileContentEntry(resource.key);
  const persistEditorState = useCallback((state: unknown) => {
    fileContentStore.persistEditorState(resource.key, state, entry.contentRevision);
  }, [entry.contentRevision, resource.key]);
  const snapshot = entry.snapshot;
  const markdown = snapshot?.kind === 'text' && /\.md(?:own)?$/iu.test(resource.locator.canonicalPath);
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
  const approvalCount = [...markdownImages.values()].filter((image) => image.kind === 'approvalRequired').length;
  useEffect(() => {
    if (!markdown) return;
    void fileContentStore.syncMarkdownImages(
      resource.key,
      markdownMode === 'live-preview' ? markdownSources : [],
    );
  }, [markdown, markdownMode, markdownSources, resource.key]);
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
        <div className="min-h-0 flex-1">
        <WorkspaceFileEditor
          key={entry.contentRevision}
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
          onPersistState={persistEditorState}
          onLocationAdjusted={onLocationAdjusted}
          markdownMode={markdownMode}
          markdownLivePreviewAvailable={markdownLivePreviewAvailable}
          onMarkdownModeChange={(mode) => fileContentStore.setMarkdownMode(resource.key, mode)}
          markdownImages={markdownImages}
          markdownHasImages={markdownSources.length > 0}
        />
        </div>
      </div>
    );
  }
  if (snapshot.kind === 'image') return <ImagePreview resource={resource} />;
  return <UnsupportedFile resource={resource} />;
}

function ImagePreview({ resource }: { resource: FileWorkspaceResource }) {
  const { t } = useTranslation();
  const entry = useFileContentEntry(resource.key);
  const snapshot = entry.snapshot?.kind === 'image' ? entry.snapshot : null;
  const initialViewState = useMemo(() => fileContentStore.imageViewState(resource.key), [resource.key]);
  const [zoom, setZoomState] = useState(initialViewState.zoom);
  const [animationPaused, setAnimationPaused] = useState(() => (
    Boolean(snapshot?.animated)
    && typeof window !== 'undefined'
    && Boolean(window.matchMedia?.('(prefers-reduced-motion: reduce)').matches)
  ));
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ pointerId: number; x: number; y: number; left: number; top: number } | null>(null);
  useEffect(() => {
    if (!snapshot?.animated) {
      setAnimationPaused(false);
      return;
    }
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      setAnimationPaused(true);
    }
  }, [snapshot?.animated, snapshot?.previewToken]);
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport || !snapshot) return;
    viewport.scrollLeft = initialViewState.scrollLeft;
    viewport.scrollTop = initialViewState.scrollTop;
  }, [initialViewState.scrollLeft, initialViewState.scrollTop, resource.key, snapshot?.previewToken]);
  if (!snapshot) return null;
  const persistView = (nextZoom: number, viewport = viewportRef.current) => {
    fileContentStore.persistImageViewState(resource.key, {
      zoom: nextZoom,
      scrollLeft: viewport?.scrollLeft ?? 0,
      scrollTop: viewport?.scrollTop ?? 0,
    });
  };
  const setZoom = (next: number | ((current: number) => number)) => {
    setZoomState((current) => {
      const value = typeof next === 'function' ? next(current) : next;
      persistView(value);
      return value;
    });
  };
  const fitImage = () => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    setZoom(Math.max(0.1, Math.min(1, (viewport.clientWidth - 40) / snapshot.width, (viewport.clientHeight - 40) / snapshot.height)));
  };
  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex h-9 shrink-0 items-center justify-end gap-1 border-b border-border/40 px-2">
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
        <Button size="icon" variant="ghost" className="size-7" onClick={() => setZoom((value) => Math.max(0.1, value - 0.15))} aria-label={t('workspace.filesPanel.zoomOut')}><ZoomOut className="size-3.5" /></Button>
        <Button size="icon" variant="ghost" className="size-7" onClick={fitImage} aria-label={t('workspace.filesPanel.fitImage')}><Maximize2 className="size-3.5" /></Button>
        <Button size="icon" variant="ghost" className="size-7" onClick={() => {
          setZoom(1);
          const viewport = viewportRef.current;
          if (viewport) viewport.scrollTo({ left: 0, top: 0 });
        }} aria-label={t('workspace.filesPanel.resetImage')}><RotateCcw className="size-3.5" /></Button>
        <Button size="icon" variant="ghost" className="size-7" onClick={() => setZoom((value) => Math.min(8, value + 0.15))} aria-label={t('workspace.filesPanel.zoomIn')}><ZoomIn className="size-3.5" /></Button>
        <Button size="icon" variant="ghost" className="size-7" onClick={() => void openFileWithSystemApp(resource.locator.canonicalPath)} aria-label={t('workspace.filesPanel.openWithSystem')}><ExternalLink className="size-3.5" /></Button>
      </div>
      <div
        ref={viewportRef}
        className="gold-themed-scrollbar flex min-h-0 flex-1 cursor-grab items-center justify-center overflow-auto bg-[linear-gradient(45deg,var(--muted)_25%,transparent_25%),linear-gradient(-45deg,var(--muted)_25%,transparent_25%),linear-gradient(45deg,transparent_75%,var(--muted)_75%),linear-gradient(-45deg,transparent_75%,var(--muted)_75%)] bg-[length:16px_16px] bg-[position:0_0,0_8px,8px_-8px,-8px_0px] p-5 active:cursor-grabbing"
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          const viewport = viewportRef.current;
          if (!viewport) return;
          viewport.setPointerCapture(event.pointerId);
          dragRef.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, left: viewport.scrollLeft, top: viewport.scrollTop };
        }}
        onPointerMove={(event) => {
          const drag = dragRef.current;
          const viewport = viewportRef.current;
          if (!drag || !viewport || drag.pointerId !== event.pointerId) return;
          viewport.scrollLeft = drag.left - (event.clientX - drag.x);
          viewport.scrollTop = drag.top - (event.clientY - drag.y);
        }}
        onPointerUp={(event) => {
          if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null;
        }}
        onPointerCancel={() => { dragRef.current = null; }}
        onScroll={(event) => persistView(zoom, event.currentTarget)}
      >
        <img
          src={workspaceFilePreviewUrl(snapshot.previewToken, animationPaused)}
          alt={snapshot.name}
          draggable={false}
          className="max-w-none select-none object-contain shadow-lg"
          style={{ width: snapshot.width * zoom, height: snapshot.height * zoom }}
        />
      </div>
      <div className="shrink-0 border-t border-border/40 px-3 py-1 text-[10px] text-muted-foreground">{snapshot.width} × {snapshot.height} · {Math.round(zoom * 100)}%</div>
    </div>
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
        <Button className="mt-4" size="sm" variant="outline" onClick={() => void openFileWithSystemApp(resource.locator.canonicalPath)}><ExternalLink className="size-3.5" />{t('workspace.filesPanel.openWithSystem')}</Button>
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
    const resolved = await resolveWorkspaceFileLink(resource.projectId, resource.locator.canonicalPath);
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
    const resolved = await resolveWorkspaceFileLink(resource.projectId, resource.locator.canonicalPath);
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
          <Button size="sm" variant="ghost" onClick={() => void openFileWithSystemApp(resource.locator.canonicalPath)}><ExternalLink className="size-3.5" />{t('workspace.filesPanel.openWithSystem')}</Button>
        </div>
      </div>
    </div>
  );
}
