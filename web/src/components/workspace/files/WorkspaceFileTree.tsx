import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, File, FileCode2, Folder, FolderOpen, ListCollapse, ListTree, LoaderCircle, Search, X } from 'lucide-react';
import { Tree, type NodeRendererProps, type TreeApi } from 'react-arborist';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from '@/components/ui/context-menu';
import { Input } from '@/components/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { useMeasuredElementHeight } from '@/hooks/use-measured-element-height';
import { openWorkspacePathInFileManager } from '@/api';
import type { WorkspaceDirectoryEntryVm } from '@/types';
import {
  fileExplorerStore,
  fileTreeView,
  useFileExplorerSnapshot,
  type FileTreeDisplayMode,
  type FileTreeViewNode,
} from './file-explorer-store';
import { WorkspaceDirectoryContextMenu } from './WorkspaceDirectoryContextMenu';

interface WorkspaceFileTreeProps {
  projectId: string;
  selectedPath: string | null;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
}

interface TreeRowContextValue {
  selectedPath: string | null;
  onOpenFile: (entry: WorkspaceDirectoryEntryVm) => void;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
  onContextMenuOpenChange: (open: boolean) => void;
  canActivateFile: () => boolean;
  displayMode: FileTreeDisplayMode;
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
  const row = (
    <div
      ref={dragHandle}
      style={{
        ...style,
        ...treeRowOverflowStyle(context.displayMode, node.level),
      }}
      className={cn(
        'group flex h-full w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 text-xs outline-none transition-[background-color,color,box-shadow]',
        context.selectedPath === entry.canonicalPath
          ? 'bg-gold-running/12 text-foreground shadow-[inset_2px_0_0_var(--gold-running)]'
          : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
        node.isFocused && context.selectedPath !== entry.canonicalPath && 'bg-accent/45 text-foreground',
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
      {isDirectory ? (
        <span className="flex size-4 shrink-0 items-center justify-center">
          {entry.loading ? <LoaderCircle className="size-3 animate-spin" /> : node.isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        </span>
      ) : <span className="size-4 shrink-0" />}
      <Icon className={cn(
        'size-3.5 shrink-0',
        context.selectedPath === entry.canonicalPath
          ? 'text-gold-running'
          : 'text-foreground/65 group-hover:text-foreground',
      )} />
      <span className={cn(
        context.displayMode === 'compact'
          ? 'shrink-0 whitespace-nowrap pr-2'
          : 'min-w-0 flex-1 truncate',
      )}>{entry.displayName}</span>
    </div>
  );
  return (
    <ContextMenu dir="ltr" onOpenChange={context.onContextMenuOpenChange}>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent
        className="w-40 min-w-40 p-1"
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <WorkspaceDirectoryContextMenu canonicalPath={entry.canonicalPath} relativePath={entry.relativePath} onCopyFailed={context.onCopyFailed} onOpenInFileManager={context.onOpenInFileManager} />
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function WorkspaceFileTree({ projectId, selectedPath, onOpenFile }: WorkspaceFileTreeProps) {
  const { t } = useTranslation();
  const snapshot = useFileExplorerSnapshot(projectId);
  const displayModeToggle = fileTreeDisplayModeToggle(snapshot.displayMode);
  const treeNodes = useMemo(() => fileTreeView(snapshot.roots, snapshot.displayMode), [snapshot.displayMode, snapshot.roots]);
  const { ref, height } = useMeasuredElementHeight(320, measureTreeViewportHeight);
  const treeRef = useRef<TreeApi<FileTreeViewNode> | null>(null);
  const pendingRevealPathRef = useRef<string | null>(null);
  const restoringScrollRef = useRef(true);
  const syncingExpandedRef = useRef(false);
  const contextMenuOpenRef = useRef(false);
  const suppressContextMenuActivationRef = useRef(false);
  const contextMenuFrameRef = useRef<number | null>(null);
  const [actionFailure, setActionFailure] = useState<'copy' | 'file-manager' | null>(null);
  const actionFailureTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    void fileExplorerStore.loadRoot(projectId);
    restoringScrollRef.current = true;
    const savedScrollTop = fileExplorerStore.snapshot(projectId).treeScrollTop;
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
  }, [projectId, snapshot.displayMode]);

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
      fileExplorerStore.takeSelectionReveal(projectId, null);
      pendingRevealPathRef.current = null;
      return;
    }
    if (fileExplorerStore.takeSelectionReveal(projectId, selectedPath)) {
      pendingRevealPathRef.current = selectedPath;
    }
  }, [projectId, selectedPath]);

  useEffect(() => {
    const reveal = consumePendingTreeReveal(pendingRevealPathRef.current, treeNodes);
    if (!reveal.targetId) return;
    pendingRevealPathRef.current = reveal.pendingPath;
    const frame = requestAnimationFrame(() => treeRef.current?.scrollTo(reveal.targetId!));
    return () => cancelAnimationFrame(frame);
  }, [selectedPath, treeNodes]);

