import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GroupImperativeHandle, Layout, LayoutChangedMeta, PanelImperativeHandle } from 'react-resizable-panels';
import type { AppConfigVm, ConversationPage, ConversationSidebarVm, DesktopPlatform, DesktopWindowFrameStyle } from '../../types';
import { ConversationSidebar, type ConversationSidebarWorkspaceRevealRequest } from '../conversation/ConversationSidebar';
import { saveConversationPreference } from '../../api';
import { AppTitleBar } from '../AppTitleBar';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet';
import { TooltipProvider } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import {
  isWorkspaceLayoutDiagnosticsEnabled,
  installWorkspaceLayoutDiagnosticShortcut,
  recordWorkspaceLayoutDiagnostic,
  type WorkspaceLayoutDiagnosticStage,
} from '@/lib/workspace-layout-diagnostics';
import { useThemeWallpaperSurface } from '@/components/theme/ThemeAssetsContext';
import { RightWorkspaceDock } from './RightWorkspaceDock';
import {
  ConversationWorkspaceStore,
  conversationDirectoryWorkspaceDataKey,
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
  resolveWorkspaceCanonicalLayout,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  resolveWorkspaceUserResizeTarget,
  FALLBACK_WORKSPACE_FILES,
  resolveRightWorkspaceSheetOpenTransition,
  WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
  WORKSPACE_SIDEBAR_MAX_WIDTH,
  WORKSPACE_SIDEBAR_MIN_WIDTH,
  workspaceAutoCollapsePresentationChanged,
  workspaceCanonicalLayoutMissingPanel,
  workspaceCanonicalLayoutNeedsConvergence,
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
  onNewConversationInWorkspace?: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (projectId: string) => Promise<void>;
  activeWorkspaceId?: string | null;
  defaultExpandedWorkspaceId?: string | null;
  workspaceRevealRequest?: ConversationSidebarWorkspaceRevealRequest | null;
  conversationTaskUuid?: string | null;
  sourceControlWorkspacePath?: string | null;
  conversationWorkspaceStore: ConversationWorkspaceStore;
  children: React.ReactNode;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function loadWidth(prefs: Record<string, unknown> | null | undefined, key: string, fallback: number, min: number, max: number) {
  const value = prefs?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? clamp(value, min, max) : fallback;
}

function loadOptionalWidth(prefs: Record<string, unknown> | null | undefined, key: string, min: number, max: number) {
  const value = prefs?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? clamp(value, min, max) : null;
}

function panelDiagnosticSize(panel: PanelImperativeHandle | null) {
  if (!panel) return null;
  try {
    const size = panel.getSize();
    return {
      collapsed: panel.isCollapsed(),
      pixels: Math.round(size.inPixels),
      percentage: Math.round(size.asPercentage * 1_000) / 1_000,
    };
  } catch {
    return { unavailable: true };
  }
}

function workspaceDiagnosticEnvironment(shell: HTMLElement | null) {
  return {
    viewport: {
      width: typeof window === 'undefined' ? null : window.innerWidth,
      height: typeof window === 'undefined' ? null : window.innerHeight,
      devicePixelRatio: typeof window === 'undefined' ? null : window.devicePixelRatio,
    },
    shellWidth: shell?.clientWidth ?? 0,
  };
}

function workspacePanelGroupWidth(element: HTMLDivElement | null) {
  if (!element) return 0;
  const panelWidth = Array.from(element.children).reduce((total, child) => (
    child instanceof HTMLElement && child.hasAttribute('data-panel')
      ? total + child.offsetWidth
      : total
  ), 0);
  return panelWidth > 0 ? panelWidth : element.clientWidth;
}

const LazyFileWorkspacePanel = lazy(() => import('./files/FileWorkspacePanel').then((module) => ({ default: module.FileWorkspacePanel })));
const LazyTurnFileWorkspacePanel = lazy(() => import('./files/TurnFileWorkspacePanel').then((module) => ({ default: module.TurnFileWorkspacePanel })));
const LazyConversationAssetWorkspacePanel = lazy(() => import('./files/ConversationAssetWorkspacePanel').then((module) => ({ default: module.ConversationAssetWorkspacePanel })));
const LazyDraftAttachmentWorkspacePanel = lazy(() => import('./files/DraftAttachmentWorkspacePanel').then((module) => ({ default: module.DraftAttachmentWorkspacePanel })));
const LazyConversationDirectoryWorkspacePanel = lazy(() => import('./ConversationDirectoryWorkspacePanel').then((module) => ({ default: module.ConversationDirectoryWorkspacePanel })));
const LazySourceControlWorkspacePanel = lazy(() => import('./source-control/SourceControlWorkspacePanel').then((module) => ({ default: module.SourceControlWorkspacePanel })));
const workspaceLayoutDiagnosticsEnabled = isWorkspaceLayoutDiagnosticsEnabled();

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
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyConversationDirectoryWorkspacePanel key={conversationDirectoryWorkspaceDataKey(resource.locator)} resource={resource} layout={layout} /></Suspense>
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
  useEffect(() => workspace.registerResourceRenderer('draft-attachment', (resource: RightWorkspaceResource) => (
    resource.kind === 'draft-attachment'
      ? <Suspense fallback={<div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">…</div>}><LazyDraftAttachmentWorkspacePanel resource={resource} /></Suspense>
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
  useThemeWallpaperSurface();
  const rightWorkspaceLayout = props.appConfig.workspaceLayout.rightWorkspace;
  const initialRightWidth = loadWidth(
    props.vm.preferences,
    'rightWorkspace.width',
    rightWorkspaceLayout.defaultWidth,
    rightWorkspaceLayout.minWidth,
    rightWorkspaceLayout.maxWidth,
  );
  const activeConversation = props.active.kind === 'conversation-run' ? props.active : null;
  const rightWorkspaceScope = useMemo(() => {
    if (props.active.kind === 'conversation-home' || props.active.kind === 'scheduled-task-create') {
      return createDraftConversationWorkspaceScope(props.activeWorkspaceId ?? 'default');
    }
    if (activeConversation) {
      return createConversationWorkspaceScope({
        projectId: activeConversation.projectId,
        taskId: activeConversation.taskId,
        taskUuid: props.conversationTaskUuid,
        runId: activeConversation.runId,
      });
    }
    return null;
  }, [
    activeConversation?.projectId,
    activeConversation?.runId,
    activeConversation?.taskId,
    props.active.kind,
    props.activeWorkspaceId,
    props.conversationTaskUuid,
  ]);
  return (
    <TooltipProvider>
      <RightWorkspaceProvider
        initialWidth={initialRightWidth}
        scope={rightWorkspaceScope}
        sourceControlWorkspacePath={props.sourceControlWorkspacePath}
        store={props.conversationWorkspaceStore}
      >
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
  activeWorkspaceId: _activeWorkspaceId,
  defaultExpandedWorkspaceId,
  workspaceRevealRequest,
  sourceControlWorkspacePath,
  children,
}: WorkspaceShellProps) {
  const { t } = useTranslation();
  const workspace = useRightWorkspace();
  const storedSidebarWidth = loadOptionalWidth(
    vm.preferences,
    'sidebar.width',
    WORKSPACE_SIDEBAR_MIN_WIDTH,
    WORKSPACE_SIDEBAR_MAX_WIDTH,
  );
  const shellRef = useRef<HTMLDivElement>(null);
  const panelGroupElementRef = useRef<HTMLDivElement>(null);
  const panelGroupRef = useRef<GroupImperativeHandle | null>(null);
  const compactSheetContentRef = useRef<HTMLDivElement>(null);
  const leftPanelRef = useRef<PanelImperativeHandle | null>(null);
  const centerPanelRef = useRef<PanelImperativeHandle | null>(null);
  const rightPanelRef = useRef<PanelImperativeHandle | null>(null);
  const leftSeparatorRef = useRef<HTMLDivElement>(null);
  const rightSeparatorRef = useRef<HTMLDivElement>(null);
  const lastCommittedLayoutRef = useRef<Layout | null>(null);
  const sidebarWidthTouchedRef = useRef(false);
  const resizeFrameRef = useRef<number | null>(null);
  const diagnosticContextRef = useRef<Record<string, unknown>>({});
  const handledOpenRevisionRef = useRef(workspace.openRevision);
  const handledWorkspaceScopeRef = useRef(workspace.scopeKey);
  const [compactSheetOpen, setCompactSheetOpen] = useState(false);
  const autoCollapseStateRef = useRef<WorkspaceAutoCollapseState>({
    previousWidth: 0,
    left: false,
    right: false,
    rightOwnsWindowResize: false,
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
    rightOwnsWindowResize: false,
  });
  const [sidebarWidth, setSidebarWidth] = useState(() => storedSidebarWidth ?? WORKSPACE_SIDEBAR_DEFAULT_WIDTH);
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
    rightPreferredWidth: workspace.width,
    rightWidthForStableLeftRestore: fileWorkspaceActive
      ? appConfig.workspaceLayout.rightWorkspace.file.splitMinWidth
      : appConfig.workspaceLayout.rightWorkspace.minWidth,
  };
  const showLeft = !sidebarCollapsed && !autoCollapse.left;
  const rightWorkspaceCompact = wantsRight && autoCollapse.right;
  const showRightDock = wantsRight && !rightWorkspaceCompact;
  if (workspaceLayoutDiagnosticsEnabled) {
    diagnosticContextRef.current = {
      page: active.kind,
      sidebar: {
        manuallyCollapsed: sidebarCollapsed,
        preferredWidth: sidebarWidth,
        storedWidth: storedSidebarWidth,
        autoCollapsed: autoCollapse.left,
        visible: showLeft,
      },
      rightWorkspace: {
        available: rightWorkspaceAvailable,
        requestedOpen: workspace.requestedOpen,
        preferredWidth: workspace.width,
        autoCollapsed: autoCollapse.right,
        compact: rightWorkspaceCompact,
        dockVisible: showRightDock,
        tabCount: workspace.tabs.length,
      },
      profile: {
        centerMinWidth: profile.centerMinWidth,
        centerAutoCollapseWidth: profile.centerAutoCollapseWidth,
        fileWorkspaceActive,
        rightOwnsWindowResize: autoCollapse.rightOwnsWindowResize,
      },
    };
  }
  const recordPanelSnapshot = useCallback((
    stage: WorkspaceLayoutDiagnosticStage,
    details: Record<string, unknown> = {},
  ) => {
    if (!workspaceLayoutDiagnosticsEnabled) return;
    recordWorkspaceLayoutDiagnostic(stage, () => ({
      ...diagnosticContextRef.current,
      ...workspaceDiagnosticEnvironment(shellRef.current),
      panels: {
        left: panelDiagnosticSize(leftPanelRef.current),
        center: panelDiagnosticSize(centerPanelRef.current),
        right: panelDiagnosticSize(rightPanelRef.current),
      },
      ...details,
    }));
  }, []);
  useEffect(() => {
    if (!workspaceLayoutDiagnosticsEnabled) return;
    return installWorkspaceLayoutDiagnosticShortcut();
  }, []);

  useLayoutEffect(() => {
    if (sidebarWidthTouchedRef.current || storedSidebarWidth == null) return;
    setSidebarWidth(storedSidebarWidth);
  }, [storedSidebarWidth]);

  const evaluateAutoCollapse = useCallback((availableWidth: number) => {
    const current = autoCollapseStateRef.current;
    const input = {
      ...autoCollapseInputRef.current,
      availableWidth: Math.round(availableWidth),
    };
    const next = reduceWorkspaceAutoCollapse(current, {
      ...input,
    });
    if (workspaceLayoutDiagnosticsEnabled) {
      recordWorkspaceLayoutDiagnostic('auto-collapse-evaluated', () => ({
        ...diagnosticContextRef.current,
        ...workspaceDiagnosticEnvironment(shellRef.current),
        input,
        previous: current,
        next,
        presentationChanged: workspaceAutoCollapsePresentationChanged(current, next),
      }));
    }
    autoCollapseStateRef.current = next;
    if (workspaceAutoCollapsePresentationChanged(current, next)) {
      setAutoCollapse({
        left: next.left,
        right: next.right,
        rightOwnsWindowResize: next.rightOwnsWindowResize,
      });
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

  useEffect(() => {
    const applyCanonicalLayout = (attempt: 'initial' | 'constraint-settled') => {
      const group = panelGroupRef.current;
      const groupWidth = workspacePanelGroupWidth(panelGroupElementRef.current);
      if (!group || groupWidth <= 0) return null;
      const target = resolveWorkspaceCanonicalLayout({
        groupWidth,
        centerMinWidth: profile.centerMinWidth,
        leftVisible: showLeft,
        leftWidth: sidebarWidth,
        rightVisible: showRightDock,
        rightPreferredWidth: workspace.width,
      });
      if (!target) return null;
      const before = workspaceLayoutDiagnosticsEnabled ? group.getLayout() : null;
      let firstApplied: Layout | null = null;
      let applied: Layout | null = null;
      const expandedPanels: string[] = [];
      try {
        firstApplied = group.setLayout(target);
        applied = firstApplied;
        const expansionCandidates = [
          ['workspace-right', rightPanelRef.current],
          ['workspace-navigation', leftPanelRef.current],
        ] as const;
        for (const [panelId, panel] of expansionCandidates) {
          if (!panel || !workspaceCanonicalLayoutMissingPanel(target, applied, panelId)) continue;
          if (!panel.isCollapsed()) continue;
          panel.expand();
          expandedPanels.push(panelId);
        }
        if (expandedPanels.length > 0) applied = group.setLayout(target);
      } catch {
        // The panel group may be unmounting while the desktop surface changes.
      }
      if (workspaceLayoutDiagnosticsEnabled) {
        recordPanelSnapshot('group-layout-sync', {
          attempt,
          groupWidth,
          target,
          before,
          firstApplied,
          expandedPanels,
          applied,
        });
      }
      return applied == null ? null : { applied, groupWidth, target };
    };

    const initial = applyCanonicalLayout('initial');
    if (
      initial == null
      || !workspaceCanonicalLayoutNeedsConvergence(initial.target, initial.applied, initial.groupWidth)
    ) return;

    const convergenceFrame = requestAnimationFrame(() => {
      applyCanonicalLayout('constraint-settled');
    });
    return () => cancelAnimationFrame(convergenceFrame);
  }, [
    autoCollapse.rightOwnsWindowResize,
    profile.centerMinWidth,
    recordPanelSnapshot,
    showLeft,
    showRightDock,
    sidebarWidth,
    workspace.width,
  ]);

  useEffect(() => {
    if (!workspaceLayoutDiagnosticsEnabled) return;
    const frame = requestAnimationFrame(() => recordPanelSnapshot('presentation-committed'));
    return () => cancelAnimationFrame(frame);
  }, [active.kind, autoCollapse.left, autoCollapse.right, recordPanelSnapshot, showLeft, showRightDock, sidebarWidth, workspace.requestedOpen, workspace.scopeKey, workspace.width]);

  useEffect(() => {
    if (handledWorkspaceScopeRef.current !== workspace.scopeKey) {
      handledWorkspaceScopeRef.current = workspace.scopeKey;
      handledOpenRevisionRef.current = workspace.openRevision;
      setCompactSheetOpen(false);
      return;
    }
    const transition = resolveRightWorkspaceSheetOpenTransition({
      compact: rightWorkspaceCompact,
      previousOpenRevision: handledOpenRevisionRef.current,
      openRevision: workspace.openRevision,
    });
    handledOpenRevisionRef.current = transition.handledOpenRevision;
    if (transition.openSheet) {
      setCompactSheetOpen(true);
    }
  }, [rightWorkspaceCompact, workspace.openRevision, workspace.scopeKey]);

  useEffect(() => {
    if (!rightWorkspaceCompact || !wantsRight) setCompactSheetOpen(false);
  }, [rightWorkspaceCompact, wantsRight]);

  const setRightWorkspaceWidth = workspace.setWidth;
  const trackWorkspaceLayout = useCallback((layout: Layout) => {
    if (!workspaceLayoutDiagnosticsEnabled) return;
    const groupWidth = workspacePanelGroupWidth(panelGroupElementRef.current);
    recordWorkspaceLayoutDiagnostic('group-layout-change', () => ({
      ...diagnosticContextRef.current,
      ...workspaceDiagnosticEnvironment(shellRef.current),
      groupWidth,
      layout: { ...layout },
      pixels: Object.fromEntries(Object.entries(layout).map(([id, percentage]) => [
        id,
        Math.round(groupWidth * percentage / 100),
      ])),
    }));
  }, []);
  const saveWorkspaceLayout = useCallback((layout: Layout, meta: LayoutChangedMeta) => {
    const previousLayout = lastCommittedLayoutRef.current;
    lastCommittedLayoutRef.current = { ...layout };
    const activeElement = typeof document === 'undefined' ? null : document.activeElement;
    const focusedResizeTarget = activeElement === leftSeparatorRef.current
      ? 'left'
      : activeElement === rightSeparatorRef.current
        ? 'right'
        : null;
    const resizeTarget = resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout,
      isUserInteraction: meta.isUserInteraction,
      focusedTarget: focusedResizeTarget,
    });
    if (workspaceLayoutDiagnosticsEnabled) {
      recordWorkspaceLayoutDiagnostic('group-layout-changed', () => ({
        ...diagnosticContextRef.current,
        ...workspaceDiagnosticEnvironment(shellRef.current),
        groupWidth: workspacePanelGroupWidth(panelGroupElementRef.current),
        layout: { ...layout },
        previousLayout,
        isUserInteraction: meta.isUserInteraction,
        focusedResizeTarget,
        resizeTarget,
      }));
    }
    if (resizeTarget == null) return;
    const groupWidth = workspacePanelGroupWidth(panelGroupElementRef.current);
    if (resizeTarget === 'left') {
      sidebarWidthTouchedRef.current = true;
      const nextSidebarWidth = resolveWorkspacePanelWidthFromLayout({
        layout,
        panelId: 'workspace-navigation',
        groupWidth,
        minWidth: WORKSPACE_SIDEBAR_MIN_WIDTH,
        maxWidth: WORKSPACE_SIDEBAR_MAX_WIDTH,
      });
      if (nextSidebarWidth != null && nextSidebarWidth !== Math.round(sidebarWidth)) {
        setSidebarWidth(nextSidebarWidth);
        void saveConversationPreference('sidebar.width', nextSidebarWidth);
      }
    }
    if (resizeTarget === 'right') {
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
      data-theme-role="shell"
      data-theme-wallpaper-slot="app"
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
      <ResizablePanelGroup
        elementRef={panelGroupElementRef}
        groupRef={panelGroupRef}
        orientation="horizontal"
        className="min-h-0 flex-1 bg-sidebar !overflow-x-clip !overflow-y-visible"
        onLayoutChange={workspaceLayoutDiagnosticsEnabled ? trackWorkspaceLayout : undefined}
        onLayoutChanged={saveWorkspaceLayout}
      >
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
              defaultExpandedWorkspaceId={defaultExpandedWorkspaceId}
              workspaceRevealRequest={workspaceRevealRequest}
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
          elementRef={leftSeparatorRef}
          id="workspace-left-resize-handle"
          className={cn('z-20 bg-transparent hover:bg-transparent', !showLeft && 'pointer-events-none opacity-0')}
          disabled={!showLeft}
          aria-hidden={!showLeft}
        />
        <ResizablePanel
          panelRef={workspaceLayoutDiagnosticsEnabled ? centerPanelRef : undefined}
          id="workspace-center"
          minSize={profile.centerMinWidth}
          className={cn(
            'relative z-10 min-w-0 [box-shadow:var(--workspace-main-surface-shadow)]',
            showLeft && 'rounded-tl-2xl',
          )}
          groupResizeBehavior={autoCollapse.rightOwnsWindowResize ? 'preserve-pixel-size' : 'preserve-relative-size'}
        >
          <main data-theme-wallpaper-slot="workspace" className={cn('relative flex h-full min-w-0 flex-col overflow-hidden border-t border-sidebar-border bg-gold-workspace', showLeft && 'rounded-tl-2xl border-l')}>
            {children}
          </main>
        </ResizablePanel>
        <ResizableHandle
          elementRef={rightSeparatorRef}
          id="workspace-right-resize-handle"
          className={cn(
            'z-20 bg-workspace-divider hover:bg-primary/30',
            !showRightDock && 'pointer-events-none opacity-0',
          )}
          disabled={!showRightDock}
          aria-hidden={!showRightDock}
        />
        <ResizablePanel
          panelRef={rightPanelRef}
          id="workspace-right"
          defaultSize={workspace.width}
          minSize={appConfig.workspaceLayout.rightWorkspace.minWidth}
          maxSize={appConfig.workspaceLayout.rightWorkspace.maxWidth}
          collapsedSize={0}
          collapsible
          groupResizeBehavior={autoCollapse.rightOwnsWindowResize ? 'preserve-relative-size' : 'preserve-pixel-size'}
          className={cn(
            'relative z-10 border-t border-workspace-divider [box-shadow:var(--workspace-main-surface-shadow)]',
            !showRightDock && 'pointer-events-none overflow-hidden',
          )}
        >
          {showRightDock ? <RightWorkspaceDock sourceControlWorkspacePath={sourceControlWorkspacePath} /> : null}
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
              <RightWorkspaceDock sourceControlWorkspacePath={sourceControlWorkspacePath} />
            </div>
          ) : null}
        </SheetContent>
      </Sheet>
    </div>
  );
}
