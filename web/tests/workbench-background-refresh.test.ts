import { describe, expect, it } from 'vitest';
import {
  shouldRunWorkbenchBackgroundRefresh,
  WORKBENCH_BACKGROUND_REFRESH_HIDDEN_INTERVAL_MS,
  WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS,
} from '@/lib/workbench-background-refresh';

describe('workbench background refresh policy', () => {
  it('runs only when workbench has bootstrap and page data', () => {
    expect(shouldRunWorkbenchBackgroundRefresh({
      uiMode: 'workbench',
      bootstrapReady: true,
      hasPageData: true,
    })).toBe(true);
  });

  it('does not run in conversation mode even when legacy page data exists', () => {
    expect(shouldRunWorkbenchBackgroundRefresh({
      uiMode: 'conversation',
      bootstrapReady: true,
      hasPageData: true,
    })).toBe(false);
  });

  it('does not run before bootstrap or page data is ready', () => {
    expect(shouldRunWorkbenchBackgroundRefresh({
      uiMode: 'workbench',
      bootstrapReady: false,
      hasPageData: true,
    })).toBe(false);
    expect(shouldRunWorkbenchBackgroundRefresh({
      uiMode: 'workbench',
      bootstrapReady: true,
      hasPageData: false,
    })).toBe(false);
  });

  it('keeps the existing visible and hidden refresh cadence', () => {
    expect(WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS).toBe(10_000);
    expect(WORKBENCH_BACKGROUND_REFRESH_HIDDEN_INTERVAL_MS).toBe(30_000);
  });
});
