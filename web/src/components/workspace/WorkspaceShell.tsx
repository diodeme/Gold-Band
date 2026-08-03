import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Layout, LayoutChangedMeta, PanelImperativeHandle } from 'react-resizable-panels';
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
  resolveRightWorkspaceRestoreWidth,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  FALLBACK_WORKSPACE_FILES,
  shouldOpenRightWorkspaceSheet,
  shouldPersistRightWorkspaceWidth,
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
  stoppingRun?: boolean;
  onPauseRun?: (projectId: string, taskId: string, runId: string) => void | Promise<void>;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onDeleteTask: (projectId: string, taskId: string) => void;
  onNewConversationInWorkspace?: (projectId: string) => void;
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
    if (props.active.kind === 'conversation-home') {
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
  stoppingRun = false,
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
  const rightPanelRef = useRef<PanelImperativeHandle | null>(null);
  const rightResizeIntentRef = useRef(false);
  const resizeFrameRef = useRef<number | null>(null);
  const previousShellWidthRef = useRef(0);
  const handledOpenRevisionRef = useRef(workspace.openRevision);
  const handledWorkspaceScopeRef = useRef(workspace.scopeKey);
  const [compactSheetOpen, setCompactSheetOpen] = useState(false);
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
        const next = evaluateAutoCollapse(width);
        const panel = rightPanelRef.current;
        if (
          panel
          && width > previousShellWidthRef.current
          && autoCollapseInputRef.current.wantsRight
          && !next.right
        ) {
          const targetWidth = resolveRightWorkspaceRestoreWidth({
            shellWidth: width,
            preferredWidth: workspace.width,
            actualWidth: panel.getSize().inPixels,
            centerMinWidth: autoCollapseInputRef.current.centerMinWidth,
            sidebarWidth: autoCollapseInputRef.current.sidebarWidth,
            showLeft: !autoCollapseInputRef.current.sidebarManuallyCollapsed && !next.left,
            rightMinWidth: appConfig.workspaceLayout.rightWorkspace.minWidth,
          });
          if (targetWidth !== null) panel.resize(targetWidth);
        }
        previousShellWidthRef.current = width;
        resizeFrameRef.current = null;
      });
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
    };
  }, [appConfig.workspaceLayout.rightWorkspace.minWidth, evaluateAutoCollapse, workspace.width]);

  useLayoutEffect(() => {
    const element = shellRef.current;
    if (element) evaluateAutoCollapse(element.clientWidth);
  }, [evaluateAutoCollapse, fileWorkspaceActive, profile.centerAutoCollapseWidth, profile.centerMinWidth, sidebarCollapsed, sidebarWidth, wantsRight]);

  useEffect(() => {
    if (!showRightDock) return;
    const frame = requestAnimationFrame(() => {
      try {
        const panel = rightPanelRef.current;
        const shellWidth = shellRef.current?.clientWidth ?? 0;
        if (!panel || shellWidth <= 0) return;
        const targetWidth = resolveRightWorkspaceRestoreWidth({
          shellWidth,
          preferredWidth: workspace.width,
          actualWidth: panel.getSize().inPixels,
          centerMinWidth: profile.centerMinWidth,
          sidebarWidth,
          showLeft,
          rightMinWidth: appConfig.workspaceLayout.rightWorkspace.minWidth,
        });
        if (targetWidth !== null) panel.resize(targetWidth);
      } catch {
        // The panel may have been replaced by the compact Sheet before the frame ran.
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [appConfig.workspaceLayout.rightWorkspace.minWidth, profile.centerMinWidth, showLeft, showRightDock, sidebarWidth, workspace.width]);

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
        {showLeft ? (
          <>
            <ResizablePanel id="workspace-navigation" defaultSize={sidebarWidth} minSize={WORKSPACE_SIDEBAR_MIN_WIDTH} maxSize={WORKSPACE_SIDEBAR_MAX_WIDTH} groupResizeBehavior="preserve-pixel-size">
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
            </ResizablePanel>
            <ResizableHandle className="z-20 bg-transparent hover:bg-transparent" data-testid="workspace-left-resize-handle" />
          </>
        ) : null}
        <ResizablePanel id="workspace-center" minSize={profile.centerMinWidth} className="min-w-0">
          <main className={cn('relative flex h-full min-w-0 flex-col overflow-hidden border-t border-sidebar-border/70 bg-gold-workspace', showLeft && 'rounded-tl-2xl border-l')}>
            {children}
            {stoppingRun ? (
              <div className="absolute inset-0 z-40 flex items-center justify-center bg-background/55 backdrop-blur-sm">
                <div className="flex items-center gap-3 rounded-full border border-border/60 bg-popover/95 px-4 py-2 text-sm font-medium text-popover-foreground shadow-lg">
                  <span className="size-3.5 animate-spin rounded-full border-2 border-primary/25 border-t-primary" aria-hidden="true" />
                  <span>{t('conversation.runtime.stoppingRunOverlay')}</span>
                </div>
              </div>
            ) : null}
          </main>
        </ResizablePanel>
        {showRightDock ? (
          <>
            <ResizableHandle
              className="z-20 bg-sidebar-border/70 hover:bg-primary/30"
              data-testid="workspace-right-resize-handle"
              onPointerDown={() => { rightResizeIntentRef.current = true; }}
              onKeyDown={() => { rightResizeIntentRef.current = true; }}
            />
            <ResizablePanel
              panelRef={rightPanelRef}
              id="workspace-right"
              defaultSize={workspace.width}
              minSize={appConfig.workspaceLayout.rightWorkspace.minWidth}
              maxSize={appConfig.workspaceLayout.rightWorkspace.maxWidth}
              groupResizeBehavior="preserve-pixel-size"
            >
              <RightWorkspaceDock />
            </ResizablePanel>
          </>
        ) : null}
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
