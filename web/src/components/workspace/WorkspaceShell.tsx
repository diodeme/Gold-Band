import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { useTranslation } from 'react-i18next';
import type { Layout, LayoutChangedMeta, PanelImperativeHandle, PanelSize } from 'react-resizable-panels';
import type { AppConfigVm, ConversationPage, ConversationSidebarVm, DesktopPlatform, DesktopWindowFrameStyle } from '../../types';
import { ConversationSidebar } from '../conversation/ConversationSidebar';
import { saveConversationPreference } from '../../api';
import { AppTitleBar } from '../AppTitleBar';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { TooltipProvider } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { RightWorkspaceDock } from './RightWorkspaceDock';
import {
  ConversationWorkspaceStore,
  createConversationWorkspaceScope,
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspace,
  type RightWorkspaceResource,
} from './right-workspace-context';
import { fileContentStore } from './files/file-content-store';
import { fileExplorerStore } from './files/file-explorer-store';
import { WorkspaceFileLinkProvider } from './files/WorkspaceFileLinkProvider';
import {
  reduceWorkspaceAutoCollapse,
  resolveRightWorkspacePanelMaxWidth,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  FALLBACK_WORKSPACE_FILES,
  shouldOpenRightWorkspaceSheet,
  shouldPersistRightWorkspaceWidth,
  syncRightWorkspacePanelPresentation,
  WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
  WORKSPACE_SIDEBAR_MAX_WIDTH,
  WORKSPACE_SIDEBAR_MIN_WIDTH,
  workspaceAutoCollapsePresentationChanged,
  workspaceLayoutProfileForPage,
  type WorkspaceAutoCollapseInput,
  type WorkspaceAutoCollapsePresentation,
  type WorkspaceAutoCollapseState,
} from './workspace-layout';

interface WorkspaceShellProps {
  appName: string;
  feedbackEnabled?: boolean;
  platform?: DesktopPlatform | null;
  windowFrameStyle: DesktopWindowFrameStyle;
  appConfig: AppConfigVm;
  vm: ConversationSidebarVm;
  active: ConversationPage;
  sidebarCollapsed: boolean;
  onSelect: (page: ConversationPage) => void;
  onToggleSidebar: () => void;
  onNewConversation: () => void;
  onSearch: () => void;
  onSelectTask: (projectId: string, taskId: string) => void;
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  onPauseRun?: (projectId: string, taskId: string, runId: string) => void | Promise<void>;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string) => void;
  // 必填：App 恒提供（Shell.onConversationNewInWorkspace），ConversationSidebar/MulticaRemoteTaskList
  // 均要求非空；此前误标可选导致与下游必填声明类型不一致。
  onNewConversationInWorkspace: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (projectId: string) => Promise<void>;
  activeWorkspaceId?: string | null;
  conversationTaskUuid?: string | null;
  conversationWorkspaceStore: ConversationWorkspaceStore;
  children: React.ReactNode;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function loadWidth(prefs: Record<string, unknown> | null | undefined, key: string, fallback: number, min: number, max: number) {
  const value = prefs?.[key];
  return typeof value === 'number' ? clamp(value, min, max) : fallback;
}

const LazyFileWorkspacePanel = lazy(() => import('./files/FileWorkspacePanel').then((module) => ({ default: module.FileWorkspacePanel })));
const LazyTurnFileWorkspacePanel = lazy(() => import('./files/TurnFileWorkspacePanel').then((module) => ({ default: module.TurnFileWorkspacePanel })));
const LazyConversationAssetWorkspacePanel = lazy(() => import('./files/ConversationAssetWorkspacePanel').then((module) => ({ default: module.ConversationAssetWorkspacePanel })));
const LazyConversationDirectoryWorkspacePanel = lazy(() => import('./ConversationDirectoryWorkspacePanel').then((module) => ({ default: module.ConversationDirectoryWorkspacePanel })));
const LazySourceControlWorkspacePanel = lazy(() => import('./source-control/SourceControlWorkspacePanel').then((module) => ({ default: module.SourceControlWorkspacePanel })));

