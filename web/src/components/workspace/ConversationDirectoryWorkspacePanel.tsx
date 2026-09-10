import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, FolderOpen, LoaderCircle, RotateCcw } from 'lucide-react';
import { Tree, type NodeRendererProps, type TreeApi } from 'react-arborist';
import { useTranslation } from 'react-i18next';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '@/i18n';
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu';
import { cn } from '@/lib/utils';
import { useMeasuredElementHeight } from '@/hooks/use-measured-element-height';
import { listConversationDirectory, openConversationDirectoryPathInFileManager, readConversationDirectoryFile, workspaceFilePreviewUrl } from '@/api';
import { fileTreeIconStateClassName, fileTreeRowStateClassName } from '@/lib/file-tree-row-state';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileSnapshotVm } from '@/types';
import type { FileWorkspaceLayoutVm } from '@/types';
import { conversationDirectoryWorkspaceDataKey, useRightWorkspaceCommands, type ConversationDirectoryWorkspaceResource } from './right-workspace-context';
import { WorkspaceFileEditor } from './files/WorkspaceFileEditor';
import { FileWorkspaceSplitLayout } from './files/FileWorkspacePanel';
import { WorkspaceDirectoryContextMenu } from './files/WorkspaceDirectoryContextMenu';
import { isMarkdownDocumentPath } from './files/markdown-document';
import { ReadonlyMarkdownWorkspaceViewer } from './files/ReadonlyMarkdownWorkspaceViewer';

type Node = WorkspaceDirectoryEntryVm & { id: string; children: Node[] | null; loading: boolean; error: string | null };

