import type { AppBootstrapVm, DesktopUiMode } from '@/types';

export function shouldAutoOpenWorkspacePicker(bootstrap: AppBootstrapVm, uiMode: DesktopUiMode) {
  return uiMode === 'workbench' && bootstrap.needsWorkspace;
}

export function shouldRenderWorkspacePicker(uiMode: DesktopUiMode, workspacePickerOpen: boolean) {
  return uiMode === 'workbench' && workspacePickerOpen;
}

export function canRemoveRecentWorkspace(
  recentWorkspaceCount: number,
  workspace: string,
  currentWorkspace?: string | null,
) {
  return recentWorkspaceCount > 1 && workspace !== currentWorkspace;
}
