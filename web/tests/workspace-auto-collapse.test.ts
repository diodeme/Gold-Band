import { describe, expect, it } from 'vitest';

import {
  FALLBACK_WORKSPACE_LAYOUT,
  RIGHT_WORKSPACE_MIN_WIDTH,
  WORKSPACE_LAYOUT_HYSTERESIS,
  WORKSPACE_SIDEBAR_MIN_WIDTH,
  reduceFileWorkspaceResponsiveState,
  reduceWorkspaceAutoCollapse,
  resolveFileWorkspaceResizeDirection,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspaceCanonicalLayout,
  resolveWorkspacePanelWidthFromLayout,
  resolveWorkspaceUserResizeTarget,
  resolveRightWorkspaceSheetOpenTransition,
  workspaceAutoCollapsePresentationChanged,
  workspaceCanonicalLayoutMissingPanel,
  workspaceCanonicalLayoutNeedsConvergence,
  workspaceLayoutProfileForPage,
  workspaceLayoutProfileForSurface,
  type FileWorkspaceResponsiveState,
  type WorkspaceAutoCollapseState,
} from '@/components/workspace/workspace-layout';

const initial = (): WorkspaceAutoCollapseState => ({
  previousWidth: 1_100,
  left: false,
  right: false,
  rightOwnsWindowResize: false,
});