function DirectoryReadError({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation();
  return <Alert variant="destructive" className="m-2 w-auto">
    <AlertDescription>{message}<Button variant="outline" size="sm" onClick={onRetry}>{t('common.retry')}</Button></AlertDescription>
  </Alert>;
}

interface ConversationDirectoryTreeRowContextValue {
  selectedPath: string | null;
  onLoadDirectory: (node: Node) => void;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
}

const ConversationDirectoryTreeRowContext = createContext<ConversationDirectoryTreeRowContextValue | null>(null);

function toNodes(entries: WorkspaceDirectoryEntryVm[]): Node[] {
  return entries.map((entry) => ({ ...entry, id: entry.relativePath, children: entry.kind === 'directory' ? null : [], loading: false, error: null }));
}

function update(nodes: Node[], id: string, callback: (node: Node) => Node): Node[] {
  return nodes.map((node) => node.id === id ? callback(node) : node.children ? { ...node, children: update(node.children, id, callback) } : node);
}

export function isConversationDirectorySelectedFile(selectedPath: string | null, entry: WorkspaceDirectoryEntryVm) {
  return entry.kind === 'file' && selectedPath === entry.canonicalPath;
}

function ConversationDirectoryTreeRow({ node, style }: NodeRendererProps<Node>) {
  const { t } = useTranslation();
  const context = useContext(ConversationDirectoryTreeRowContext);
  if (!context) return null;
  const directory = node.data.kind === 'directory';
  const selectedFile = isConversationDirectorySelectedFile(context.selectedPath, node.data);
  const Icon = directory ? (node.isOpen ? FolderOpen : Folder) : File;
  return (
    <ContextMenu dir="ltr">
      <ContextMenuTrigger asChild>
        <div style={style} className="flex h-full w-full min-w-0 items-center">
        <button
          type="button"
          aria-busy={node.data.loading || undefined}
          className={cn(
            'group flex h-full min-w-0 flex-1 items-center gap-1.5 rounded-md px-1.5 text-left text-xs outline-none transition-[background-color,color,box-shadow]',
            fileTreeRowStateClassName(selectedFile, node.isFocused),
          )}
          onClick={() => {
            if (directory) {
              node.toggle();
            } else {
              context.onOpenFile(node.data);
            }
          }}
        >
          <span style={{ width: node.level * 14 }} className="shrink-0" aria-hidden="true" />
          {directory ? (
            node.data.loading ? <LoaderCircle className="size-3 animate-spin" /> : node.isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />
          ) : <span className="size-3" />}
          <Icon className={cn('size-3.5 shrink-0', fileTreeIconStateClassName(selectedFile))} />
          <span className="min-w-0 flex-1 truncate">{node.data.name}</span>
        </button>
        {node.data.error && <Tooltip><TooltipTrigger asChild>
          <Button size="icon" variant="ghost" className="size-7 shrink-0 text-destructive"
            aria-label={`${t('common.retry')}: ${node.data.name}`}
            onClick={() => { node.open(); context.onLoadDirectory(node.data); }}>
            <RotateCcw className="size-3.5" /><span role="alert" className="sr-only">{node.data.error}</span>
          </Button>
        </TooltipTrigger><TooltipContent>{node.data.error}</TooltipContent></Tooltip>}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent
        className="w-40 min-w-40 p-1"
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <WorkspaceDirectoryContextMenu
          canonicalPath={node.data.canonicalPath}
          relativePath={node.data.relativePath}
          onCopyFailed={context.onCopyFailed}
          onOpenInFileManager={context.onOpenInFileManager}
        />
      </ContextMenuContent>
    </ContextMenu>
  );
}

function ConversationDirectoryTree({
  roots,
  expandedPaths,
  onExpansionChange,
  loading,
  selectedPath,
  actionFailure,
  onLoadDirectory,
  onOpenFile,
  onCopyFailed,
  onOpenInFileManager,
}: {
  roots: Node[];
  expandedPaths: Set<string>;
  onExpansionChange: (id: string, open: boolean) => void;
  loading: boolean;
  selectedPath: string | null;
  actionFailure: 'copy' | 'file-manager' | null;
  onLoadDirectory: (node: Node) => void;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
}) {
  const { t } = useTranslation();
  const treeRef = useRef<TreeApi<Node>>(null);
  const { ref: treeViewportRef, height: treeHeight } = useMeasuredElementHeight(320);
  const rowContext = useMemo(() => ({
    selectedPath,
    onLoadDirectory,
    onOpenFile,
    onCopyFailed,
    onOpenInFileManager,
  }), [onCopyFailed, onLoadDirectory, onOpenFile, onOpenInFileManager, selectedPath]);

  return (
    <div ref={treeViewportRef} className="relative flex h-full min-h-0 flex-col bg-muted/10 p-1.5">
      {actionFailure ? <div className="pointer-events-none absolute right-2 top-2 z-20 rounded-md border border-destructive/20 bg-popover/95 px-2 py-1 text-ui-caption text-destructive shadow-sm">{t(actionFailure === 'copy' ? 'workspace.filesPanel.pathCopyFailed' : 'workspace.filesPanel.fileManagerOpenFailed')}</div> : null}
      {loading ? <div className="flex items-center gap-2 p-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loading')}</div> : (
        <ConversationDirectoryTreeRowContext.Provider value={rowContext}>
          <Tree<Node> ref={treeRef} data={roots} width="100%" height={treeHeight} rowHeight={32} indent={14} idAccessor="id"
            childrenAccessor={(node) => node.kind === 'directory' ? node.children ?? [] : null}
            initialOpenState={Object.fromEntries([...expandedPaths].map(path => [path, true]))}
            onToggle={id => {
              const tree = treeRef.current;
              const node = tree?.get(id);
              if (!tree || !node || node.data.kind !== 'directory') return;
              const open = tree.isOpen(id);
              onExpansionChange(id, open);
              if (open) onLoadDirectory(node.data);
            }}
            openByDefault={false} disableDrag disableDrop disableEdit>
            {ConversationDirectoryTreeRow}
          </Tree>
        </ConversationDirectoryTreeRowContext.Provider>
      )}
    </div>
  );
}

