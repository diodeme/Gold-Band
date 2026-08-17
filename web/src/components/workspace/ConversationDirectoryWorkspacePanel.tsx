import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, FolderOpen, LoaderCircle } from 'lucide-react';
import { Tree, type NodeRendererProps } from 'react-arborist';
import { useTranslation } from 'react-i18next';
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu';
import { cn } from '@/lib/utils';
import { useMeasuredElementHeight } from '@/hooks/use-measured-element-height';
import { listConversationDirectory, openConversationDirectoryPathInFileManager, readConversationDirectoryFile, workspaceFilePreviewUrl } from '@/api';
import { fileTreeIconStateClassName, fileTreeRowStateClassName } from '@/lib/file-tree-row-state';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileSnapshotVm } from '@/types';
import type { FileWorkspaceLayoutVm } from '@/types';
import { conversationDirectoryWorkspaceDataKey, type ConversationDirectoryWorkspaceResource } from './right-workspace-context';
import { WorkspaceFileEditor } from './files/WorkspaceFileEditor';
import { FileWorkspaceSplitLayout } from './files/FileWorkspacePanel';
import { WorkspaceDirectoryContextMenu } from './files/WorkspaceDirectoryContextMenu';
import { isMarkdownDocumentPath } from './files/markdown-document';
import { ReadonlyMarkdownWorkspaceViewer } from './files/ReadonlyMarkdownWorkspaceViewer';

type Node = WorkspaceDirectoryEntryVm & { id: string; children: Node[] | null; loading: boolean };

interface ConversationDirectoryTreeRowContextValue {
  selectedPath: string | null;
  onLoadDirectory: (node: Node) => void;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
}

const ConversationDirectoryTreeRowContext = createContext<ConversationDirectoryTreeRowContextValue | null>(null);

function toNodes(entries: WorkspaceDirectoryEntryVm[]): Node[] {
  return entries.map((entry) => ({ ...entry, id: entry.relativePath, children: entry.kind === 'directory' ? null : [], loading: false }));
}

function update(nodes: Node[], id: string, callback: (node: Node) => Node): Node[] {
  return nodes.map((node) => node.id === id ? callback(node) : node.children ? { ...node, children: update(node.children, id, callback) } : node);
}

export function isConversationDirectorySelectedFile(selectedPath: string | null, entry: WorkspaceDirectoryEntryVm) {
  return entry.kind === 'file' && selectedPath === entry.canonicalPath;
}

function ConversationDirectoryTreeRow({ node, style }: NodeRendererProps<Node>) {
  const context = useContext(ConversationDirectoryTreeRowContext);
  if (!context) return null;
  const directory = node.data.kind === 'directory';
  const selectedFile = isConversationDirectorySelectedFile(context.selectedPath, node.data);
  const Icon = directory ? (node.isOpen ? FolderOpen : Folder) : File;
  return (
    <ContextMenu dir="ltr">
      <ContextMenuTrigger asChild>
        <button
          style={style}
          type="button"
          className={cn(
            'group flex h-full w-full items-center gap-1.5 rounded-md px-1.5 text-left text-xs outline-none transition-[background-color,color,box-shadow]',
            fileTreeRowStateClassName(selectedFile, node.isFocused),
          )}
          onClick={() => {
            if (directory) {
              node.toggle();
              context.onLoadDirectory(node.data);
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
  loading,
  selectedPath,
  actionFailure,
  onLoadDirectory,
  onOpenFile,
  onCopyFailed,
  onOpenInFileManager,
}: {
  roots: Node[];
  loading: boolean;
  selectedPath: string | null;
  actionFailure: 'copy' | 'file-manager' | null;
  onLoadDirectory: (node: Node) => void;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
}) {
  const { t } = useTranslation();
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
          <Tree<Node> data={roots} width="100%" height={treeHeight} rowHeight={32} indent={14} idAccessor="id" childrenAccessor={(node) => node.children} openByDefault={false} disableDrag disableDrop disableEdit>
            {ConversationDirectoryTreeRow}
          </Tree>
        </ConversationDirectoryTreeRowContext.Provider>
      )}
    </div>
  );
}