describe('workspace auto collapse state machine', () => {
  it('resolves all three panels as one canonical layout transaction', () => {
    const layout = resolveWorkspaceCanonicalLayout({
      groupWidth: 1_288,
      centerMinWidth: 360,
      leftVisible: true,
      leftWidth: 176,
      rightVisible: true,
      rightPreferredWidth: 690,
    });

    expect(layout).not.toBeNull();
    expect(layout!['workspace-navigation'] * 12.88).toBeCloseTo(176, 5);
    expect(layout!['workspace-center'] * 12.88).toBeCloseTo(422, 5);
    expect(layout!['workspace-right'] * 12.88).toBeCloseTo(690, 5);
  });

  it('constrains the right workspace without sacrificing the visible navigation', () => {
    const layout = resolveWorkspaceCanonicalLayout({
      groupWidth: 918,
      centerMinWidth: 360,
      leftVisible: true,
      leftWidth: 176,
      rightVisible: true,
      rightPreferredWidth: 690,
    });

    expect(layout!['workspace-navigation'] * 9.18).toBeCloseTo(176, 5);
    expect(layout!['workspace-center'] * 9.18).toBeCloseTo(360, 5);
    expect(layout!['workspace-right'] * 9.18).toBeCloseTo(382, 5);
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
      { previousWidth: 700, left: true, right: true, rightOwnsWindowResize: false },
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
    let state: WorkspaceAutoCollapseState = {
      previousWidth: 640,
      left: true,
      right: true,
      rightOwnsWindowResize: false,
    };
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
      { previousWidth: 700, left: true, right: true, rightOwnsWindowResize: false },
      { availableWidth: 720, centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: false },
    );
    expect(noWorkspace.right).toBe(false);
  });

  it('applies the responsive order when a workspace opens at an already narrow width', () => {
    const openedWithRoomAfterNavigation = reduceWorkspaceAutoCollapse(
      { previousWidth: 800, left: false, right: false, rightOwnsWindowResize: false },
      { availableWidth: 950, centerMinWidth: 420, centerAutoCollapseWidth: 480, sidebarWidth: 256, sidebarManuallyCollapsed: false, wantsRight: true },
    );
    expect(openedWithRoomAfterNavigation).toMatchObject({ left: true, right: false });

    const openedCompact = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: false, right: false, rightOwnsWindowResize: false },
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

  it('lets the right workspace absorb window growth until its preference is restored', () => {
    const input = {
      centerMinWidth: 360,
      centerAutoCollapseWidth: 420,
      sidebarWidth: 176,
      sidebarManuallyCollapsed: false,
      wantsRight: true,
      rightPreferredWidth: 690,
    };
    let state: WorkspaceAutoCollapseState = {
      previousWidth: 918,
      left: false,
      right: false,
      rightOwnsWindowResize: true,
    };
    let presentationUpdates = 0;

    for (let availableWidth = 919; availableWidth < 1_226; availableWidth += 1) {
      const next = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth });
      if (workspaceAutoCollapsePresentationChanged(state, next)) presentationUpdates += 1;
      state = next;
      expect(state.rightOwnsWindowResize).toBe(true);
    }

    const restored = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth: 1_226 });
    expect(presentationUpdates).toBe(0);
    expect(restored).toMatchObject({
      left: false,
      right: false,
      rightOwnsWindowResize: false,
    });
    expect(workspaceAutoCollapsePresentationChanged(state, restored)).toBe(true);
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
    expect(resolveWorkspacePanelWidthFromLayout({
      layout: { 'workspace-navigation': 0 },
      panelId: 'workspace-navigation',
      groupWidth: 1_600,
      minWidth: 176,
      maxWidth: 420,
    })).toBeNull();
  });

  it('converts the completed panel layout into a persisted right-side pixel width', () => {
    expect(resolveRightWorkspaceWidthFromLayout({ 'workspace-center': 62.5, 'workspace-right': 37.5 }, 1_600)).toBe(600);
    expect(resolveRightWorkspaceWidthFromLayout({ 'workspace-center': 100 }, 1_600)).toBeNull();
  });

  it('attributes a completed user resize from the changed outer panel', () => {
    const previousLayout = {
      'workspace-navigation': 13.869,
      'workspace-center': 28.369,
      'workspace-right': 57.762,
    };
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-center': 63.436, 'workspace-right': 22.695 },
      isUserInteraction: true,
    })).toBe('right');
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-navigation': 20, 'workspace-center': 22.238 },
      isUserInteraction: true,
    })).toBe('left');
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-navigation': 20, 'workspace-right': 51.631 },
      isUserInteraction: true,
    })).toBeNull();
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: {
        'workspace-navigation': 23.743,
        'workspace-center': 28.169,
        'workspace-right': 48.088,
      },
      isUserInteraction: true,
    })).toBe('left');
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-navigation': 20, 'workspace-right': 51.631 },
      isUserInteraction: true,
      focusedTarget: 'right',
    })).toBe('right');
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-navigation': 13.87, 'workspace-right': 57.761 },
      isUserInteraction: true,
    })).toBeNull();
    expect(resolveWorkspaceUserResizeTarget({
      previousLayout,
      layout: { ...previousLayout, 'workspace-center': 63.436, 'workspace-right': 22.695 },
      isUserInteraction: false,
    })).toBeNull();
  });

  it('detects when a canonical group layout left a visible outer panel collapsed', () => {
    const target = {
      'workspace-navigation': 13.772,
      'workspace-center': 33.489,
      'workspace-right': 52.739,
    };
    const applied = {
      'workspace-navigation': 0,
      'workspace-center': 50.078,
      'workspace-right': 49.922,
    };

    expect(workspaceCanonicalLayoutMissingPanel(target, applied, 'workspace-navigation')).toBe(true);
    expect(workspaceCanonicalLayoutMissingPanel(target, applied, 'workspace-right')).toBe(false);
  });

  it('detects a partially constrained layout that still needs canonical convergence', () => {
    const target = {
      'workspace-navigation': 332 / 13.6,
      'workspace-center': 544 / 13.6,
      'workspace-right': 484 / 13.6,
    };
    const constrained = {
      'workspace-navigation': 236 / 13.6,
      'workspace-center': 640 / 13.6,
      'workspace-right': 484 / 13.6,
    };
    const withinOnePixel = {
      ...target,
      'workspace-navigation': 331.5 / 13.6,
      'workspace-center': 544.5 / 13.6,
    };

    expect(workspaceCanonicalLayoutNeedsConvergence(target, constrained, 1_360)).toBe(true);
    expect(workspaceCanonicalLayoutNeedsConvergence(target, withinOnePixel, 1_360)).toBe(false);
  });

  it('keeps an automatically collapsed workspace hidden until a resource is explicitly opened', () => {
    expect(resolveRightWorkspaceSheetOpenTransition({
      compact: true,
      previousOpenRevision: 2,
      openRevision: 2,
    })).toEqual({ openSheet: false, handledOpenRevision: 2 });
    expect(resolveRightWorkspaceSheetOpenTransition({
      compact: true,
      previousOpenRevision: 2,
      openRevision: 3,
    })).toEqual({ openSheet: true, handledOpenRevision: 3 });
    expect(resolveRightWorkspaceSheetOpenTransition({
      compact: false,
      previousOpenRevision: 2,
      openRevision: 3,
    })).toEqual({ openSheet: false, handledOpenRevision: 2 });
    expect(resolveRightWorkspaceSheetOpenTransition({
      compact: true,
      previousOpenRevision: 2,
      openRevision: 3,
    })).toEqual({ openSheet: true, handledOpenRevision: 3 });
  });
});
