import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, File, FileCode2, FilePlus, Folder, FolderOpen, FolderPlus, ListCollapse, ListTree, LoaderCircle, Search, X } from 'lucide-react';
import { Tree, type NodeRendererProps, type TreeApi } from 'react-arborist';
import { useTranslation } from 'react-i18next';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu';
import { Input } from '@/components/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { cn } from '@/lib/utils';
import { useMeasuredElementHeight } from '@/hooks/use-measured-element-height';
import { openWorkspacePathInFileManager } from '@/api';
import { composerWorkspaceFileRefFromEntry } from '@/lib/workspace-file-reference';
import { fileTreeIconStateClassName, fileTreeRowStateClassName } from '@/lib/file-tree-row-state';
import {
  type AddWorkspaceFileRefResult,
  useWorkspaceFileReferenceCommands,
  useWorkspaceFileReferencePresentation,
} from '../workspace-file-reference-bridge';
import type { WorkspaceDirectoryEntryVm, WorkspaceRootRef } from '@/types';
import {
  FILE_TREE_DRAFT_ID,
  fileExplorerStore,
  fileTreeView,
  fileTreeWithDraft,
  useFileExplorerSnapshot,
  type FileTreeDisplayMode,
  type FileTreeOperationResult,
  type FileTreeViewNode,
} from './file-explorer-store';
import { fileContentStore } from './file-content-store';
import { WorkspaceDirectoryContextMenu, type WorkspaceEntryMenuActions } from './WorkspaceDirectoryContextMenu';

interface WorkspaceFileTreeProps {
  root: WorkspaceRootRef;
  selectedPath: string | null;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
}

interface TreeRowContextValue {
  selectedPath: string | null;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
  onReferenceToConversation: ((entry: WorkspaceDirectoryEntryVm) => AddWorkspaceFileRefResult) | null;
  canReferenceToConversation: boolean;
  onContextMenuOpenChange: (open: boolean) => void;
  canActivateFile: () => boolean;
  displayMode: FileTreeDisplayMode;
  entryActions: WorkspaceEntryMenuActions | null;
  onMenuCloseAutoFocus: (event: Event) => void;
  renamingId: string | null;
  pendingPath: string | null;
  onSubmitName: (entry: FileTreeViewNode, name: string) => void;
  onCancelName: (entry: FileTreeViewNode) => void;
}

/** A tree edit requested from a context menu; applied once the menu has released focus. */
type PendingTreeEdit =
  | { kind: 'create'; parentRelativePath: string; entryKind: 'file' | 'directory' }
  | { kind: 'rename'; nodeId: string };

interface TreeActionFailure {
  messageKey: string;
  params?: Record<string, unknown>;
}

const ACTION_FAILURE_VISIBLE_MS = 1_500;
const OPERATION_FAILURE_VISIBLE_MS = 3_000;

export function treeCreateParentPath(entry: Pick<WorkspaceDirectoryEntryVm, 'kind' | 'relativePath'>) {
  if (entry.kind === 'directory') return entry.relativePath;
  const index = entry.relativePath.lastIndexOf('/');
  return index < 0 ? '' : entry.relativePath.slice(0, index);
}

