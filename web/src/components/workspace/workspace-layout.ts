import type { Layout } from 'react-resizable-panels';
import type {
  ConversationPage,
  DesktopUiMode,
  PrimaryModule,
  WorkspaceLayoutProfileVm,
  WorkspaceLayoutVm,
  WorkspaceFilesVm,
} from '../../types';

/** Used only before desktop bootstrap is available (for example in browser previews). */
export const FALLBACK_WORKSPACE_LAYOUT: WorkspaceLayoutVm = {
  shellMinWidth: 480,
  shellMinHeight: 680,
  rightWorkspace: {
    minWidth: 288,
    defaultWidth: 440,
    maxWidth: 1440,
    file: {
      preferredWidth: 760,
      splitMinWidth: 500,
      treeDefaultWidth: 280,
      treeMinWidth: 200,
      treeMaxWidth: 420,
    },
  },
  conversation: { centerMinWidth: 360, centerAutoCollapseWidth: 420, windowMinWidth: 480 },
  contextCards: { centerMinWidth: 520, centerAutoCollapseWidth: 520, windowMinWidth: 520 },
  workflowCanvas: { centerMinWidth: 640, centerAutoCollapseWidth: 640, windowMinWidth: 640 },
  settings: { centerMinWidth: 480, centerAutoCollapseWidth: 480, windowMinWidth: 480 },
};
export const FALLBACK_WORKSPACE_FILES: WorkspaceFilesVm = {
  autoSaveDelayMs: 300,
  searchDebounceMs: 200,
  searchResultLimit: 500,
  textEditableMaxBytes: 2 * 1024 * 1024,
  textHighlightMaxChars: 120_000,
  textReadOnlyMaxBytes: 10 * 1024 * 1024,
  imagePreviewMaxBytes: 20 * 1024 * 1024,
  imagePreviewMaxPixels: 40_000_000,
  contentCacheEntries: 12,
  contentCacheMaxBytes: 16 * 1024 * 1024,
  watchDebounceMs: 150,
  externalAccessGrantTtlSeconds: 1_800,
  markdownLivePreviewMaxChars: 200_000,
  markdownEmbeddedImageLimit: 100,
  markdownEmbeddedImageMaxConcurrent: 4,
};
export const WORKSPACE_SIDEBAR_MIN_WIDTH = 176;
export const WORKSPACE_SIDEBAR_MAX_WIDTH = 420;
export const WORKSPACE_SIDEBAR_DEFAULT_WIDTH = 256;
export const RIGHT_WORKSPACE_MIN_WIDTH = FALLBACK_WORKSPACE_LAYOUT.rightWorkspace.minWidth;
export const RIGHT_WORKSPACE_MAX_WIDTH = FALLBACK_WORKSPACE_LAYOUT.rightWorkspace.maxWidth;
export const RIGHT_WORKSPACE_DEFAULT_WIDTH = FALLBACK_WORKSPACE_LAYOUT.rightWorkspace.defaultWidth;
export const WORKSPACE_LAYOUT_HYSTERESIS = 48;
export const FILE_WORKSPACE_RESIZE_DIRECTION_HOLD_MS = 120;

export interface WorkspaceAutoCollapseState {
  previousWidth: number;
  left: boolean;
  right: boolean;
}

export type WorkspaceAutoCollapsePresentation = Pick<WorkspaceAutoCollapseState, 'left' | 'right'>;

export interface FileWorkspaceResponsiveState {
  split: boolean;
  widthAtTransition: number;
}

export type FileWorkspaceResizeDirection = 'growing' | 'shrinking' | 'stationary';

export function resolveFileWorkspaceResizeDirection({
  previousShellWidth,
  shellWidth,
  previousDirection,
  elapsedSinceShellResizeMs,
  holdMs = FILE_WORKSPACE_RESIZE_DIRECTION_HOLD_MS,
}: {
  previousShellWidth: number;
  shellWidth: number;
  previousDirection: FileWorkspaceResizeDirection;
  elapsedSinceShellResizeMs: number;
  holdMs?: number;
}): FileWorkspaceResizeDirection {
  if (previousShellWidth > 0 && shellWidth !== previousShellWidth) {
    return shellWidth > previousShellWidth ? 'growing' : 'shrinking';
  }
  return elapsedSinceShellResizeMs <= holdMs ? previousDirection : 'stationary';
}

