import { describe, expect, it } from 'vitest';

import { FALLBACK_WORKSPACE_LAYOUT } from '@/components/workspace/workspace-layout';
import { planDesktopWindowMinimum } from '@/lib/desktop-window-layout';

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
});
