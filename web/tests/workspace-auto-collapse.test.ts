import { describe, expect, it } from 'vitest';

import { reduceWorkspaceAutoCollapse, type WorkspaceAutoCollapseState } from '@/components/workspace/WorkspaceShell';

const initial = (): WorkspaceAutoCollapseState => ({ previousWidth: 1_100, left: false, right: false });

describe('workspace auto collapse state machine', () => {
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
});
