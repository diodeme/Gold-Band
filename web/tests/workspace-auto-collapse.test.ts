import { describe, expect, it } from 'vitest';

import {
  reduceWorkspaceAutoCollapse,
  resolveRightWorkspaceMaxWidth,
  shouldOpenRightWorkspaceSheet,
  WORKSPACE_LAYOUT_PROFILES,
  type WorkspaceAutoCollapseState,
} from '@/components/workspace/WorkspaceShell';

const initial = (): WorkspaceAutoCollapseState => ({ previousWidth: 1_100, left: false, right: false });

describe('workspace auto collapse state machine', () => {
  it('lets text conversations become narrower than card and canvas pages', () => {
    expect(WORKSPACE_LAYOUT_PROFILES.conversation.centerMinWidth).toBe(360);
    expect(WORKSPACE_LAYOUT_PROFILES.conversation.centerMinWidth)
      .toBeLessThan(WORKSPACE_LAYOUT_PROFILES.contextCards.centerMinWidth);
    expect(WORKSPACE_LAYOUT_PROFILES.conversation.centerMinWidth)
      .toBeLessThan(WORKSPACE_LAYOUT_PROFILES.workflowCanvas.centerMinWidth);
  });

  it('collapses left then right while shrinking and restores right then left while growing', () => {
    const input = { centerMinWidth: 420, sidebarManuallyCollapsed: false, wantsRight: true };
    const leftCollapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: 900 });
    expect(leftCollapsed).toMatchObject({ left: true, right: false });

    const bothCollapsed = reduceWorkspaceAutoCollapse(leftCollapsed, { ...input, availableWidth: 700 });
    expect(bothCollapsed).toMatchObject({ left: true, right: true });

    const rightRestored = reduceWorkspaceAutoCollapse(bothCollapsed, { ...input, availableWidth: 800 });
    expect(rightRestored).toMatchObject({ left: true, right: false });

    const bothRestored = reduceWorkspaceAutoCollapse(rightRestored, { ...input, availableWidth: 1_000 });
    expect(bothRestored).toMatchObject({ left: false, right: false });
  });

  it('restores both automatically collapsed panels after a single maximize jump', () => {
    const restored = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: true, right: true },
      {
        availableWidth: 1_100,
        centerMinWidth: 420,
        sidebarManuallyCollapsed: false,
        wantsRight: true,
      },
    );
    expect(restored).toMatchObject({ left: false, right: false });
  });

  it('uses hysteresis so a boundary oscillation does not flicker', () => {
    const input = { centerMinWidth: 420, sidebarManuallyCollapsed: false, wantsRight: true };
    const collapsed = reduceWorkspaceAutoCollapse(initial(), { ...input, availableWidth: 900 });
    const nearBoundary = reduceWorkspaceAutoCollapse(collapsed, { ...input, availableWidth: 960 });
    expect(nearBoundary.left).toBe(true);
    const restored = reduceWorkspaceAutoCollapse(nearBoundary, { ...input, availableWidth: 990 });
    expect(restored.left).toBe(false);
  });

  it('keeps manual intent separate from automatic collapse', () => {
    const manualLeft = reduceWorkspaceAutoCollapse(initial(), {
      availableWidth: 700,
      centerMinWidth: 420,
      sidebarManuallyCollapsed: true,
      wantsRight: true,
    });
    expect(manualLeft).toMatchObject({ left: false, right: true });

    const noWorkspace = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: true, right: true },
      { availableWidth: 720, centerMinWidth: 420, sidebarManuallyCollapsed: false, wantsRight: false },
    );
    expect(noWorkspace.right).toBe(false);
  });

  it('applies the responsive order when a workspace opens at an already narrow width', () => {
    const openedWithRoomAfterNavigation = reduceWorkspaceAutoCollapse(
      { previousWidth: 800, left: false, right: false },
      { availableWidth: 800, centerMinWidth: 420, sidebarManuallyCollapsed: false, wantsRight: true },
    );
    expect(openedWithRoomAfterNavigation).toMatchObject({ left: true, right: false });

    const openedCompact = reduceWorkspaceAutoCollapse(
      { previousWidth: 700, left: false, right: false },
      { availableWidth: 700, centerMinWidth: 420, sidebarManuallyCollapsed: false, wantsRight: true },
    );
    expect(openedCompact).toMatchObject({ left: true, right: true });
  });

  it('caps right-side dragging before the center crosses its page minimum', () => {
    expect(resolveRightWorkspaceMaxWidth({
      availableWidth: 1_000,
      centerMinWidth: 420,
      leftVisible: true,
    })).toBe(380);
    expect(resolveRightWorkspaceMaxWidth({
      availableWidth: 1_400,
      centerMinWidth: 420,
      leftVisible: true,
    })).toBe(720);
    expect(resolveRightWorkspaceMaxWidth({
      availableWidth: 800,
      centerMinWidth: 420,
      leftVisible: false,
    })).toBe(380);
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
