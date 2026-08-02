import { describe, expect, it } from 'vitest';

import {
  FALLBACK_WORKSPACE_LAYOUT,
  reduceWorkspaceAutoCollapse,
  resolveRightWorkspaceWidthFromLayout,
  resolveWorkspacePanelWidthFromLayout,
  shouldOpenRightWorkspaceSheet,
  workspaceAutoCollapsePresentationChanged,
  workspaceLayoutProfileForPage,
  workspaceLayoutProfileForSurface,
  type WorkspaceAutoCollapseState,
} from '@/components/workspace/workspace-layout';

const initial = (): WorkspaceAutoCollapseState => ({ previousWidth: 1_100, left: false, right: false });

describe('workspace auto collapse state machine', () => {
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
    const collapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: 950 });
    const nearBoundary = reduceWorkspaceAutoCollapse(collapsed, { ...input, availableWidth: 1_020 });
    expect(nearBoundary.left).toBe(true);
    const restored = reduceWorkspaceAutoCollapse(nearBoundary, { ...input, availableWidth: 1_050 });
    expect(restored.left).toBe(false);
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
    let state = initial();
    let presentationUpdates = 0;
    for (let availableWidth = 1_099; availableWidth >= 1_000; availableWidth -= 1) {
      const next = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth });
      if (workspaceAutoCollapsePresentationChanged(state, next)) presentationUpdates += 1;
      state = next;
    }
    expect(state.previousWidth).toBe(1_000);
    expect(presentationUpdates).toBe(0);

    const crossed = reduceWorkspaceAutoCollapse(state, { ...input, availableWidth: 995 });
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