export function ConversationDirectoryWorkspacePanel({ resource, layout }: { resource: ConversationDirectoryWorkspaceResource; layout: FileWorkspaceLayoutVm }) {
  const { t } = useTranslation();
  const { getResource, synchronizeResource } = useRightWorkspaceCommands();
  const [roots, setRoots] = useState<Node[]>([]);
  const [loading, setLoading] = useState(true);
  const [directoryError, setDirectoryError] = useState<string | null>(null);
  const [rootRevision, setRootRevision] = useState(0);
  const [selected, setSelected] = useState<WorkspaceDirectoryEntryVm | null>(null);
  const [snapshot, setSnapshot] = useState<WorkspaceFileSnapshotVm | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [actionFailure, setActionFailure] = useState<'copy' | 'file-manager' | null>(null);
  const actionFailureTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fileReadGenerationRef = useRef(0);
  const directoryGenerationRef = useRef(0);
  const rootReadyRef = useRef(false);
  const pendingDirectoriesRef = useRef(new Set<string>());
  const expandedPathsRef = useRef(new Set(resource.expandedPaths ?? []));
  const resourceRef = useRef(resource);
  resourceRef.current = resource;
  const dataKey = conversationDirectoryWorkspaceDataKey(resource.locator);
  const showActionFailure = useCallback((failure: 'copy' | 'file-manager') => {
    if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
    setActionFailure(failure);
    actionFailureTimerRef.current = setTimeout(() => setActionFailure(null), 1_500);
  }, []);
  const input = useCallback((relativePath = '') => ({ ...resource.locator, relativePath }), [
    resource.locator.attemptId,
    resource.locator.nodeId,
    resource.locator.outerAttemptId,
    resource.locator.outerNodeId,
    resource.locator.projectId,
    resource.locator.roundId,
    resource.locator.runId,
    resource.locator.taskId,
  ]);
  const readSelection = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    const generation = ++fileReadGenerationRef.current;
    setSelected(entry);
    setSnapshot(null);
    setFileError(null);
    void readConversationDirectoryFile(input(entry.relativePath)).then((nextSnapshot) => {
      if (fileReadGenerationRef.current === generation) setSnapshot(nextSnapshot);
    }).catch(reason => {
      if (fileReadGenerationRef.current === generation) setFileError(displayAppError(t, reason));
    });
  }, [input, t]);
  useEffect(() => () => { if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current); }, []);
  useEffect(() => {
    let cancelled = false;
    directoryGenerationRef.current++;
    rootReadyRef.current = false;
    pendingDirectoriesRef.current.clear();
    expandedPathsRef.current = new Set(resourceRef.current.expandedPaths ?? []);
    fileReadGenerationRef.current += 1;
    setRoots([]);
    setLoading(true);
    setDirectoryError(null);
    setSelected(null);
    setSnapshot(null);
    setFileError(null);
    const restored = resourceRef.current.selectedFile;
    if (restored) readSelection(restored);
    void listConversationDirectory(input())
      .then((entries) => { if (!cancelled) { rootReadyRef.current = true; setRoots(toNodes(entries)); } })
      .catch(reason => { if (!cancelled) setDirectoryError(displayAppError(t, reason)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; rootReadyRef.current = false; fileReadGenerationRef.current++; directoryGenerationRef.current++; pendingDirectoriesRef.current.clear(); };
  }, [dataKey, input, readSelection, rootRevision, t]);
  const load = useCallback(async (node: Node) => {
    if (!rootReadyRef.current || node.kind !== 'directory' || node.loading || node.children !== null || pendingDirectoriesRef.current.has(node.id)) return;
    const generation = directoryGenerationRef.current;
    pendingDirectoriesRef.current.add(node.id);
    setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: true, error: null })));
    try {
      const entries = await listConversationDirectory(input(node.relativePath));
      if (directoryGenerationRef.current !== generation) return;
      setRoots((current) => update(current, node.id, (value) => ({ ...value, children: toNodes(entries), loading: false })));
    } catch (reason) {
      if (directoryGenerationRef.current !== generation) return;
      setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: false, error: displayAppError(t, reason) })));
    } finally {
      if (directoryGenerationRef.current === generation) pendingDirectoriesRef.current.delete(node.id);
    }
  }, [input, t]);
  useEffect(() => {
    const visit = (nodes: Node[]) => {
      for (const node of nodes) {
        if (node.kind !== 'directory' || !expandedPathsRef.current.has(node.id)) continue;
        if (node.children) visit(node.children);
        else if (!node.loading && !node.error) void load(node);
      }
    };
    visit(roots);
  }, [roots, load]);
  const onExpansionChange = useCallback((id: string, open: boolean) => {
    if (open) expandedPathsRef.current.add(id); else expandedPathsRef.current.delete(id);
    const current = getResource(resourceRef.current.key);
    if (current?.kind === 'conversation-directory' && conversationDirectoryWorkspaceDataKey(current.locator) === dataKey) {
      synchronizeResource({ ...current, expandedPaths: [...expandedPathsRef.current] });
    }
  }, [dataKey, getResource, synchronizeResource]);
  const openInManager = useCallback((relativePath: string) => void openConversationDirectoryPathInFileManager(input(relativePath)).catch(() => showActionFailure('file-manager')), [input, showActionFailure]);
  const openFile = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    readSelection(entry);
    const current = getResource(resourceRef.current.key);
    if (current?.kind === 'conversation-directory' && conversationDirectoryWorkspaceDataKey(current.locator) === dataKey) {
      synchronizeResource({ ...current, selectedFile: entry });
    }
  }, [dataKey, getResource, readSelection, synchronizeResource]);
  const onCopyFailed = useCallback(() => showActionFailure('copy'), [showActionFailure]);
  const content = !selected ? <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.chooseFromTree')}</div>
    : fileError ? <DirectoryReadError message={fileError} onRetry={() => readSelection(selected)} />
      : !snapshot ? <div className="flex h-full items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loadingFile')}</div>
      : snapshot.kind === 'text' ? (isMarkdownDocumentPath(selected.canonicalPath)
        ? <ReadonlyMarkdownWorkspaceViewer documentKey={`${resource.key}:${selected.canonicalPath}`} value={snapshot.content} fileSnapshot={snapshot} />
        : <WorkspaceFileEditor documentKey={`${resource.key}:${selected.canonicalPath}`} value={snapshot.content} editable={false} language={snapshot.language} highlight contentRevision={0} target={null} targetRevision={0} onChange={() => undefined} onSave={() => undefined} initialStateJson={null} onPersistState={() => undefined} />)
        : snapshot.kind === 'image' ? <div className="flex h-full items-center justify-center overflow-auto p-4"><img src={workspaceFilePreviewUrl(snapshot.previewGrant.token)} alt={snapshot.name} className="max-h-full max-w-full object-contain" /></div>
          : <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.unsupportedTitle')}</div>;
  const tree = (showContent: () => void) => directoryError ? <DirectoryReadError message={directoryError} onRetry={() => setRootRevision(value => value + 1)} />
    : <ConversationDirectoryTree roots={roots} expandedPaths={expandedPathsRef.current} onExpansionChange={onExpansionChange} loading={loading} selectedPath={selected?.canonicalPath ?? null} actionFailure={actionFailure} onLoadDirectory={load} onOpenFile={entry => { openFile(entry); showContent(); }} onCopyFailed={onCopyFailed} onOpenInFileManager={openInManager} />;
  return <FileWorkspaceSplitLayout layout={layout} hasFile={Boolean(selected)} selectedFileKey={selected?.canonicalPath ?? null} content={content} tree={tree} treeWidth={null} onTreeWidthChange={() => undefined} />;
}
