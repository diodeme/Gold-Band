import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Layout, LayoutChangedMeta } from 'react-resizable-panels';
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
} from './right-workspace-context';
import {
  reduceWorkspaceAutoCollapse,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  RIGHT_WORKSPACE_DEFAULT_WIDTH,
  RIGHT_WORKSPACE_MAX_WIDTH,
  RIGHT_WORKSPACE_MIN_WIDTH,
  shouldOpenRightWorkspaceSheet,
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

export function WorkspaceShell(props: WorkspaceShellProps) {
  const initialRightWidth = loadWidth(
    props.vm.preferences,
    'rightWorkspace.width',
    RIGHT_WORKSPACE_DEFAULT_WIDTH,
    RIGHT_WORKSPACE_MIN_WIDTH,
    RIGHT_WORKSPACE_MAX_WIDTH,
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
        <WorkspaceShellLayout {...props} />
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
  const resizeFrameRef = useRef<number | null>(null);
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
  autoCollapseInputRef.current = {
    centerMinWidth: profile.centerMinWidth,
    centerAutoCollapseWidth: profile.centerAutoCollapseWidth,
    sidebarWidth,
    sidebarManuallyCollapsed: sidebarCollapsed,
    wantsRight,
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
    if (!workspaceAutoCollapsePresentationChanged(current, next)) return;
    setAutoCollapse({ left: next.left, right: next.right });
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
  }, [evaluateAutoCollapse, profile.centerAutoCollapseWidth, profile.centerMinWidth, sidebarCollapsed, sidebarWidth, wantsRight]);

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
    const nextRightWidth = resolveRightWorkspaceWidthFromLayout(layout, groupWidth);
    if (nextRightWidth != null && nextRightWidth !== Math.round(workspace.width)) {
      setRightWorkspaceWidth(nextRightWidth);
      void saveConversationPreference('rightWorkspace.width', nextRightWidth);
    }
  }, [setRightWorkspaceWidth, sidebarWidth, workspace.width]);

  const rightWorkspacePresented = showRightDock || (rightWorkspaceCompact && compactSheetOpen);
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
                onDeleteTask={onDeleteTask}
                onNewConversationInWorkspace={onNewConversationInWorkspace}
                onAddWorkspace={onAddWorkspace}
                onRemoveWorkspace={onRemoveWorkspace}
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
            <ResizableHandle className="z-20 bg-sidebar-border/70 hover:bg-primary/30" data-testid="workspace-right-resize-handle" />
            <ResizablePanel id="workspace-right" defaultSize={workspace.width} minSize={RIGHT_WORKSPACE_MIN_WIDTH} maxSize={RIGHT_WORKSPACE_MAX_WIDTH} groupResizeBehavior="preserve-pixel-size">
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