/** Ctrl+Z (⌘Z on macOS) outside text inputs undoes the last tree operation. */
export function isTreeUndoShortcut(event: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'target'>) {
  if (!(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return false;
  if (event.key.toLowerCase() !== 'z') return false;
  const target = event.target as Partial<Pick<Element, 'closest'>> | null;
  return !target?.closest?.('input, textarea, [contenteditable]:not([contenteditable="false"])');
}

/** Preselect the base name so typing replaces it and keeps the extension. */
export function entryNameSelectionEnd(name: string, kind: string) {
  const extensionStart = name.lastIndexOf('.');
  return kind === 'file' && extensionStart > 0 ? extensionStart : name.length;
}

function operationFailure(result: FileTreeOperationResult): TreeActionFailure | null {
  if (result.status !== 'failed') return null;
  return { messageKey: `workspace.filesPanel.errors.${result.errorCode}`, params: result.params };
}

function TreeEntryNameEditor({ entry, pending, onSubmit, onCancel }: {
  entry: FileTreeViewNode;
  pending: boolean;
  onSubmit: (name: string) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState(entry.name);
  const settledRef = useRef(false);
  const settle = (submit: boolean) => {
    if (settledRef.current) return;
    settledRef.current = true;
    const name = value.trim();
    if (submit && name && name !== entry.name) onSubmit(name);
    else onCancel();
  };
  return (
    <Input
      autoFocus
      value={value}
      disabled={pending}
      aria-label={t('workspace.filesPanel.entryName')}
      className="h-6 min-w-0 flex-1 rounded-sm px-1.5 py-0 text-xs"
      onFocus={(event) => event.currentTarget.setSelectionRange(0, entryNameSelectionEnd(entry.name, entry.kind))}
      onChange={(event) => setValue(event.target.value)}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === 'Enter') settle(true);
        else if (event.key === 'Escape') settle(false);
      }}
      onBlur={() => settle(true)}
    />
  );
}

const TreeRowContext = createContext<TreeRowContextValue | null>(null);
const TREE_ROW_HEIGHT = 32;
const TREE_ROW_BASE_MIN_WIDTH = 190;
const TREE_ROW_INDENT = 14;

export function treeViewportContentHeight(clientHeight: number, paddingTop: number, paddingBottom: number) {
  return Math.max(1, Math.floor(clientHeight - paddingTop - paddingBottom));
}

const measureTreeViewportHeight = (element: HTMLDivElement) => {
  const style = getComputedStyle(element);
  return treeViewportContentHeight(
    element.clientHeight,
    Number.parseFloat(style.paddingTop) || 0,
    Number.parseFloat(style.paddingBottom) || 0,
  );
};

export function treeOverscanCount(viewportHeight: number, rowHeight = TREE_ROW_HEIGHT) {
  const visibleRows = Math.ceil(Math.max(1, viewportHeight) / Math.max(1, rowHeight));
  return Math.min(96, Math.max(24, visibleRows * 2));
}

export function treeRowMinimumWidth(level: number) {
  return TREE_ROW_BASE_MIN_WIDTH + Math.max(0, level) * TREE_ROW_INDENT;
}

export function treeRowOverflowStyle(displayMode: FileTreeDisplayMode, level: number) {
  return displayMode === 'compact'
    ? { width: 'max-content', minWidth: `max(100%, ${treeRowMinimumWidth(level)}px)` }
    : { minWidth: treeRowMinimumWidth(level) };
}

export function fileTreeDisplayModeToggle(displayMode: FileTreeDisplayMode) {
  return displayMode === 'compact'
    ? {
      currentIcon: 'compact' as const,
      targetMode: 'tree' as const,
      labelKey: 'workspace.filesPanel.switchToTreeView' as const,
    }
    : {
      currentIcon: 'tree' as const,
      targetMode: 'compact' as const,
      labelKey: 'workspace.filesPanel.switchToCompactView' as const,
    };
}

export { copyableAbsolutePath } from './WorkspaceDirectoryContextMenu';
export { copyableRelativePath } from './WorkspaceDirectoryContextMenu';
export { canReferenceWorkspaceFileToConversation } from './WorkspaceDirectoryContextMenu';

export function shouldActivateTreeFile(contextMenuOpen: boolean, suppressContextMenuActivation: boolean) {
  return !contextMenuOpen && !suppressContextMenuActivation;
}

function fileIcon(name: string) {
  return /\.(?:rs|js|jsx|ts|tsx|py|go|java|kt|c|cc|cpp|h|hpp|cs|swift|php|rb|sh|ps1|sql|json|ya?ml|toml|xml|html|css|scss|md)$/iu.test(name)
    ? FileCode2
    : File;
}