  const onCopyFailed = useCallback(() => {
    if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
    setActionFailure('copy');
    actionFailureTimerRef.current = setTimeout(() => setActionFailure(null), 1_500);
  }, []);
  const onOpenInFileManager = useCallback((relativePath: string) => {
    void openWorkspacePathInFileManager(projectId, relativePath).catch(() => {
      if (actionFailureTimerRef.current) clearTimeout(actionFailureTimerRef.current);
      setActionFailure('file-manager');
      actionFailureTimerRef.current = setTimeout(() => setActionFailure(null), 1_500);
    });
  }, [projectId]);
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
    onContextMenuOpenChange,
    canActivateFile,
    displayMode: snapshot.displayMode,
  }), [canActivateFile, onContextMenuOpenChange, onCopyFailed, onOpenFile, onOpenInFileManager, selectedPath, snapshot.displayMode]);
  const searchEntries = snapshot.searchResult?.entries ?? [];
  const searching = snapshot.searchQuery.trim().length > 0;

  return (
    <aside className="relative flex h-full min-h-0 flex-col bg-muted/10" aria-label={t('workspace.filesPanel.workspaceTree')}>
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border/50 p-2">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            variant="toolbar"
            value={snapshot.searchQuery}
            onChange={(event) => fileExplorerStore.setSearchQuery(projectId, event.target.value)}
            placeholder={t('workspace.filesPanel.filterPlaceholder')}
            className="h-8 pl-8 pr-8 text-xs"
          />
          {snapshot.searchQuery ? (
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="absolute right-1 top-1/2 -translate-y-1/2"
              onClick={() => fileExplorerStore.setSearchQuery(projectId, '')}
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
              onClick={() => fileExplorerStore.setDisplayMode(projectId, displayModeToggle.targetMode)}
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
      {actionFailure ? <div className="pointer-events-none absolute right-2 top-12 z-20 rounded-md border border-destructive/20 bg-popover/95 px-2 py-1 text-ui-caption text-destructive shadow-sm">{t(actionFailure === 'copy' ? 'workspace.filesPanel.pathCopyFailed' : 'workspace.filesPanel.fileManagerOpenFailed')}</div> : null}
      <div ref={ref} className="min-h-0 flex-1 overflow-hidden p-1.5">
        {snapshot.status === 'loading' || snapshot.searchStatus === 'loading' ? (
          <div className="flex items-center gap-2 px-2 py-3 text-xs text-muted-foreground"><LoaderCircle className="size-3.5 animate-spin" />{t('workspace.filesPanel.loading')}</div>
        ) : snapshot.status === 'error' || snapshot.searchStatus === 'error' ? (
          <div className="space-y-2 px-2 py-3 text-xs text-muted-foreground">
            <p>{t(`workspace.filesPanel.errors.${snapshot.errorCode}`, snapshot.errorCode ?? '')}</p>
            <Button size="sm" variant="outline" onClick={() => void fileExplorerStore.loadRoot(projectId, true)}>{t('workspace.filesPanel.retry')}</Button>
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
                        void fileExplorerStore.revealFile(projectId, entry.relativePath).then(() => onOpenFile(entry));
                      }}
                    >
                      <Icon className="mt-0.5 size-3.5 shrink-0" />
                      <span className="min-w-0"><span className="block truncate text-foreground">{entry.name}</span><span className="block truncate text-ui-micro">{entry.relativePath}</span></span>
                    </button>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-40 min-w-40 p-1" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}>
                    <WorkspaceDirectoryContextMenu canonicalPath={entry.canonicalPath} relativePath={entry.relativePath} onCopyFailed={onCopyFailed} onOpenInFileManager={onOpenInFileManager} />
                  </ContextMenuContent>
                </ContextMenu>
              );
            })}
            {snapshot.searchStatus === 'ready' && searchEntries.length === 0 ? <p className="px-2 py-3 text-xs text-muted-foreground">{t('workspace.filesPanel.noSearchResults')}</p> : null}
            {snapshot.searchResult?.truncated ? <p className="px-2 py-2 text-ui-caption text-muted-foreground">{t('workspace.filesPanel.searchTruncated')}</p> : null}
          </div>
        ) : (
          <TreeRowContext.Provider value={contextValue}>
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
                if (entry) void fileExplorerStore.toggleDirectory(projectId, entry.relativePath, !snapshot.expanded.has(entry.relativePath));
              }}
              className="gold-themed-scrollbar !overflow-x-auto overscroll-contain [overflow-anchor:none] [scrollbar-gutter:stable]"
              rowClassName="w-full px-0.5"
              onScroll={({ scrollOffset, scrollUpdateWasRequested }) => {
                if (!restoringScrollRef.current && !scrollUpdateWasRequested) {
                  fileExplorerStore.setTreeScrollTop(projectId, scrollOffset);
                }
              }}
              onActivate={(node) => {
                if (node.data.kind === 'file' && canActivateFile()) onOpenFile(node.data);
              }}
              aria-label={t('workspace.filesPanel.workspaceTree')}
            >
              {TreeNodeRow}
            </Tree>
          </TreeRowContext.Provider>
        )}
      </div>
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
