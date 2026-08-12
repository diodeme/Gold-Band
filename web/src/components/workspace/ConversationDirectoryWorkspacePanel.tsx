import { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, FolderOpen, LoaderCircle } from 'lucide-react';
import { Tree, type NodeRendererProps } from 'react-arborist';
import { useTranslation } from 'react-i18next';
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu';
import { cn } from '@/lib/utils';
import { useMeasuredElementHeight } from '@/hooks/use-measured-element-height';
import { listConversationDirectory, openConversationDirectoryPathInFileManager, readConversationDirectoryFile, workspaceFilePreviewUrl } from '@/api';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileSnapshotVm } from '@/types';
import type { FileWorkspaceLayoutVm } from '@/types';
import type { ConversationDirectoryWorkspaceResource } from './right-workspace-context';
import { WorkspaceFileEditor } from './files/WorkspaceFileEditor';
import { FileWorkspaceSplitLayout } from './files/FileWorkspacePanel';
import { WorkspaceDirectoryContextMenu } from './files/WorkspaceDirectoryContextMenu';

type Node = WorkspaceDirectoryEntryVm & { id: string; children: Node[] | null; loading: boolean };

function toNodes(entries: WorkspaceDirectoryEntryVm[]): Node[] {
  return entries.map((entry) => ({ ...entry, id: entry.relativePath, children: entry.kind === 'directory' ? null : [], loading: false }));
}

function update(nodes: Node[], id: string, callback: (node: Node) => Node): Node[] {
  return nodes.map((node) => node.id === id ? callback(node) : node.children ? { ...node, children: update(node.children, id, callback) } : node);
}

export function isConversationDirectorySelectedFile(selectedPath: string | null, entry: WorkspaceDirectoryEntryVm) {
  return entry.kind === 'file' && selectedPath === entry.canonicalPath;
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

  return (
    <div ref={treeViewportRef} className="relative flex h-full min-h-0 flex-col bg-muted/10 p-1.5">
      {actionFailure ? <div className="pointer-events-none absolute right-2 top-2 z-20 rounded-md border border-destructive/20 bg-popover/95 px-2 py-1 text-[11px] text-destructive shadow-sm">{t(actionFailure === 'copy' ? 'workspace.filesPanel.pathCopyFailed' : 'workspace.filesPanel.fileManagerOpenFailed')}</div> : null}
      {loading ? <div className="flex items-center gap-2 p-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loading')}</div> : <Tree<Node> data={roots} width="100%" height={treeHeight} rowHeight={32} indent={14} idAccessor="id" childrenAccessor={(node) => node.children} openByDefault={false} disableDrag disableDrop disableEdit>{({ node, style }: NodeRendererProps<Node>) => {
        const directory = node.data.kind === 'directory'; const selectedFile = isConversationDirectorySelectedFile(selectedPath, node.data); const Icon = directory ? (node.isOpen ? FolderOpen : Folder) : File;
        return <ContextMenu dir="ltr"><ContextMenuTrigger asChild><button style={style} type="button" className={cn('group flex h-full w-full items-center gap-1.5 rounded-md px-1.5 text-left text-xs outline-none transition-[background-color,color,box-shadow]', selectedFile ? 'bg-gold-running/12 text-foreground shadow-[inset_2px_0_0_var(--gold-running)]' : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground', node.isFocused && !selectedFile && 'bg-accent/45 text-foreground')} onClick={() => { if (directory) { node.toggle(); onLoadDirectory(node.data); } else onOpenFile(node.data); }}><span style={{ width: node.level * 14 }} className="shrink-0" aria-hidden="true" />{directory ? (node.data.loading ? <LoaderCircle className="size-3 animate-spin" /> : node.isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />) : <span className="size-3" />}<Icon className={cn('size-3.5 shrink-0', selectedFile ? 'text-gold-running' : 'text-foreground/65 group-hover:text-foreground')} /><span className="min-w-0 flex-1 truncate">{node.data.name}</span></button></ContextMenuTrigger><ContextMenuContent className="w-40 min-w-40 p-1" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}><WorkspaceDirectoryContextMenu canonicalPath={node.data.canonicalPath} relativePath={node.data.relativePath} onCopyFailed={onCopyFailed} onOpenInFileManager={onOpenInFileManager} /></ContextMenuContent></ContextMenu>;
      }}</Tree>}
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
  const input = (relativePath = '') => ({ ...resource.locator, relativePath });
  const showActionFailure = useCallback((failure: 'copy' | 'file-manager') => {
    if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
    setActionFailure(failure);
    actionFailureTimerRef.current = setTimeout(() => setActionFailure(null), 1_500);
  }, []);
  useEffect(() => () => { if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current); }, []);
  useEffect(() => { void listConversationDirectory(input()).then((entries) => setRoots(toNodes(entries))).finally(() => setLoading(false)); }, [resource.key]);
  const load = async (node: Node) => {
    if (node.kind !== 'directory' || node.loading || node.children !== null) return;
    setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: true })));
    try {
      const entries = await listConversationDirectory(input(node.relativePath));
      setRoots((current) => update(current, node.id, (value) => ({ ...value, children: toNodes(entries), loading: false })));
    } catch { setRoots((current) => update(current, node.id, (value) => ({ ...value, loading: false }))); }
  };
  const openInManager = (relativePath: string) => void openConversationDirectoryPathInFileManager(input(relativePath)).catch(() => showActionFailure('file-manager'));
  const openFile = (entry: WorkspaceDirectoryEntryVm) => { setSelected(entry); setSnapshot(null); void readConversationDirectoryFile(input(entry.relativePath)).then(setSnapshot); };
  const content = !selected ? <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.chooseFromTree')}</div>
    : !snapshot ? <div className="flex h-full items-center justify-center gap-2 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loadingFile')}</div>
      : snapshot.kind === 'text' ? <WorkspaceFileEditor documentKey={`conversation-directory:${selected.canonicalPath}`} value={snapshot.content} editable={false} language={snapshot.language} highlight contentRevision={0} target={null} targetRevision={0} onChange={() => undefined} onSave={() => undefined} initialStateJson={null} onPersistState={() => undefined} />
        : snapshot.kind === 'image' ? <div className="flex h-full items-center justify-center overflow-auto p-4"><img src={workspaceFilePreviewUrl(snapshot.previewGrant.token)} alt={snapshot.name} className="max-h-full max-w-full object-contain" /></div>
          : <div className="flex h-full items-center justify-center text-xs text-muted-foreground">{t('workspace.filesPanel.unsupportedTitle')}</div>;
  const tree = <ConversationDirectoryTree roots={roots} loading={loading} selectedPath={selected?.canonicalPath ?? null} actionFailure={actionFailure} onLoadDirectory={(node) => { void load(node); }} onOpenFile={openFile} onCopyFailed={() => showActionFailure('copy')} onOpenInFileManager={openInManager} />;
  return <FileWorkspaceSplitLayout layout={layout} hasFile={Boolean(selected)} selectedFileKey={selected?.canonicalPath ?? null} content={content} tree={tree} treeWidth={null} onTreeWidthChange={() => undefined} />;
}
