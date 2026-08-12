import { describe, expect, it } from 'vitest';

import {
  FALLBACK_WORKSPACE_LAYOUT,
  RIGHT_WORKSPACE_MIN_WIDTH,
  WORKSPACE_LAYOUT_HYSTERESIS,
  WORKSPACE_SIDEBAR_MIN_WIDTH,
  reduceFileWorkspaceResponsiveState,
  reduceWorkspaceAutoCollapse,
  resolveFileWorkspaceResizeDirection,
  resolveRightWorkspacePanelMaxWidth,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  shouldOpenRightWorkspaceSheet,
  shouldPersistRightWorkspaceWidth,
  syncRightWorkspacePanelPresentation,
  workspaceAutoCollapsePresentationChanged,
  workspaceLayoutProfileForPage,
  workspaceLayoutProfileForSurface,
  type FileWorkspaceResponsiveState,
  type WorkspaceAutoCollapseState,
} from '@/components/workspace/workspace-layout';

const initial = (): WorkspaceAutoCollapseState => ({ previousWidth: 1_100, left: false, right: false });

describe('workspace auto collapse state machine', () => {
  it('restores the canonical right workspace width whenever the dock becomes visible', () => {
    const calls: string[] = [];
    let collapsed = true;
    const panel = {
      collapse: () => { collapsed = true; calls.push('collapse'); },
      expand: () => { collapsed = false; calls.push('expand'); },
      isCollapsed: () => collapsed,
      resize: (size: number | string) => { calls.push(`resize:${size}`); },
    };

    syncRightWorkspacePanelPresentation({ panel, visible: true, preferredWidth: 767 });

    expect(calls).toEqual(['expand', 'resize:767']);
  });

  it('collapses a hidden right workspace without applying a stale width', () => {
    const calls: string[] = [];
    let collapsed = false;
    const panel = {
      collapse: () => { collapsed = true; calls.push('collapse'); },
      expand: () => { collapsed = false; calls.push('expand'); },
      isCollapsed: () => collapsed,
      resize: (size: number | string) => { calls.push(`resize:${size}`); },
    };

    syncRightWorkspacePanelPresentation({ panel, visible: false, preferredWidth: 767 });

    expect(calls).toEqual(['collapse']);
  });

  it('allows the navigation sidebar to shrink to its compact readable width', () => {
    expect(WORKSPACE_SIDEBAR_MIN_WIDTH).toBe(176);
  });

  it('keeps file workspace pixel widths outside React presentation state', () => {
    let state: FileWorkspaceResponsiveState = { split: false, widthAtTransition: 0 };
    const compactState = state;

    for (let width = 320; width < 500; width += 1) {
      state = reduceFileWorkspaceResponsiveState(state, width, 500);
      expect(state).toBe(compactState);
    }

    state = reduceFileWorkspaceResponsiveState(state, 500, 500);
    expect(state).toEqual({ split: true, widthAtTransition: 500 });
    const splitState = state;

    for (let width = 501; width <= 1_440; width += 1) {
      state = reduceFileWorkspaceResponsiveState(state, width, 500);
      expect(state).toBe(splitState);
    }

    state = reduceFileWorkspaceResponsiveState(state, 499, 500);
    expect(state).toEqual({ split: false, widthAtTransition: 499 });
  });

  it('keeps file workspace presentation monotonic in the window resize direction', () => {
    const split: FileWorkspaceResponsiveState = { split: true, widthAtTransition: 500 };
    expect(reduceFileWorkspaceResponsiveState(split, 479, 500, 'growing')).toBe(split);
    expect(reduceFileWorkspaceResponsiveState(split, 499, 500, 'shrinking'))
      .toEqual({ split: false, widthAtTransition: 499 });

    const compact: FileWorkspaceResponsiveState = { split: false, widthAtTransition: 499 };
    expect(reduceFileWorkspaceResponsiveState(compact, 568, 500, 'shrinking')).toBe(compact);
    expect(reduceFileWorkspaceResponsiveState(compact, 500, 500, 'growing'))
      .toEqual({ split: true, widthAtTransition: 500 });

    expect(reduceFileWorkspaceResponsiveState(split, 480, 500, 'stationary'))
      .toEqual({ split: false, widthAtTransition: 480 });
    expect(reduceFileWorkspaceResponsiveState(compact, 560, 500, 'stationary'))
      .toEqual({ split: true, widthAtTransition: 560 });
  });

  it('holds the last window direction across follow-up layout callbacks until resize settles', () => {
    expect(resolveFileWorkspaceResizeDirection({
      previousShellWidth: 936,
      shellWidth: 928,
      previousDirection: 'stationary',
      elapsedSinceShellResizeMs: 1_000,
    })).toBe('shrinking');
    expect(resolveFileWorkspaceResizeDirection({
      previousShellWidth: 928,
      shellWidth: 928,
      previousDirection: 'shrinking',
      elapsedSinceShellResizeMs: 16,
    })).toBe('shrinking');
    expect(resolveFileWorkspaceResizeDirection({
      previousShellWidth: 928,
      shellWidth: 928,
      previousDirection: 'shrinking',
      elapsedSinceShellResizeMs: 121,
    })).toBe('stationary');
  });

  it('lets text conversations become narrower than card and canvas pages', () => {
    expect(FALLBACK_WORKSPACE_LAYOUT.conversation.centerMinWidth).toBe(360);
    expect(FALLBACK_WORKSPACE_LAYOUT.conversation.centerMinWidth)
      .toBeLessThan(FALLBACK_WORKSPACE_LAYOUT.contextCards.centerMinWidth);
    expect(FALLBACK_WORKSPACE_LAYOUT.conversation.centerMinWidth)
      .toBeLessThan(FALLBACK_WORKSPACE_LAYOUT.workflowCanvas.centerMinWidth);
    expect(FALLBACK_WORKSPACE_LAYOUT.conversation.centerAutoCollapseWidth)
      .toBeGreaterThan(FALLBACK_WORKSPACE_LAYOUT.conversation.centerMinWidth);
  });

  it('resolves page profiles from the bootstrapped app configuration', () => {
    expect(workspaceLayoutProfileForPage({ kind: 'conversation-home' }, FALLBACK_WORKSPACE_LAYOUT))
      .toBe(FALLBACK_WORKSPACE_LAYOUT.conversation);
    expect(workspaceLayoutProfileForPage({ kind: 'contexts' }, FALLBACK_WORKSPACE_LAYOUT))
      .toBe(FALLBACK_WORKSPACE_LAYOUT.contextCards);
    expect(workspaceLayoutProfileForSurface({
      uiMode: 'workbench',
      conversationPage: { kind: 'conversation-home' },
      primaryModule: 'settings',
      layout: FALLBACK_WORKSPACE_LAYOUT,
    })).toBe(FALLBACK_WORKSPACE_LAYOUT.settings);
  });

  it('collapses left then right while shrinking and restores right then left while growing', () => {
    const input = { centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true };
    const leftCollapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: 950 });
    expect(leftCollapsed).toMatchObject({ left: true, right: false });

    const bothCollapsed = reduceWorkspaceAutoCollapse(leftCollapsed, { ...input, availableWidth: 700 });
    expect(bothCollapsed).toMatchObject({ left: true, right: true });

    const rightRestored = reduceWorkspaceAutoCollapse(bothCollapsed, { ...input, availableWidth: 800 });
    expect(rightRestored).toMatchObject({ left: true, right: false });

    const bothRestored = reduceWorkspaceAutoCollapse(rightRestored, { ...input, availableWidth: 1_100 });
    expect(bothRestored).toMatchObject({ left: false, right: false });
  });

  it('restores both automatically collapsed panels after a single maximize jump', () => {
    const restored = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: true, right: true },
      {
        availableWidth: 1_100,
        centerMinWidth: 420,
        centerAutoCollapseWidth: 480,
        sidebarWidth: 256,
        sidebarManuallyCollapsed: false,
        wantsRight: true,
      },
    );
    expect(restored).toMatchObject({ left: false, right: false });
  });

  it('uses hysteresis so a boundary oscillation does not flicker', () => {
    const input = { centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true };
    const collapseBoundary = input.centerMinWidth + input.sidebarWidth + RIGHT_WORKSPACE_MIN_WIDTH;
    const restoreBoundary = collapseBoundary + WORKSPACE_LAYOUT_HYSTERESIS;
    const collapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: collapseBoundary - 1 });
    const nearBoundary = reduceWorkspaceAutoCollapse(collapsed, { ...input, availableWidth: restoreBoundary });
    expect(nearBoundary.left).toBe(true);
    const restored = reduceWorkspaceAutoCollapse(nearBoundary, { ...input, availableWidth: restoreBoundary + 1 });
    expect(restored.left).toBe(false);
  });

  it('does not restore navigation until the file workspace can keep its visible dual columns', () => {
    const input = {
      centerMinWidth: 360,
      centerAutoCollapseWidth: 420,
      sidebarWidth: 256,
      sidebarManuallyCollapsed: false,
      wantsRight: true,
      rightMinWidth: 320,
      rightWidthForStableLeftRestore: 540,
    };
    let state: WorkspaceAutoCollapseState = { previousWidth: 640, left: true, right: true };
    let dualColumnsBecameVisible = false;

    for (let availableWidth = 641; availableWidth <= 1_300; availableWidth += 1) {
      state = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth });
      const visibleSidebarWidth = state.left ? 0 : input.sidebarWidth;
      const availableRightWidth = state.right
        ? 0
        : availableWidth - input.centerMinWidth - visibleSidebarWidth;
      const dualColumnsVisible = availableRightWidth >= input.rightWidthForStableLeftRestore;

      if (dualColumnsVisible) dualColumnsBecameVisible = true;
      if (dualColumnsBecameVisible) expect(dualColumnsVisible).toBe(true);
    }

    expect(state).toMatchObject({ left: false, right: false });
  });

  it('keeps manual intent separate from automatic collapse', () => {
    const manualLeft = reduceWorkspaceAutoCollapse(initial(), {
      availableWidth: 700,
      centerMinWidth: 420,
      centerAutoCollapseWidth: 480,
      sidebarWidth: 256,
      sidebarManuallyCollapsed: true,
      wantsRight: true,
    });
    expect(manualLeft).toMatchObject({ left: false, right: true });

    const noWorkspace = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: true, right: true },
      { availableWidth: 720, centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: false },
    );
    expect(noWorkspace.right).toBe(false);
  });

  it('applies the responsive order when a workspace opens at an already narrow width', () => {
    const openedWithRoomAfterNavigation = reduceWorkspaceAutoCollapse(
      { previousWidth: 800, left: false, right: false },
      { availableWidth: 950, centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true },
    );
    expect(openedWithRoomAfterNavigation).toMatchObject({ left: true, right: false });

    const openedCompact = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: false, right: false },
      { availableWidth: 700, centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true },
    );
    expect(openedCompact).toMatchObject({ left: true, right: true });
  });

  it('uses the center comfort width to collapse the left sidebar when no right workspace is open', () => {
    const input = {
      centerMinWidth: 360,
      centerAutoCollapseWidth: 420,
      sidebarWidth: 256,
      sidebarManuallyCollapsed: false,
      wantsRight: false,
    };
    const collapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: 650 });
    expect(collapsed).toMatchObject({ left: true, right: false });

    const heldByHysteresis = reduceWorkspaceAutoCollapse(collapsed, { ...input, availableWidth: 700 });
    expect(heldByHysteresis.left).toBe(true);

    const restored = reduceWorkspaceAutoCollapse(heldByHysteresis, { ...input, availableWidth: 730 });
    expect(restored.left).toBe(false);
  });

  it('does not publish React presentation updates for per-pixel widths inside one threshold band', () => {
    const input = { centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true };
    const collapseBoundary = input.centerMinWidth + input.sidebarWidth + RIGHT_WORKSPACE_MIN_WIDTH;
    let state = initial();
    let presentationUpdates = 0;
    for (let availableWidth = 1_099; availableWidth >= collapseBoundary; availableWidth -= 1) {
      const next = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth });
      if (workspaceAutoCollapsePresentationChanged(state, next)) presentationUpdates += 1;
      state = next;
    }
    expect(state.previousWidth).toBe(collapseBoundary);
    expect(presentationUpdates).toBe(0);

    const crossed = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth: collapseBoundary - 1 });
    expect(workspaceAutoCollapsePresentationChanged(state, crossed)).toBe(true);
    expect(crossed).toMatchObject({ left: true, right: false });
  });

  it('converts any completed panel layout into a bounded persisted pixel width', () => {
    expect(resolveWorkspacePanelWidthFromLayout({
      layout: { 'workspace-navigation': 20 },
      panelId: 'workspace-navigation',
      groupWidth: 1_600,
      minWidth: 200,
      maxWidth: 420,
    })).toBe(320);
    expect(resolveWorkspacePanelWidthFromLayout({
      layout: { 'workspace-navigation': 50 },
      panelId: 'workspace-navigation',
      groupWidth: 1_600,
      minWidth: 200,
      maxWidth: 420,
    })).toBe(420);
  });

  it('converts the completed panel layout into a persisted right-side pixel width', () => {
    expect(resolveRightWorkspaceWidthFromLayout({ 'workspace-center': 62.5, 'workspace-right': 37.5 }, 1_600)).toBe(600);
    expect(resolveRightWorkspaceWidthFromLayout({ 'workspace-center': 100 }, 1_600)).toBeNull();
  });

  it('caps automatic right-panel growth at the preference and unlocks the configured range for direct resizing', () => {
    expect(resolveRightWorkspacePanelMaxWidth({
      preferredWidth: 760,
      minWidth: 320,
      maxWidth: 1440,
      userResizing: false,
    })).toBe(760);
    expect(resolveRightWorkspacePanelMaxWidth({
      preferredWidth: 760,
      minWidth: 320,
      maxWidth: 1440,
      userResizing: true,
    })).toBe(1440);
    expect(resolveRightWorkspacePanelMaxWidth({
      preferredWidth: 200,
      minWidth: 320,
      maxWidth: 1440,
      userResizing: false,
    })).toBe(320);
  });

  it('persists right width only for a direct right-separator interaction', () => {
    expect(shouldPersistRightWorkspaceWidth(true, true)).toBe(true);
    expect(shouldPersistRightWorkspaceWidth(true, false)).toBe(false);
    expect(shouldPersistRightWorkspaceWidth(false, true)).toBe(false);
  });

  it('keeps an automatically collapsed workspace hidden until a resource is explicitly opened', () => {
    expect(shouldOpenRightWorkspaceSheet({
      compact: true,
      previousOpenRevision: 2,
      openRevision: 2,
    })).toBe(false);
    expect(shouldOpenRightWorkspaceSheet({
      compact: true,
      previousOpenRevision: 2,
      openRevision: 3,
    })).toBe(true);
    expect(shouldOpenRightWorkspaceSheet({
      compact: false,
      previousOpenRevision: 2,
      openRevision: 3,
    })).toBe(false);
  });
});