function TreeNodeRow({ style, node, dragHandle }: NodeRendererProps<FileTreeViewNode>) {
  const context = useContext(TreeRowContext);
  if (!context) return null;
  const entry = node.data;
  const isDirectory = entry.kind === 'directory';
  const Icon = isDirectory ? (node.isOpen ? FolderOpen : Folder) : fileIcon(entry.name);
  const isDraft = node.id === FILE_TREE_DRAFT_ID;
  const pending = context.pendingPath !== null && (isDraft || context.pendingPath === entry.relativePath);
  if (isDraft || context.renamingId === node.id) {
    return (
      <div
        ref={dragHandle}
        style={{ ...style, ...treeRowOverflowStyle('tree', node.level) }}
        className="flex h-full w-full items-center gap-1.5 rounded-md px-1.5 text-xs"
      >
        <span style={{ width: node.level * 14 }} className="shrink-0" aria-hidden="true" />
        <span className="flex size-4 shrink-0 items-center justify-center">
          {pending ? <LoaderCircle className="size-3 animate-spin" /> : null}
        </span>
        <Icon className="size-3.5 shrink-0 text-muted-foreground" />
        <TreeEntryNameEditor
          entry={entry}
          pending={pending}
          onSubmit={(name) => context.onSubmitName(entry, name)}
          onCancel={() => context.onCancelName(entry)}
        />
      </div>
    );
  }
  const row = (
    <div
      ref={dragHandle}
      style={{
        ...style,
        ...treeRowOverflowStyle(context.displayMode, node.level),
      }}
      aria-busy={pending || undefined}
      className={cn(
        'group flex h-full w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-xs outline-none transition-[background-color,color,box-shadow]',
        fileTreeRowStateClassName(context.selectedPath === entry.canonicalPath, node.isFocused),
        pending && 'opacity-60',
      )}
      onClick={(event) => {
        event.stopPropagation();
        node.select();
        if (isDirectory) node.toggle();
        else if (context.canActivateFile()) context.onOpenFile(entry);
      }}
      onDoubleClick={(event) => event.preventDefault()}
    >
      <span style={{ width: node.level * 14 }} className="shrink-0" aria-hidden="true" />
      {isDirectory || pending ? (
        <span className="flex size-4 shrink-0 items-center justify-center">
          {entry.loading || pending ? <LoaderCircle className="size-3 animate-spin" /> : node.isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        </span>
      ) : <span className="size-4 shrink-0" />}
      <Icon className={cn('size-3.5 shrink-0', fileTreeIconStateClassName(context.selectedPath === entry.canonicalPath))} />
      <span className={cn(
        context.displayMode === 'compact'
          ? 'shrink-0 whitespace-nowrap pr-2'
          : 'min-w-0 flex-1 truncate',
      )}>{entry.displayName}</span>
    </div>
  );
  return (
    <ContextMenu dir="ltr" onOpenChange={context.onContextMenuOpenChange}>
      {/* Keep the blank-area menu of the tree from opening underneath this row menu. */}
      <ContextMenuTrigger asChild onContextMenu={(event) => event.stopPropagation()}>{row}</ContextMenuTrigger>
      <ContextMenuContent
        className="w-40 min-w-40 p-1"
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
        onCloseAutoFocus={context.onMenuCloseAutoFocus}
      >
        <WorkspaceDirectoryContextMenu
          canonicalPath={entry.canonicalPath}
          relativePath={entry.relativePath}
          entry={entry}
          canReferenceToConversation={context.canReferenceToConversation}
          onCopyFailed={context.onCopyFailed}
          onOpenInFileManager={context.onOpenInFileManager}
          onReferenceToConversation={context.onReferenceToConversation ?? undefined}
          entryActions={context.pendingPath === null ? (context.entryActions ?? undefined) : undefined}
        />
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function WorkspaceFileTree({ root, selectedPath, onOpenFile }: WorkspaceFileTreeProps) {
  const { t } = useTranslation();
  const workspaceFileReferenceCommands = useWorkspaceFileReferenceCommands();
  const workspaceFileReferencePresentation = useWorkspaceFileReferencePresentation();
  const canReferenceToConversation = workspaceFileReferenceCommands?.available === true;
  const readOnly = useReadOnlyExperience();
  const snapshot = useFileExplorerSnapshot(root);
  const displayModeToggle = fileTreeDisplayModeToggle(snapshot.displayMode);
  const treeNodes = useMemo(
    () => fileTreeView(fileTreeWithDraft(snapshot.roots, snapshot.draft), snapshot.displayMode),
    [snapshot.displayMode, snapshot.draft, snapshot.roots],
  );
  const { ref, height } = useMeasuredElementHeight(320, measureTreeViewportHeight);
  const treeRef = useRef<TreeApi<FileTreeViewNode> | null>(null);
  const pendingRevealPathRef = useRef<string | null>(null);
  const restoringScrollRef = useRef(true);
  const syncingExpandedRef = useRef(false);
  const contextMenuOpenRef = useRef(false);
  const suppressContextMenuActivationRef = useRef(false);
  const contextMenuFrameRef = useRef<number | null>(null);
  const [actionFailure, setActionFailure] = useState<TreeActionFailure | null>(null);
  const actionFailureTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const asideRef = useRef<HTMLElement | null>(null);
  const pendingEditRef = useRef<PendingTreeEdit | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ entry: WorkspaceDirectoryEntryVm; unsaved: boolean } | null>(null);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    void fileExplorerStore.loadRoot(root);
    restoringScrollRef.current = true;
    const savedScrollTop = fileExplorerStore.snapshot(root).treeScrollTop;
    let settleFrame: number | null = null;
    const frame = requestAnimationFrame(() => {
      treeRef.current?.scrollToOffset(savedScrollTop);
      settleFrame = requestAnimationFrame(() => { restoringScrollRef.current = false; });
    });
    return () => {
      cancelAnimationFrame(frame);
      if (settleFrame !== null) cancelAnimationFrame(settleFrame);
      if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
      if (contextMenuFrameRef.current !== null) cancelAnimationFrame(contextMenuFrameRef.current);
    };
  }, [root, snapshot.displayMode]);

  useEffect(() => {
    const tree = treeRef.current;
    if (!tree) return;
    syncingExpandedRef.current = true;
    try {
      for (const node of visibleTreeNodes(treeNodes)) {
        if (snapshot.expanded.has(node.relativePath)) tree.open(node.id);
      }
    } finally {
      syncingExpandedRef.current = false;
    }
  }, [snapshot.expanded, treeNodes]);

  useEffect(() => {
    if (!selectedPath) {
      fileExplorerStore.takeSelectionReveal(root, null);
      pendingRevealPathRef.current = null;
      return;
    }
    if (fileExplorerStore.takeSelectionReveal(root, selectedPath)) {
      pendingRevealPathRef.current = selectedPath;
    }
  }, [root, selectedPath]);

  useEffect(() => {
    const reveal = consumePendingTreeReveal(pendingRevealPathRef.current, treeNodes);
    if (!reveal.targetId) return;
    pendingRevealPathRef.current = reveal.pendingPath;
    const frame = requestAnimationFrame(() => treeRef.current?.scrollTo(reveal.targetId!));
    return () => cancelAnimationFrame(frame);
  }, [selectedPath, treeNodes]);

  useEffect(() => {
    if (!snapshot.draft) return;
    const frame = requestAnimationFrame(() => treeRef.current?.scrollTo(FILE_TREE_DRAFT_ID));
    return () => cancelAnimationFrame(frame);
  }, [snapshot.draft]);

  const showActionFailure = useCallback((failure: TreeActionFailure, visibleMs = ACTION_FAILURE_VISIBLE_MS) => {
    if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
    setActionFailure(failure);
    actionFailureTimerRef.current = setTimeout(() => setActionFailure(null), visibleMs);
  }, []);
  const reportOperation = useCallback((result: FileTreeOperationResult) => {
    const failure = operationFailure(result);
    if (failure) showActionFailure(failure, OPERATION_FAILURE_VISIBLE_MS);
  }, [showActionFailure]);
  const onCopyFailed = useCallback(() => {
    showActionFailure({ messageKey: 'workspace.filesPanel.pathCopyFailed' });
  }, [showActionFailure]);
  const onOpenInFileManager = useCallback((relativePath: string) => {
    void openWorkspacePathInFileManager(root, relativePath).catch(() => {
      showActionFailure({ messageKey: 'workspace.filesPanel.fileManagerOpenFailed' });
    });
  }, [root, showActionFailure]);
  const focusTree = useCallback(() => asideRef.current?.focus({ preventScroll: true }), []);
  const treeNodesRef = useRef(treeNodes);
  treeNodesRef.current = treeNodes;
  const entryActions = useMemo<WorkspaceEntryMenuActions | null>(() => (readOnly ? null : {
    onCreate: (entry, kind) => {
      pendingEditRef.current = { kind: 'create', parentRelativePath: treeCreateParentPath(entry), entryKind: kind };
    },
    onRename: (entry) => {
      const node = findTreeNodeByRelativePath(treeNodesRef.current, entry.relativePath);
      if (node) pendingEditRef.current = { kind: 'rename', nodeId: node.id };
    },
    onDelete: (entry) => {
      setDeleteTarget({ entry, unsaved: fileContentStore.hasUnsavedWithin(root.projectId, entry.canonicalPath) });
    },
  }), [root, readOnly]);
  /** Radix restores focus to the trigger on close, which would blur a freshly mounted name editor. */
  const onMenuCloseAutoFocus = useCallback((event: Event) => {
    const edit = pendingEditRef.current;
    if (!edit) return;
    pendingEditRef.current = null;
    event.preventDefault();
    if (edit.kind === 'rename') setRenamingId(edit.nodeId);
    else void fileExplorerStore.startDraft(root, edit.parentRelativePath, edit.entryKind);
  }, [root]);
  const onSubmitName = useCallback((entry: FileTreeViewNode, name: string) => {
    void (async () => {
      if (entry.id === FILE_TREE_DRAFT_ID) {
        const result = await fileExplorerStore.createEntry(root, name);
        reportOperation(result);
        if (result.status === 'done' && result.entry?.kind === 'file') onOpenFile(result.entry);
      } else {
        const result = await fileExplorerStore.renameEntry(root, entry, name);
        setRenamingId(null);
        reportOperation(result);
      }
      focusTree();
    })();
  }, [focusTree, onOpenFile, root, reportOperation]);
  const onCancelName = useCallback((entry: FileTreeViewNode) => {
    if (entry.id === FILE_TREE_DRAFT_ID) fileExplorerStore.cancelDraft(root);
    else setRenamingId(null);
    focusTree();
  }, [focusTree, root]);
  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    const result = await fileExplorerStore.deleteEntry(root, deleteTarget.entry);
    setDeleting(false);
    setDeleteTarget(null);
    reportOperation(result);
  }, [deleteTarget, root, reportOperation]);
  const onTreeKeyDown = useCallback((event: React.KeyboardEvent<HTMLElement>) => {
    if (readOnly || !isTreeUndoShortcut(event.nativeEvent)) return;
    event.preventDefault();
    void fileExplorerStore.undo(root).then(reportOperation);
  }, [root, readOnly, reportOperation]);
  const onReferenceToConversation = useCallback((entry: WorkspaceDirectoryEntryVm) => {
    if (!workspaceFileReferenceCommands?.available) return { kind: 'unavailable' } as const;
    return workspaceFileReferenceCommands.addWorkspaceFileRef(
      composerWorkspaceFileRefFromEntry(root.projectId, entry),
      workspaceFileReferencePresentation,
    );
  }, [root, workspaceFileReferenceCommands, workspaceFileReferencePresentation]);
  const onContextMenuOpenChange = useCallback((open: boolean) => {
    contextMenuOpenRef.current = open;
    suppressContextMenuActivationRef.current = true;
    if (contextMenuFrameRef.current !== null) cancelAnimationFrame(contextMenuFrameRef.current);
    if (!open) {
      contextMenuFrameRef.current = requestAnimationFrame(() => {
        suppressContextMenuActivationRef.current = false;
        contextMenuFrameRef.current = null;
      });
    }
  }, []);
  const canActivateFile = useCallback(() => shouldActivateTreeFile(
    contextMenuOpenRef.current,
    suppressContextMenuActivationRef.current,
  ), []);
  const contextValue = useMemo(() => ({
    selectedPath,
    onOpenFile,
    onCopyFailed,
    onOpenInFileManager,
    onReferenceToConversation,
    canReferenceToConversation,
    onContextMenuOpenChange,
    canActivateFile,
    displayMode: snapshot.displayMode,
    entryActions,
    onMenuCloseAutoFocus,
    renamingId,
    pendingPath: snapshot.pendingPath,
    onSubmitName,
    onCancelName,
  }), [canActivateFile, canReferenceToConversation, entryActions, onCancelName, onContextMenuOpenChange, onCopyFailed, onMenuCloseAutoFocus, onOpenFile, onOpenInFileManager, onReferenceToConversation, onSubmitName, renamingId, selectedPath, snapshot.displayMode, snapshot.pendingPath]);
  const searchEntries = snapshot.searchResult?.entries ?? [];
  const searching = snapshot.searchQuery.trim().length > 0;

  return (
    <aside
      ref={asideRef}
      tabIndex={-1}
      onKeyDown={onTreeKeyDown}
      className="relative flex h-full min-h-0 flex-col bg-muted/10 outline-none"
      aria-label={t('workspace.filesPanel.workspaceTree')}
    >
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border/50 p-2">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            variant="toolbar"
            value={snapshot.searchQuery}
            onChange={(event) => fileExplorerStore.setSearchQuery(root, event.target.value)}
            placeholder={t('workspace.filesPanel.filterPlaceholder')}
            className="h-8 pl-8 pr-8 text-xs"
          />
          {snapshot.searchQuery ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="absolute right-1 top-1/2 -translate-y-1/2"
              onClick={() => fileExplorerStore.setSearchQuery(root, '')}
              aria-label={t('workspace.filesPanel.clearSearch')}
            >
              <X className="size-3" />
            </Button>
          ) : null}
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="size-8 text-muted-foreground"
              onClick={() => fileExplorerStore.setDisplayMode(root, displayModeToggle.targetMode)}
              aria-label={t(displayModeToggle.labelKey)}
            >
              {displayModeToggle.currentIcon === 'tree' ? <ListTree className="size-4" /> : <ListCollapse className="size-4" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" sideOffset={6}>
            {t(displayModeToggle.labelKey)}
          </TooltipContent>
        </Tooltip>
      </div>
      {actionFailure ? (
        <div role="status" className="pointer-events-none absolute right-2 top-12 z-20 max-w-[calc(100%-1rem)] rounded-md border border-destructive/20 bg-popover/95 px-2 py-1 text-ui-caption text-destructive shadow-sm">
          {t(actionFailure.messageKey, { ...actionFailure.params, defaultValue: t('workspace.filesPanel.operationFailed') })}
        </div>
      ) : null}
      <div ref={ref} className="min-h-0 flex-1 overflow-hidden p-1.5">
        {snapshot.status === 'loading' || snapshot.searchStatus === 'loading' ? (
          <div className="flex items-center gap-2 px-2 py-3 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loading')}</div>
        ) : snapshot.status === 'error' || snapshot.searchStatus === 'error' ? (
          <div className="space-y-2 px-2 py-3 text-xs text-muted-foreground">
            <p>{t(`workspace.filesPanel.errors.${snapshot.errorCode}`, snapshot.errorCode ?? '')}</p>
            <Button size="sm" variant="outline" onClick={() => void fileExplorerStore.loadRoot(root, true)}>{t('workspace.filesPanel.retry')}</Button>
          </div>
        ) : searching ? (
          <div className="gold-themed-scrollbar h-full overflow-auto py-1">
            {searchEntries.map((entry) => {
              const Icon = fileIcon(entry.name);
              return (
                <ContextMenu key={entry.canonicalPath} dir="ltr" onOpenChange={onContextMenuOpenChange}>
                  <ContextMenuTrigger asChild>
                    <button
                      type="button"
                      className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left text-xs text-muted-foreground hover:bg-muted/45 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      onClick={() => {
                        void fileExplorerStore.revealFile(root, entry.relativePath).then(() => onOpenFile(entry));
                      }}
                    >
                      <Icon className="mt-0.5 size-3.5 shrink-0" />
                      <span className="min-w-0"><span className="block truncate text-foreground">{entry.name}</span><span className="block truncate text-ui-micro">{entry.relativePath}</span></span>
                    </button>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-40 min-w-40 p-1" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}>
                    <WorkspaceDirectoryContextMenu
                      canonicalPath={entry.canonicalPath}
                      relativePath={entry.relativePath}
                      entry={entry}
                      canReferenceToConversation={canReferenceToConversation}
                      onCopyFailed={onCopyFailed}
                      onOpenInFileManager={onOpenInFileManager}
                      onReferenceToConversation={onReferenceToConversation}
                    />
                  </ContextMenuContent>
                </ContextMenu>
              );
            })}
            {snapshot.searchStatus === 'ready' && searchEntries.length === 0 ? <p className="px-2 py-3 text-xs text-muted-foreground">{t('workspace.filesPanel.noSearchResults')}</p> : null}
            {snapshot.searchResult?.truncated ? <p className="px-2 py-2 text-ui-caption text-muted-foreground">{t('workspace.filesPanel.searchTruncated')}</p> : null}
          </div>
        ) : (
          <TreeRowContext.Provider value={contextValue}>
            <ContextMenu dir="ltr">
            <ContextMenuTrigger asChild disabled={readOnly || snapshot.pendingPath !== null}>
            <div className="h-full">
            <Tree<FileTreeViewNode>
              key={snapshot.displayMode}
              ref={treeRef}
              data={treeNodes}
              width="100%"
              height={height}
              rowHeight={TREE_ROW_HEIGHT}
              indent={14}
              overscanCount={treeOverscanCount(height)}
              idAccessor="id"
              childrenAccessor={(entry) => entry.kind === 'directory' ? (entry.children ?? []) : null}
              initialOpenState={treeOpenState(treeNodes, snapshot.expanded)}
              openByDefault={false}
              disableDrag
              disableDrop
              disableEdit
              disableMultiSelection
              selection={undefined}
              onToggle={(id) => {
                if (syncingExpandedRef.current) return;
                const entry = findTreeNodeById(treeNodes, id);
                if (entry) void fileExplorerStore.toggleDirectory(root, entry.relativePath, !snapshot.expanded.has(entry.relativePath));
              }}
              className="gold-themed-scrollbar !overflow-x-auto overscroll-contain [overflow-anchor:none] [scrollbar-gutter:stable]"
              rowClassName="w-full px-0.5"
              onScroll={({ scrollOffset, scrollUpdateWasRequested }) => {
                if (!restoringScrollRef.current && !scrollUpdateWasRequested) {
                  fileExplorerStore.setTreeScrollTop(root, scrollOffset);
                }
              }}
              onActivate={(node) => {
                if (node.data.kind === 'file' && canActivateFile()) onOpenFile(node.data);
              }}
              aria-label={t('workspace.filesPanel.workspaceTree')}
            >
              {TreeNodeRow}
            </Tree>
            </div>
            </ContextMenuTrigger>
            <ContextMenuContent className="w-40 min-w-40 p-1" onCloseAutoFocus={onMenuCloseAutoFocus}>
              <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => { pendingEditRef.current = { kind: 'create', parentRelativePath: '', entryKind: 'file' }; }}>
                <FilePlus className="size-3.5" />
                {t('workspace.filesPanel.newFile')}
              </ContextMenuItem>
              <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => { pendingEditRef.current = { kind: 'create', parentRelativePath: '', entryKind: 'directory' }; }}>
                <FolderPlus className="size-3.5" />
                {t('workspace.filesPanel.newFolder')}
              </ContextMenuItem>
            </ContextMenuContent>
            </ContextMenu>
          </TreeRowContext.Provider>
        )}
      </div>
      <AlertDialog open={deleteTarget !== null} onOpenChange={(open) => { if (!open && !deleting) setDeleteTarget(null); }}>
        <AlertDialogContent
          size="sm"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            focusTree();
          }}
        >
          <AlertDialogHeader className="gap-1.5">
            <AlertDialogTitle className="break-all text-base font-semibold">
              {t('workspace.filesPanel.deleteTitle', { name: deleteTarget?.entry.name ?? '' })}
            </AlertDialogTitle>
            <AlertDialogDescription className="text-sm leading-6">
              {t(deleteTarget?.entry.kind === 'directory'
                ? 'workspace.filesPanel.deleteDirectoryDescription'
                : 'workspace.filesPanel.deleteFileDescription')}
              {deleteTarget?.unsaved ? <span className="mt-1 block text-destructive">{t('workspace.filesPanel.deleteUnsavedWarning')}</span> : null}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter className="gap-1.5">
            <AlertDialogCancel variant="ghost" size="sm" disabled={deleting}>
              {t('workspace.filesPanel.cancel')}
            </AlertDialogCancel>
            <AlertDialogAction
              variant="ghost"
              size="sm"
              className="bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive"
              disabled={deleting}
              onClick={(event) => {
                event.preventDefault();
                void confirmDelete();
              }}
            >
              {deleting ? <LoaderCircle className="size-4 animate-spin" /> : null}
              {t(deleting ? 'workspace.filesPanel.deleting' : 'workspace.filesPanel.delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </aside>
  );
}