export function ConversationDirectoryWorkspacePanel({ resource, layout }: { resource: ConversationDirectoryWorkspaceResource; layout: FileWorkspaceLayoutVm }) {
  const { t } = useTranslation();
  const [roots, setRoots] = useState<Node[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<WorkspaceDirectoryEntryVm | null>(null);
  const [snapshot, setSnapshot] = useState<WorkspaceFileSnapshotVm | null>(null);
  const [actionFailure, setActionFailure] = useState<'copy' | 'file-manager' | null>(null);
  const actionFailureTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fileReadGenerationRef = useRef(0);
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
  useEffect(() => () => { if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current); }, []);
  useEffect(() => {
    let cancelled = false;
    fileReadGenerationRef.current += 1;
    setRoots([]);
    setLoading(true);
    setSelected(null);
    setSnapshot(null);
    void listConversationDirectory(input())
      .then((entries) => { if (!cancelled) setRoots(toNodes(entries)); })
      .catch(() => { if (!cancelled) setRoots([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [dataKey, input]);
  const load = useCallback(async (node: Node) => {
    if (node.kind !== 'directory' || node.loading || node.children !== null) return;
    setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: true })));
    try {
      const entries = await listConversationDirectory(input(node.relativePath));
      setRoots((current) => update(current, node.id, (value) => ({ ...value, children: toNodes(entries), loading: false })));
    } catch { setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: false }))); }
  }, [input]);
  const openInManager = useCallback((relativePath: string) => void openConversationDirectoryPathInFileManager(input(relativePath)).catch(() => showActionFailure('file-manager')), [input, showActionFailure]);
  const openFile = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    const generation = ++fileReadGenerationRef.current;
    setSelected(entry);
    setSnapshot(null);
    void readConversationDirectoryFile(input(entry.relativePath)).then((nextSnapshot) => {
      if (fileReadGenerationRef.current === generation) setSnapshot(nextSnapshot);
    });
  }, [input]);
  const onCopyFailed = useCallback(() => showActionFailure('copy'), [showActionFailure]);
  const content = !selected ? <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.chooseFromTree')}</div>
    : !snapshot ? <div className="flex h-full items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loadingFile')}</div>
      : snapshot.kind === 'text' ? (isMarkdownDocumentPath(selected.canonicalPath)
        ? <ReadonlyMarkdownWorkspaceViewer documentKey={`${resource.key}:${selected.canonicalPath}`} value={snapshot.content} />
        : <WorkspaceFileEditor documentKey={`${resource.key}:${selected.canonicalPath}`} value={snapshot.content} editable={false} language={snapshot.language} highlight contentRevision={0} target={null} targetRevision={0} onChange={() => undefined} onSave={() => undefined} initialStateJson={null} onPersistState={() => undefined} />)
        : snapshot.kind === 'image' ? <div className="flex h-full items-center justify-center overflow-auto p-4"><img src={workspaceFilePreviewUrl(snapshot.previewGrant.token)} alt={snapshot.name} className="max-h-full max-w-full object-contain" /></div>
          : <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.unsupportedTitle')}</div>;
  const tree = <ConversationDirectoryTree roots={roots} loading={loading} selectedPath={selected?.canonicalPath ?? null} actionFailure={actionFailure} onLoadDirectory={load} onOpenFile={openFile} onCopyFailed={onCopyFailed} onOpenInFileManager={openInManager} />;
  return <FileWorkspaceSplitLayout layout={layout} hasFile={Boolean(selected)} selectedFileKey={selected?.canonicalPath ?? null} content={content} tree={tree} treeWidth={null} onTreeWidthChange={() => undefined} />;
}
