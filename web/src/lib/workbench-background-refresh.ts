import type { DesktopUiMode } from "@/types";

export const WORKBENCH_BACKGROUND_REFRESH_INTERVAL_MS = 10_000;
export const WORKBENCH_BACKGROUND_REFRESH_HIDDEN_INTERVAL_MS = 30_000;

export function shouldRunWorkbenchBackgroundRefresh(input: {
  uiMode: DesktopUiMode;
  bootstrapReady: boolean;
  hasPageData: boolean;
}) {
  return input.uiMode === "workbench" && input.bootstrapReady && input.hasPageData;
}