export interface WorkspaceAutoCollapseInput {
  availableWidth: number;
  centerMinWidth: number;
  centerAutoCollapseWidth: number;
  sidebarWidth: number;
  sidebarManuallyCollapsed: boolean;
  wantsRight: boolean;
  rightMinWidth?: number;
  rightWidthForStableLeftRestore?: number;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function reduceWorkspaceAutoCollapse(
  state: WorkspaceAutoCollapseState,
  input: WorkspaceAutoCollapseInput,
): WorkspaceAutoCollapseState {
  const {
    availableWidth,
    centerMinWidth,
    centerAutoCollapseWidth,
    sidebarWidth,
    sidebarManuallyCollapsed,
    wantsRight,
    rightMinWidth = RIGHT_WORKSPACE_MIN_WIDTH,
    rightWidthForStableLeftRestore = rightMinWidth,
  } = input;
  if (availableWidth <= 0) return state;

  const shrinking = state.previousWidth === 0 || availableWidth < state.previousWidth;
  const desiredSidebarWidth = sidebarManuallyCollapsed
    ? 0
    : clamp(sidebarWidth, WORKSPACE_SIDEBAR_MIN_WIDTH, WORKSPACE_SIDEBAR_MAX_WIDTH);
  const centerWidthBeforeLeftCollapse = wantsRight
    ? centerMinWidth
    : Math.max(centerMinWidth, centerAutoCollapseWidth);
  const needsAll = centerWidthBeforeLeftCollapse + desiredSidebarWidth + (wantsRight ? rightMinWidth : 0);
  const needsAllForStableRestore = centerWidthBeforeLeftCollapse
    + desiredSidebarWidth
    + (wantsRight ? Math.max(rightMinWidth, rightWidthForStableLeftRestore) : 0);
  const needsCenterAndRight = centerMinWidth + (wantsRight ? rightMinWidth : 0);
  let left = sidebarManuallyCollapsed ? false : state.left;
  let right = wantsRight ? state.right : false;

  if (!sidebarManuallyCollapsed && !left && availableWidth < needsAll) left = true;
  if (wantsRight && (sidebarManuallyCollapsed || left) && !right && availableWidth < needsCenterAndRight) {
    right = true;
  } else if (!shrinking) {
    if (right && availableWidth > needsCenterAndRight + WORKSPACE_LAYOUT_HYSTERESIS) {
      right = false;
    }
    if (!sidebarManuallyCollapsed && left && availableWidth > needsAllForStableRestore + WORKSPACE_LAYOUT_HYSTERESIS) {
      left = false;
    }
  }

  if (state.previousWidth === availableWidth && state.left === left && state.right === right) return state;
  return { previousWidth: availableWidth, left, right };
}

export function workspaceAutoCollapsePresentationChanged(
  current: WorkspaceAutoCollapsePresentation,
  next: WorkspaceAutoCollapsePresentation,
) {
  return current.left !== next.left || current.right !== next.right;
}

export function reduceFileWorkspaceResponsiveState(
  state: FileWorkspaceResponsiveState,
  width: number,
  splitMinWidth: number,
  direction: FileWorkspaceResizeDirection = 'stationary',
): FileWorkspaceResponsiveState {
  const split = width >= splitMinWidth;
  if (state.split === split) return state;
  if (direction === 'growing' && state.split) return state;
  if (direction === 'shrinking' && !state.split) return state;
  return { split, widthAtTransition: width };
}

export function resolveWorkspacePanelWidthFromLayout({
  layout,
  panelId,
  groupWidth,
  minWidth,
  maxWidth,
}: {
  layout: Layout;
  panelId: string;
  groupWidth: number;
  minWidth: number;
  maxWidth: number;
}) {
  const percentage = layout[panelId];
  if (percentage == null || groupWidth <= 0) return null;
  return clamp(
    Math.round(groupWidth * percentage / 100),
    minWidth,
    maxWidth,
  );
}

export function resolveRightWorkspaceWidthFromLayout(
  layout: Layout,
  groupWidth: number,
  bounds = FALLBACK_WORKSPACE_LAYOUT.rightWorkspace,
) {
  return resolveWorkspacePanelWidthFromLayout({
    layout,
    panelId: 'workspace-right',
    groupWidth,
    minWidth: bounds.minWidth,
    maxWidth: bounds.maxWidth,
  });
}

export function resolveRightWorkspacePanelMaxWidth({
  preferredWidth,
  minWidth,
  maxWidth,
  userResizing,
}: {
  preferredWidth: number;
  minWidth: number;
  maxWidth: number;
  userResizing: boolean;
}) {
  return userResizing ? maxWidth : clamp(preferredWidth, minWidth, maxWidth);
}

export function shouldPersistRightWorkspaceWidth(isUserInteraction: boolean, hasResizeIntent: boolean) {
  return isUserInteraction && hasResizeIntent;
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

export function workspaceLayoutProfileForPage(
  page: ConversationPage,
  layout: WorkspaceLayoutVm,
): WorkspaceLayoutProfileVm {
  if (page.kind === 'conversation-home' || page.kind === 'conversation-run') return layout.conversation;
  if (page.kind === 'contexts') return layout.contextCards;
  if (page.kind === 'settings') return layout.settings;
  return layout.workflowCanvas;
}

export function workspaceLayoutProfileForSurface({
  uiMode,
  conversationPage,
  primaryModule,
  layout,
}: {
  uiMode: DesktopUiMode;
  conversationPage: ConversationPage;
  primaryModule: PrimaryModule;
  layout: WorkspaceLayoutVm;
}): WorkspaceLayoutProfileVm {
  if (uiMode === 'conversation') return workspaceLayoutProfileForPage(conversationPage, layout);
  if (primaryModule === 'knowledge-base') return layout.contextCards;
  if (primaryModule === 'settings') return layout.settings;
  return layout.workflowCanvas;
}
