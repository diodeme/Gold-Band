import { LogicalSize, type Window as TauriWindow } from '@tauri-apps/api/window';

import type { WorkspaceLayoutProfileVm, WorkspaceLayoutVm } from '@/types';

export interface DesktopWindowMinimumPlan {
  minimum: { width: number; height: number };
  resizeTo: { width: number; height: number } | null;
}

export interface DesktopWindowMinimumSyncState {
  appliedMinimum: DesktopWindowMinimumPlan['minimum'] | null;
  pending: boolean;
}

export const INITIAL_DESKTOP_WINDOW_MINIMUM_SYNC_STATE: DesktopWindowMinimumSyncState = {
  appliedMinimum: null,
  pending: false,
};

function sameMinimum(
  left: DesktopWindowMinimumPlan['minimum'] | null,
  right: DesktopWindowMinimumPlan['minimum'],
) {
  return left?.width === right.width && left.height === right.height;
}

export function planDesktopWindowMinimum({
  currentWidth,
  currentHeight,
  layout,
  profile,
}: {
  currentWidth: number;
  currentHeight: number;
  layout: WorkspaceLayoutVm;
  profile: WorkspaceLayoutProfileVm;
}): DesktopWindowMinimumPlan {
  const minimum = {
    width: Math.max(layout.shellMinWidth, profile.windowMinWidth),
    height: layout.shellMinHeight,
  };
  const width = Math.max(currentWidth, minimum.width);
  const height = Math.max(currentHeight, minimum.height);
  return {
    minimum,
    resizeTo: width === currentWidth && height === currentHeight ? null : { width, height },
  };
}

export async function syncDesktopWindowMinimum(
  appWindow: TauriWindow,
  layout: WorkspaceLayoutVm,
  profile: WorkspaceLayoutProfileVm,
  state: DesktopWindowMinimumSyncState,
): Promise<DesktopWindowMinimumSyncState> {
  const minimum = {
    width: Math.max(layout.shellMinWidth, profile.windowMinWidth),
    height: layout.shellMinHeight,
  };
  if (sameMinimum(state.appliedMinimum, minimum)) {
    return { appliedMinimum: state.appliedMinimum, pending: false };
  }
  if (await appWindow.isMaximized()) {
    return { appliedMinimum: state.appliedMinimum, pending: true };
  }

  const [physicalSize, scaleFactor] = await Promise.all([
    appWindow.innerSize(),
    appWindow.scaleFactor(),
  ]);
  const currentSize = physicalSize.toLogical(scaleFactor);
  const plan = planDesktopWindowMinimum({
    currentWidth: currentSize.width,
    currentHeight: currentSize.height,
    layout,
    profile,
  });
  await appWindow.setMinSize(new LogicalSize(plan.minimum.width, plan.minimum.height));
  if (plan.resizeTo) {
    await appWindow.setSize(new LogicalSize(plan.resizeTo.width, plan.resizeTo.height));
  }
  return { appliedMinimum: plan.minimum, pending: false };
}
