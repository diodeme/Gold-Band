import type { Layout } from 'react-resizable-panels';
import type {
  ConversationPage,
  DesktopUiMode,
  PrimaryModule,
  WorkspaceLayoutProfileVm,
  WorkspaceLayoutVm,
} from '../../types';

/** Used only before desktop bootstrap is available (for example in browser previews). */
export const FALLBACK_WORKSPACE_LAYOUT: WorkspaceLayoutVm = {
  shellMinWidth: 480,
  shellMinHeight: 680,
  conversation: { centerMinWidth: 360, centerAutoCollapseWidth: 420, windowMinWidth: 480 },
  contextCards: { centerMinWidth: 520, centerAutoCollapseWidth: 520, windowMinWidth: 520 },
  workflowCanvas: { centerMinWidth: 640, centerAutoCollapseWidth: 640, windowMinWidth: 640 },
  settings: { centerMinWidth: 480, centerAutoCollapseWidth: 480, windowMinWidth: 480 },
};
export const WORKSPACE_SIDEBAR_MIN_WIDTH = 200;
export const WORKSPACE_SIDEBAR_MAX_WIDTH = 420;
export const WORKSPACE_SIDEBAR_DEFAULT_WIDTH = 256;
export const RIGHT_WORKSPACE_MIN_WIDTH = 320;
export const RIGHT_WORKSPACE_MAX_WIDTH = 720;
export const RIGHT_WORKSPACE_DEFAULT_WIDTH = 440;
export const WORKSPACE_LAYOUT_HYSTERESIS = 48;

export interface WorkspaceAutoCollapseState {
  previousWidth: number;
  left: boolean;
  right: boolean;
}

export interface WorkspaceAutoCollapseInput {
  availableWidth: number;
  centerMinWidth: number;
  centerAutoCollapseWidth: number;
  sidebarWidth: number;
  sidebarManuallyCollapsed: boolean;
  wantsRight: boolean;
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
  } = input;
  if (availableWidth <= 0) return state;

  const shrinking = state.previousWidth === 0 || availableWidth < state.previousWidth;
  const desiredSidebarWidth = sidebarManuallyCollapsed
    ? 0
    : clamp(sidebarWidth, WORKSPACE_SIDEBAR_MIN_WIDTH, WORKSPACE_SIDEBAR_MAX_WIDTH);
  const centerWidthBeforeLeftCollapse = wantsRight
    ? centerMinWidth
    : Math.max(centerMinWidth, centerAutoCollapseWidth);
  const needsAll = centerWidthBeforeLeftCollapse + desiredSidebarWidth + (wantsRight ? RIGHT_WORKSPACE_MIN_WIDTH : 0);
  const needsCenterAndRight = centerMinWidth + (wantsRight ? RIGHT_WORKSPACE_MIN_WIDTH : 0);
  let left = sidebarManuallyCollapsed ? false : state.left;
  let right = wantsRight ? state.right : false;

  if (!sidebarManuallyCollapsed && !left && availableWidth < needsAll) left = true;
  if (wantsRight && (sidebarManuallyCollapsed || left) && !right && availableWidth < needsCenterAndRight) {
    right = true;
  } else if (!shrinking) {
    if (right && availableWidth > needsCenterAndRight + WORKSPACE_LAYOUT_HYSTERESIS) {
      right = false;
    }
    if (!sidebarManuallyCollapsed && left && availableWidth > needsAll + WORKSPACE_LAYOUT_HYSTERESIS) {
      left = false;
    }
  }

  if (state.previousWidth === availableWidth && state.left === left && state.right === right) return state;
  return { previousWidth: availableWidth, left, right };
}

export function resolveRightWorkspaceMaxWidth({
  availableWidth,
  centerMinWidth,
  leftWidth,
}: {
  availableWidth: number;
  centerMinWidth: number;
  leftWidth: number;
}) {
  if (availableWidth <= 0) return RIGHT_WORKSPACE_MAX_WIDTH;
  const reservedWidth = centerMinWidth + Math.max(0, leftWidth);
  return clamp(
    availableWidth - reservedWidth,
    RIGHT_WORKSPACE_MIN_WIDTH,
    RIGHT_WORKSPACE_MAX_WIDTH,
  );
}

export function resolveRightWorkspaceWidthFromLayout(layout: Layout, groupWidth: number) {
  const rightPercentage = layout['workspace-right'];
  if (rightPercentage == null || groupWidth <= 0) return null;
  return clamp(
    Math.round(groupWidth * rightPercentage / 100),
    RIGHT_WORKSPACE_MIN_WIDTH,
    RIGHT_WORKSPACE_MAX_WIDTH,
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