function FileWorkspaceIntegration({
  config = FALLBACK_WORKSPACE_FILES,
  layout,
}: {
  config?: AppConfigVm['workspaceFiles'];
  layout: AppConfigVm['workspaceLayout']['rightWorkspace']['file'];
}) {
  const workspace = useRightWorkspace();
  useEffect(() => {
    fileContentStore.configure(config);
    fileExplorerStore.configure(config);
  }, [config]);
  useEffect(() => workspace.registerResourceRenderer('file-browser', (resource: RightWorkspaceResource) => (
    resource.kind === 'file-browser'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyFileWorkspacePanel resource={resource} layout={layout} /></Suspense>
      : null
  )), [layout, workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('file', (resource: RightWorkspaceResource) => (
    resource.kind === 'file'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyFileWorkspacePanel resource={resource} layout={layout} /></Suspense>
      : null
  )), [layout, workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('conversation-directory', (resource: RightWorkspaceResource) => (
    resource.kind === 'conversation-directory'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></Suspense>
      : null
  )), [layout, workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('file-diff', (resource: RightWorkspaceResource) => (
    resource.kind === 'file-diff'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyTurnFileWorkspacePanel resource={resource} /></Suspense>
      : null
  )), [workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('file-version', (resource: RightWorkspaceResource) => (
    resource.kind === 'file-version'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyTurnFileWorkspacePanel resource={resource} /></Suspense>
      : null
  )), [workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('conversation-asset', (resource: RightWorkspaceResource) => (
    resource.kind === 'conversation-asset'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyConversationAssetWorkspacePanel resource={resource} /></Suspense>
      : null
  )), [workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceRenderer('source-control', (resource: RightWorkspaceResource) => (
    resource.kind === 'source-control'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazySourceControlWorkspacePanel resource={resource} /></Suspense>
      : null
  )), [workspace.registerResourceRenderer]);
  useEffect(() => workspace.registerResourceCloseResolver('file', (resource, reason) => (
    resource.kind === 'file'
      ? (reason === 'close' ? fileContentStore.close(resource.key) : fileContentStore.flush(resource.key))
      : true
  )), [workspace.registerResourceCloseResolver]);
  return null;
}

export function WorkspaceShell(props: WorkspaceShellProps) {
  const rightWorkspaceLayout = props.appConfig.workspaceLayout.rightWorkspace;
  const initialRightWidth = loadWidth(
    props.vm.preferences,
    'rightWorkspace.width',
    rightWorkspaceLayout.defaultWidth,
    rightWorkspaceLayout.minWidth,
    rightWorkspaceLayout.maxWidth,
  );
  const rightWorkspaceScope = useMemo(() => {
    if (props.active.kind === 'conversation-home' || props.active.kind === 'scheduled-task-create') {
      return createDraftConversationWorkspaceScope(props.activeWorkspaceId ?? 'default');
    }
    if (props.active.kind === 'conversation-run') {
      return createConversationWorkspaceScope({
        projectId: props.active.projectId,
        taskId: props.active.taskId,
        taskUuid: props.conversationTaskUuid,
        runId: props.active.runId,
      });
    }
    return null;
  }, [props.active, props.activeWorkspaceId, props.conversationTaskUuid]);
  return (
    <TooltipProvider>
      <RightWorkspaceProvider initialWidth={initialRightWidth} scope={rightWorkspaceScope} store={props.conversationWorkspaceStore}>
        <WorkspaceFileLinkProvider>
          <WorkspaceShellLayout {...props} />
        </WorkspaceFileLinkProvider>
      </RightWorkspaceProvider>
    </TooltipProvider>
  );
}

function WorkspaceShellLayout({
  appName,
  feedbackEnabled,
  platform,
  windowFrameStyle,
  appConfig,
  vm,
  active,
  sidebarCollapsed,
  onSelect,
  onToggleSidebar,
  onNewConversation,
  onSearch,
  onSelectTask,
  onSelectRun,
  onPauseRun,
  onPinTask,
  onUnpinTask,
  onRenameTask,
  onDeleteTask,
  onNewConversationInWorkspace,
  onAddWorkspace,
  onRemoveWorkspace,
  activeWorkspaceId,
  children,
}: WorkspaceShellProps) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const shellRef = useRef<HTMLDivElement>(null);
  const compactSheetContentRef = useRef<HTMLDivElement>(null);
  const leftPanelRef = useRef<PanelImperativeHandle | null>(null);
  const rightPanelRef = useRef<PanelImperativeHandle | null>(null);
  const rightResizeIntentRef = useRef(false);
  const resizeFrameRef = useRef<number | null>(null);
  const rightPanelActualWidthRef = useRef(0);
  const rightPanelAtPreferredWidthRef = useRef(false);
  const handledOpenRevisionRef = useRef(workspace.openRevision);
  const handledWorkspaceScopeRef = useRef(workspace.scopeKey);
  const [compactSheetOpen, setCompactSheetOpen] = useState(false);
  const [rightPanelResizeActive, setRightPanelResizeActive] = useState(false);
  const [rightPanelAtPreferredWidth, setRightPanelAtPreferredWidth] = useState(false);
  const autoCollapseStateRef = useRef<WorkspaceAutoCollapseState>({
    previousWidth: 0,
    left: false,
    right: false,
  });
  const autoCollapseInputRef = useRef<Omit<WorkspaceAutoCollapseInput, 'availableWidth'>>({
    centerMinWidth: 0,
    centerAutoCollapseWidth: 0,
    sidebarWidth: 0,
    sidebarManuallyCollapsed: false,
    wantsRight: false,
  });
  const [autoCollapse, setAutoCollapse] = useState<WorkspaceAutoCollapsePresentation>({
    left: false,
    right: false,
  });
  const sidebarWidth = loadWidth(
    vm.preferences,
    'sidebar.width',
    WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
    WORKSPACE_SIDEBAR_MIN_WIDTH,
    WORKSPACE_SIDEBAR_MAX_WIDTH,
  );
  const profile = useMemo(
    () => workspaceLayoutProfileForPage(active, appConfig.workspaceLayout),
    [active, appConfig.workspaceLayout],
  );
  const rightWorkspaceAvailable = workspace.scopeKey !== null;
  const wantsRight = rightWorkspaceAvailable && workspace.requestedOpen;
  const activeRightResource = workspace.tabs.find((tab) => tab.key === workspace.activeTabKey) ?? null;
  const fileWorkspaceActive = activeRightResource?.kind === 'file' || activeRightResource?.kind === 'file-browser';
  autoCollapseInputRef.current = {
    centerMinWidth: profile.centerMinWidth,
    centerAutoCollapseWidth: profile.centerAutoCollapseWidth,
    sidebarWidth,
    sidebarManuallyCollapsed: sidebarCollapsed,
    wantsRight,
    rightMinWidth: appConfig.workspaceLayout.rightWorkspace.minWidth,
    rightWidthForStableLeftRestore: fileWorkspaceActive
      ? appConfig.workspaceLayout.rightWorkspace.file.splitMinWidth
      : appConfig.workspaceLayout.rightWorkspace.minWidth,
  };
  const showLeft = !sidebarCollapsed && !autoCollapse.left;
  const rightWorkspaceCompact = wantsRight && autoCollapse.right;
  const showRightDock = wantsRight && !rightWorkspaceCompact;
  const rightPanelMaxWidth = resolveRightWorkspacePanelMaxWidth({
    preferredWidth: workspace.width,
    minWidth: appConfig.workspaceLayout.rightWorkspace.minWidth,
    maxWidth: appConfig.workspaceLayout.rightWorkspace.maxWidth,
    userResizing: rightPanelResizeActive,
  });
  const rightPanelOwnsWindowResize = showRightDock && !rightPanelAtPreferredWidth;
  const trackRightPanelSize = useCallback((size: PanelSize) => {
    rightPanelActualWidthRef.current = size.inPixels;
    const next = size.inPixels >= rightPanelMaxWidth - 1;
    if (rightPanelAtPreferredWidthRef.current === next) return;
    rightPanelAtPreferredWidthRef.current = next;
    setRightPanelAtPreferredWidth(next);
  }, [rightPanelMaxWidth]);
  const beginRightPanelResize = useCallback(() => {
    rightResizeIntentRef.current = true;
    // react-resizable-panels reads maxSize as it starts the gesture. Commit the
    // expanded bound in this same event so a saved narrow width cannot cap it.
    flushSync(() => setRightPanelResizeActive(true));
  }, []);
  const endRightPanelResize = useCallback(() => {
    setRightPanelResizeActive(false);
  }, []);

  const evaluateAutoCollapse = useCallback((availableWidth: number) => {
    const current = autoCollapseStateRef.current;
    const next = reduceWorkspaceAutoCollapse(current, {
      ...autoCollapseInputRef.current,
      availableWidth: Math.round(availableWidth),
    });
    autoCollapseStateRef.current = next;
    if (workspaceAutoCollapsePresentationChanged(current, next)) {
      setAutoCollapse({ left: next.left, right: next.right });
    }
    return next;
  }, []);

  useEffect(() => {
    const element = shellRef.current;
    if (!element) return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? element.clientWidth;
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
      resizeFrameRef.current = requestAnimationFrame(() => {
        evaluateAutoCollapse(width);
        resizeFrameRef.current = null;
      });
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
    };
  }, [evaluateAutoCollapse]);

  useLayoutEffect(() => {
    const element = shellRef.current;
    if (element) evaluateAutoCollapse(element.clientWidth);
  }, [evaluateAutoCollapse, fileWorkspaceActive, profile.centerAutoCollapseWidth, profile.centerMinWidth, sidebarCollapsed, sidebarWidth, wantsRight]);

  useLayoutEffect(() => {
    const panel = leftPanelRef.current;
    if (!panel) return;
    try {
      if (showLeft) {
        if (panel.isCollapsed()) panel.expand();
      } else if (!panel.isCollapsed()) {
        panel.collapse();
      }
    } catch {
      // The panel group may be unmounting while the desktop surface changes.
    }
  }, [showLeft]);

  useLayoutEffect(() => {
    const panel = rightPanelRef.current;
    if (!panel) return;
    try {
      syncRightWorkspacePanelPresentation({
        panel,
        visible: showRightDock,
        preferredWidth: workspace.width,
      });
    } catch {
      // The panel group may be unmounting while the desktop surface changes.
    }
  }, [showRightDock, workspace.width]);

  useEffect(() => {
    if (handledWorkspaceScopeRef.current !== workspace.scopeKey) {
      handledWorkspaceScopeRef.current = workspace.scopeKey;
      handledOpenRevisionRef.current = workspace.openRevision;
      setCompactSheetOpen(false);
      return;
    }
    const previousOpenRevision = handledOpenRevisionRef.current;
    handledOpenRevisionRef.current = workspace.openRevision;
    if (shouldOpenRightWorkspaceSheet({
      compact: rightWorkspaceCompact,
      previousOpenRevision,
      openRevision: workspace.openRevision,
    })) {
      setCompactSheetOpen(true);
    }
  }, [rightWorkspaceCompact, workspace.openRevision, workspace.scopeKey]);

  useEffect(() => {
    if (!rightWorkspaceCompact || !wantsRight) setCompactSheetOpen(false);
  }, [rightWorkspaceCompact, wantsRight]);

  useEffect(() => {
    if (showRightDock) return;
    rightResizeIntentRef.current = false;
    rightPanelActualWidthRef.current = 0;
    rightPanelAtPreferredWidthRef.current = false;
    setRightPanelResizeActive(false);
    setRightPanelAtPreferredWidth(false);
  }, [showRightDock]);

  useLayoutEffect(() => {
    const actualWidth = rightPanelActualWidthRef.current;
    if (actualWidth <= 0) return;
    const next = actualWidth >= rightPanelMaxWidth - 1;
    if (rightPanelAtPreferredWidthRef.current === next) return;
    rightPanelAtPreferredWidthRef.current = next;
    setRightPanelAtPreferredWidth(next);
  }, [rightPanelMaxWidth]);

  const setRightWorkspaceWidth = workspace.setWidth;
  const saveWorkspaceLayout = useCallback((layout: Layout, meta: LayoutChangedMeta) => {
    if (!meta.isUserInteraction) return;
    const groupWidth = shellRef.current?.clientWidth ?? 0;
    const nextSidebarWidth = resolveWorkspacePanelWidthFromLayout({
      layout,
      panelId: 'workspace-navigation',
      groupWidth,
      minWidth: WORKSPACE_SIDEBAR_MIN_WIDTH,
      maxWidth: WORKSPACE_SIDEBAR_MAX_WIDTH,
    });
    if (nextSidebarWidth != null && nextSidebarWidth !== Math.round(sidebarWidth)) {
      void saveConversationPreference('sidebar.width', nextSidebarWidth);
    }
    if (shouldPersistRightWorkspaceWidth(meta.isUserInteraction, rightResizeIntentRef.current)) {
      rightResizeIntentRef.current = false;
      const nextRightWidth = resolveRightWorkspaceWidthFromLayout(
        layout,
        groupWidth,
        appConfig.workspaceLayout.rightWorkspace,
      );
      if (nextRightWidth != null && nextRightWidth !== Math.round(workspace.width)) {
        setRightWorkspaceWidth(nextRightWidth);
        void saveConversationPreference('rightWorkspace.width', nextRightWidth);
      }
    }
  }, [appConfig.workspaceLayout.rightWorkspace, setRightWorkspaceWidth, sidebarWidth, workspace.width]);

  const rightWorkspacePresented = showRightDock || (rightWorkspaceCompact && compactSheetOpen);
  const removeWorkspace = useCallback(async (projectId: string) => {
    if (!onRemoveWorkspace) return;
    const saved = await fileContentStore.flushAll(projectId);
    if (!saved) throw new Error('workspace-file.pending-save-failed');
    await onRemoveWorkspace(projectId);
    await fileContentStore.releaseProject(projectId);
    fileExplorerStore.clear(projectId);
  }, [onRemoveWorkspace]);
  const deleteTask = useCallback((projectId: string, taskId: string) => {
    void fileContentStore.flushAll(projectId).then((saved) => {
      if (saved) onDeleteTask(projectId, taskId);
    });
  }, [onDeleteTask]);
  const toggleRightWorkspace = useCallback(() => {
    if (rightWorkspacePresented) {
      setCompactSheetOpen(false);
      workspace.closeWorkspace();
      return;
    }
    workspace.openWorkspace();
  }, [rightWorkspacePresented, workspace.closeWorkspace, workspace.openWorkspace]);

  return (
    <div
      ref={shellRef}
      className="app-window-shell flex h-screen flex-col bg-gold-workspace text-foreground"
      data-window-frame-style={windowFrameStyle}
      onContextMenu={(event) => event.preventDefault()}
    >
      <FileWorkspaceIntegration
        config={appConfig.workspaceFiles}
        layout={appConfig.workspaceLayout.rightWorkspace.file}
      />
      <AppTitleBar
        appName={appName}
        feedbackEnabled={feedbackEnabled}
        platform={platform}
        sidebarCollapsed={sidebarCollapsed || autoCollapse.left}
        onToggleSidebar={onToggleSidebar}
        rightWorkspaceOpen={rightWorkspacePresented}
        onToggleRightWorkspace={rightWorkspaceAvailable ? toggleRightWorkspace : undefined}
      />
      <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1 bg-sidebar" onLayoutChanged={saveWorkspaceLayout}>
        <ResizablePanel
          panelRef={leftPanelRef}
          id="workspace-navigation"
          defaultSize={sidebarWidth}
          minSize={WORKSPACE_SIDEBAR_MIN_WIDTH}
          maxSize={WORKSPACE_SIDEBAR_MAX_WIDTH}
          collapsedSize={0}
          collapsible
          groupResizeBehavior="preserve-pixel-size"
          className={cn(!showLeft && 'pointer-events-none overflow-hidden')}
        >
          {showLeft ? (
            <ConversationSidebar
              vm={vm}
              active={active}
              activeWorkspaceId={activeWorkspaceId}
              onSelect={onSelect}
              onNewConversation={onNewConversation}
              onSearch={onSearch}
              onSelectTask={onSelectTask}
              onSelectRun={onSelectRun}
              onPauseRun={onPauseRun}
              onPinTask={onPinTask}
              onUnpinTask={onUnpinTask}
              onRenameTask={onRenameTask}
              onDeleteTask={deleteTask}
              onNewConversationInWorkspace={onNewConversationInWorkspace}
              onAddWorkspace={onAddWorkspace}
              onRemoveWorkspace={onRemoveWorkspace ? removeWorkspace : undefined}
            />
          ) : null}
        </ResizablePanel>
        <ResizableHandle
          className={cn('z-20 bg-transparent hover:bg-transparent', !showLeft && 'pointer-events-none opacity-0')}
          data-testid="workspace-left-resize-handle"
          disabled={!showLeft}
          aria-hidden={!showLeft}
        />
        <ResizablePanel
          id="workspace-center"
          minSize={profile.centerMinWidth}
          className="min-w-0"
          groupResizeBehavior={rightPanelOwnsWindowResize ? 'preserve-pixel-size' : 'preserve-relative-size'}
        >
          <main className={cn('relative flex h-full min-w-0 flex-col overflow-hidden border-t border-workspace-divider bg-gold-workspace', showLeft && 'rounded-tl-2xl border-l')}>
            {children}
          </main>
        </ResizablePanel>
        <ResizableHandle
          className={cn(
            'z-20 bg-workspace-divider hover:bg-primary/30',
            !showRightDock && 'pointer-events-none opacity-0',
          )}
          data-testid="workspace-right-resize-handle"
          disabled={!showRightDock}
          aria-hidden={!showRightDock}
          onPointerDown={beginRightPanelResize}
          onPointerUp={endRightPanelResize}
          onPointerCancel={endRightPanelResize}
          onLostPointerCapture={endRightPanelResize}
          onKeyDown={beginRightPanelResize}
          onKeyUp={endRightPanelResize}
          onBlur={endRightPanelResize}
        />
        <ResizablePanel
          panelRef={rightPanelRef}
          id="workspace-right"
          defaultSize={workspace.width}
          minSize={appConfig.workspaceLayout.rightWorkspace.minWidth}
          maxSize={rightPanelMaxWidth}
          collapsedSize={0}
          collapsible
          groupResizeBehavior={rightPanelOwnsWindowResize ? 'preserve-relative-size' : 'preserve-pixel-size'}
          onResize={trackRightPanelSize}
          className={cn(
            'border-t border-workspace-divider',
            !showRightDock && 'pointer-events-none overflow-hidden',
          )}
        >
          {showRightDock ? <RightWorkspaceDock /> : null}
        </ResizablePanel>
      </ResizablePanelGroup>
      <Sheet
        open={wantsRight && rightWorkspaceCompact && compactSheetOpen}
        onOpenChange={(open) => {
          setCompactSheetOpen(open);
          if (!open) workspace.closeWorkspace();
        }}
      >
        <SheetContent
          ref={compactSheetContentRef}
          side="right"
          tabIndex={-1}
          className="flex w-[min(92vw,44rem)] flex-col gap-0 p-0 focus:outline-none sm:max-w-none"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            compactSheetContentRef.current?.focus({ preventScroll: true });
          }}
        >
          <SheetTitle className="sr-only">{t('workspace.rightWorkspace')}</SheetTitle>
          {rightWorkspaceCompact ? (
            <div className="flex min-h-0 flex-1 flex-col" data-right-workspace-presentation="sheet">
              <RightWorkspaceDock />
            </div>
          ) : null}
        </SheetContent>
      </Sheet>
    </div>
  );
}
