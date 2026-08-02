import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Layout, LayoutChangedMeta } from 'react-resizable-panels';
import type { ConversationPage, ConversationSidebarVm, DesktopPlatform, DesktopWindowFrameStyle } from '../../types';
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

interface WorkspaceShellProps {
  appName: string;
  feedbackEnabled?: boolean;
  platform?: DesktopPlatform | null;
  windowFrameStyle: DesktopWindowFrameStyle;
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

export interface WorkspaceLayoutProfile {
  centerMinWidth: number;
}

export const WORKSPACE_LAYOUT_PROFILES = {
  conversation: { centerMinWidth: 360 },
  contextCards: { centerMinWidth: 520 },
  workflowCanvas: { centerMinWidth: 640 },
  settings: { centerMinWidth: 480 },
} satisfies Record<string, WorkspaceLayoutProfile>;

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 420;
const SIDEBAR_DEFAULT = 256;
const RIGHT_WORKSPACE_MIN = 320;
const RIGHT_WORKSPACE_MAX = 720;
const RIGHT_WORKSPACE_DEFAULT = 440;
const LAYOUT_HYSTERESIS = 48;

export interface WorkspaceAutoCollapseState {
  previousWidth: number;
  left: boolean;
  right: boolean;
}

export interface WorkspaceAutoCollapseInput {
  availableWidth: number;
  centerMinWidth: number;
  sidebarManuallyCollapsed: boolean;
  wantsRight: boolean;
}

export function reduceWorkspaceAutoCollapse(
  state: WorkspaceAutoCollapseState,
  input: WorkspaceAutoCollapseInput,
): WorkspaceAutoCollapseState {
  const { availableWidth, centerMinWidth, sidebarManuallyCollapsed, wantsRight } = input;
  if (availableWidth <= 0) return state;
  const shrinking = state.previousWidth === 0 || availableWidth < state.previousWidth;
  const needsAll = centerMinWidth + SIDEBAR_MIN + (wantsRight ? RIGHT_WORKSPACE_MIN : 0);
  const needsCenterAndRight = centerMinWidth + (wantsRight ? RIGHT_WORKSPACE_MIN : 0);
  let left = state.left;
  let right = wantsRight ? state.right : false;
  if (!sidebarManuallyCollapsed && !left && availableWidth < needsAll) left = true;
  if (wantsRight && (sidebarManuallyCollapsed || left) && !right && availableWidth < needsCenterAndRight) {
    right = true;
  } else if (!shrinking) {
    // A maximize can cross both thresholds in one ResizeObserver delivery.
    // Restore in the designed order without requiring a second resize event.
    if (right && availableWidth > needsCenterAndRight + LAYOUT_HYSTERESIS) {
      right = false;
    }
    if (left && availableWidth > needsAll + LAYOUT_HYSTERESIS) {
      left = false;
    }
  }
  if (state.previousWidth === availableWidth && state.left === left && state.right === right) return state;
  return { previousWidth: availableWidth, left, right };
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function resolveRightWorkspaceMaxWidth({
  availableWidth,
  centerMinWidth,
  leftVisible,
}: {
  availableWidth: number;
  centerMinWidth: number;
  leftVisible: boolean;
}) {
  if (availableWidth <= 0) return RIGHT_WORKSPACE_MAX;
  const reservedWidth = centerMinWidth + (leftVisible ? SIDEBAR_MIN : 0);
  return clamp(availableWidth - reservedWidth, RIGHT_WORKSPACE_MIN, RIGHT_WORKSPACE_MAX);
}

export function resolveRightWorkspaceWidthFromLayout(layout: Layout, groupWidth: number) {
  const rightPercentage = layout['workspace-right'];
  if (rightPercentage == null || groupWidth <= 0) return null;
  return clamp(
    Math.round(groupWidth * rightPercentage / 100),
    RIGHT_WORKSPACE_MIN,
    RIGHT_WORKSPACE_MAX,
  );
}

export function shouldOpenRightWorkspaceSheet({
  compact,
  previousOpenRevision,
  openRevision,
}: {
  compact: boolean;
  previousOpenRevision: number;
  openRevision: number;
}) {
  return compact && openRevision > previousOpenRevision;
}

function loadWidth(prefs: Record<string, unknown> | null | undefined, key: string, fallback: number, min: number, max: number) {
  const value = prefs?.[key];
  return typeof value === 'number' ? clamp(value, min, max) : fallback;
}

function profileForPage(page: ConversationPage): WorkspaceLayoutProfile {
  if (page.kind === 'conversation-home' || page.kind === 'conversation-run') return WORKSPACE_LAYOUT_PROFILES.conversation;
  if (page.kind === 'contexts') return WORKSPACE_LAYOUT_PROFILES.contextCards;
  if (page.kind === 'settings') return WORKSPACE_LAYOUT_PROFILES.settings;
  return WORKSPACE_LAYOUT_PROFILES.workflowCanvas;
}

export function WorkspaceShell(props: WorkspaceShellProps) {
  const initialRightWidth = loadWidth(props.vm.preferences, 'rightWorkspace.width', RIGHT_WORKSPACE_DEFAULT, RIGHT_WORKSPACE_MIN, RIGHT_WORKSPACE_MAX);
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
  const resizeFrameRef = useRef<number | null>(null);
  const sidebarWidthSaveTimerRef = useRef<number | null>(null);
  const handledOpenRevisionRef = useRef(workspace.openRevision);
  const handledWorkspaceScopeRef = useRef(workspace.scopeKey);
  const [availableWidth, setAvailableWidth] = useState(0);
  const [compactSheetOpen, setCompactSheetOpen] = useState(false);
  const [autoCollapse, setAutoCollapse] = useState<WorkspaceAutoCollapseState>({
    previousWidth: 0,
    left: false,
    right: false,
  });
  const sidebarWidth = loadWidth(vm.preferences, 'sidebar.width', SIDEBAR_DEFAULT, SIDEBAR_MIN, SIDEBAR_MAX);
  const profile = useMemo(() => profileForPage(active), [active]);
  const rightWorkspaceAvailable = workspace.scopeKey !== null;
  const wantsRight = rightWorkspaceAvailable && workspace.requestedOpen;
  const showLeft = !sidebarCollapsed && !autoCollapse.left;
  const rightWorkspaceCompact = wantsRight && (
    autoCollapse.right
    || (availableWidth > 0 && availableWidth < profile.centerMinWidth + RIGHT_WORKSPACE_MIN)
  );
  const showRightDock = wantsRight && !rightWorkspaceCompact;
  const rightWorkspaceMaxWidth = resolveRightWorkspaceMaxWidth({
    availableWidth,
    centerMinWidth: profile.centerMinWidth,
    leftVisible: showLeft,
  });

  useEffect(() => {
    const element = shellRef.current;
    if (!element) return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? element.clientWidth;
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
      resizeFrameRef.current = requestAnimationFrame(() => {
        setAvailableWidth(Math.round(width));
        resizeFrameRef.current = null;
      });
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
    };
  }, []);

  useEffect(() => () => {
    if (sidebarWidthSaveTimerRef.current !== null) window.clearTimeout(sidebarWidthSaveTimerRef.current);
  }, []);

  useEffect(() => {
    setAutoCollapse((current) => reduceWorkspaceAutoCollapse(current, {
      availableWidth,
      centerMinWidth: profile.centerMinWidth,
      sidebarManuallyCollapsed: sidebarCollapsed,
      wantsRight,
    }));
  }, [availableWidth, profile.centerMinWidth, sidebarCollapsed, wantsRight]);

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

  const saveSidebarWidth = useCallback((size: { inPixels: number }) => {
    if (size.inPixels < SIDEBAR_MIN) return;
    if (sidebarWidthSaveTimerRef.current !== null) window.clearTimeout(sidebarWidthSaveTimerRef.current);
    const width = Math.round(size.inPixels);
    sidebarWidthSaveTimerRef.current = window.setTimeout(() => {
      sidebarWidthSaveTimerRef.current = null;
      void saveConversationPreference('sidebar.width', width);
    }, 160);
  }, []);
  const setRightWorkspaceWidth = workspace.setWidth;
  const saveWorkspaceLayout = useCallback((layout: Layout, meta: LayoutChangedMeta) => {
    if (!meta.isUserInteraction) return;
    const width = resolveRightWorkspaceWidthFromLayout(layout, shellRef.current?.clientWidth ?? 0);
    if (width == null) return;
    setRightWorkspaceWidth(width);
    void saveConversationPreference('rightWorkspace.width', width);
  }, [setRightWorkspaceWidth]);

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
            <ResizablePanel id="workspace-navigation" defaultSize={sidebarWidth} minSize={SIDEBAR_MIN} maxSize={SIDEBAR_MAX} groupResizeBehavior="preserve-pixel-size" onResize={saveSidebarWidth}>
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
            <ResizablePanel id="workspace-right" defaultSize={workspace.width} minSize={RIGHT_WORKSPACE_MIN} maxSize={rightWorkspaceMaxWidth} groupResizeBehavior="preserve-pixel-size">
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
        <SheetContent side="right" className="flex w-[min(92vw,44rem)] flex-col gap-0 p-0 sm:max-w-none">
          <SheetTitle className="sr-only">{t('workspace.rightWorkspace')}</SheetTitle>
          <div className="flex min-h-0 flex-1 flex-col" data-right-workspace-presentation="sheet">
            <RightWorkspaceDock />
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
}
