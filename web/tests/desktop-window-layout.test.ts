import { PhysicalSize, type Window as TauriWindow } from '@tauri-apps/api/window';
import { describe, expect, it, vi } from 'vitest';

import { FALLBACK_WORKSPACE_LAYOUT } from '@/components/workspace/workspace-layout';
import {
  planDesktopWindowMinimum,
  syncDesktopWindowMinimum,
  type DesktopWindowMinimumSyncState,
} from '@/lib/desktop-window-layout';

function createWindowHost({
  maximized,
  width = 1280,
  height = 800,
}: {
  maximized: boolean;
  width?: number;
  height?: number;
}) {
  const host = {
    isMaximized: vi.fn().mockResolvedValue(maximized),
    innerSize: vi.fn().mockResolvedValue(new PhysicalSize(width, height)),
    scaleFactor: vi.fn().mockResolvedValue(1),
    setMinSize: vi.fn().mockResolvedValue(undefined),
    setSize: vi.fn().mockResolvedValue(undefined),
  };
  return { host, window: host as unknown as TauriWindow };
}

const conversationMinimumApplied: DesktopWindowMinimumSyncState = {
  appliedMinimum: { width: 480, height: 680 },
  pending: false,
};

describe('desktop page window minimum', () => {
  it('uses the conversation page minimum without shrinking an existing window', () => {
    expect(planDesktopWindowMinimum({
      currentWidth: 600,
      currentHeight: 720,
      layout: FALLBACK_WORKSPACE_LAYOUT,
      profile: FALLBACK_WORKSPACE_LAYOUT.conversation,
    })).toEqual({
      minimum: { width: 480, height: 680 },
      resizeTo: null,
    });
  });

  it('expands a narrow conversation window when navigating to a workflow page', () => {
    expect(planDesktopWindowMinimum({
      currentWidth: 500,
      currentHeight: 700,
      layout: FALLBACK_WORKSPACE_LAYOUT,
      profile: FALLBACK_WORKSPACE_LAYOUT.workflowCanvas,
    })).toEqual({
      minimum: { width: 640, height: 680 },
      resizeTo: { width: 640, height: 700 },
    });
  });

  it('keeps application chrome above the configured shell minimum', () => {
    expect(planDesktopWindowMinimum({
      currentWidth: 420,
      currentHeight: 620,
      layout: FALLBACK_WORKSPACE_LAYOUT,
      profile: FALLBACK_WORKSPACE_LAYOUT.conversation,
    })).toEqual({
      minimum: { width: 480, height: 680 },
      resizeTo: { width: 480, height: 680 },
    });
  });

  it('does not mutate the host when conversation and settings share the same constraint', async () => {
    const { host, window } = createWindowHost({ maximized: true });

    await expect(syncDesktopWindowMinimum(
      window,
      FALLBACK_WORKSPACE_LAYOUT,
      FALLBACK_WORKSPACE_LAYOUT.settings,
      conversationMinimumApplied,
    )).resolves.toEqual(conversationMinimumApplied);

    expect(host.isMaximized).not.toHaveBeenCalled();
    expect(host.setMinSize).not.toHaveBeenCalled();
    expect(host.setSize).not.toHaveBeenCalled();
  });

  it('defers a changed page constraint while the native window is maximized', async () => {
    const { host, window } = createWindowHost({ maximized: true });

    await expect(syncDesktopWindowMinimum(
      window,
      FALLBACK_WORKSPACE_LAYOUT,
      FALLBACK_WORKSPACE_LAYOUT.workflowCanvas,
      conversationMinimumApplied,
    )).resolves.toEqual({
      appliedMinimum: conversationMinimumApplied.appliedMinimum,
      pending: true,
    });

    expect(host.isMaximized).toHaveBeenCalledOnce();
    expect(host.innerSize).not.toHaveBeenCalled();
    expect(host.setMinSize).not.toHaveBeenCalled();
    expect(host.setSize).not.toHaveBeenCalled();
  });

  it('applies the latest deferred constraint after the window is restored', async () => {
    const { host, window } = createWindowHost({ maximized: false, width: 500, height: 700 });

    await expect(syncDesktopWindowMinimum(
      window,
      FALLBACK_WORKSPACE_LAYOUT,
      FALLBACK_WORKSPACE_LAYOUT.workflowCanvas,
      { ...conversationMinimumApplied, pending: true },
    )).resolves.toEqual({
      appliedMinimum: { width: 640, height: 680 },
      pending: false,
    });

    expect(host.setMinSize).toHaveBeenCalledOnce();
    expect(host.setMinSize.mock.calls[0]?.[0]).toMatchObject({ width: 640, height: 680 });
    expect(host.setSize).toHaveBeenCalledOnce();
    expect(host.setSize.mock.calls[0]?.[0]).toMatchObject({ width: 640, height: 700 });
  });
});