export function consumePendingTreeReveal(pendingPath: string | null, nodes: FileTreeViewNode[]) {
  if (!pendingPath) return { pendingPath: null, targetId: null };
  const selected = findTreeNodeByCanonicalPath(nodes, pendingPath);
  return selected
    ? { pendingPath: null, targetId: selected.id }
    : { pendingPath, targetId: null };
}

function findTreeNodeByCanonicalPath(nodes: FileTreeViewNode[], canonicalPath: string): FileTreeViewNode | null {
  const target = normalizeCanonicalPath(canonicalPath);
  for (const node of nodes) {
    if (normalizeCanonicalPath(node.canonicalPath) === target) return node;
    if (node.children) {
      const child = findTreeNodeByCanonicalPath(node.children, canonicalPath);
      if (child) return child;
    }
  }
  return null;
}

function findTreeNodeByRelativePath(nodes: FileTreeViewNode[], relativePath: string): FileTreeViewNode | null {
  for (const node of visibleTreeNodes(nodes)) {
    if (node.relativePath === relativePath) return node;
  }
  return null;
}

function findTreeNodeById(nodes: FileTreeViewNode[], id: string): FileTreeViewNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.children) {
      const child = findTreeNodeById(node.children, id);
      if (child) return child;
    }
  }
  return null;
}

function* visibleTreeNodes(nodes: FileTreeViewNode[]): Generator<FileTreeViewNode> {
  for (const node of nodes) {
    yield node;
    if (node.children) yield* visibleTreeNodes(node.children);
  }
}

export function treeOpenState(nodes: FileTreeViewNode[], expanded: ReadonlySet<string>) {
  return Object.fromEntries([...visibleTreeNodes(nodes)].map((node) => [node.id, expanded.has(node.relativePath)]));
}

function normalizeCanonicalPath(path: string) {
  const normalized = path.replaceAll('\\', '/');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}
