import type { SessionWorkLocationVm, WorkspaceRootRef } from '@/types';

/**
 * File root shown by a workspace tab (files, source control). It follows the
 * current session's work location; `unavailable` is never collapsed into the
 * project root, so a reclaimed worktree cannot silently show main files.
 */
export type WorkspaceTabRoot =
  | { kind: 'available'; workspacePath: string | null }
  | { kind: 'unavailable'; reason: 'released' | 'unresolved'; workspacePath: string | null };

export const MAIN_WORKSPACE_TAB_ROOT: WorkspaceTabRoot = { kind: 'available', workspacePath: null };

/** Location of a session whose canonical projection is not loaded yet. */
export const UNRESOLVED_WORK_LOCATION: SessionWorkLocationVm = { kind: 'unavailable', reason: 'unresolved' };

export function normalizeWorkspacePath(workspacePath: string | null | undefined) {
  if (!workspacePath) return null;
  let normalized = workspacePath.replaceAll('\\', '/');
  if (/^\/\/\?\/unc\//iu.test(normalized)) normalized = `//${normalized.slice(8)}`;
  else if (/^\/\/\?\/[a-z]:\//iu.test(normalized)) normalized = normalized.slice(4);
  normalized = normalized.replace(/\/+$/u, '');
  return /^[a-z]:\//iu.test(normalized) || /^\/\/[^/]+\/[^/]+/u.test(normalized)
    ? normalized.toLowerCase()
    : normalized;
}

export function sameWorkspacePath(left: string | null | undefined, right: string | null | undefined) {
  return normalizeWorkspacePath(left) === normalizeWorkspacePath(right);
}

/** Stable identity of a file root for stores, watches and event routing. */
export function workspaceRootKey(projectId: string, workspacePath: string | null | undefined) {
  return `${projectId}\u0000${normalizeWorkspacePath(workspacePath) ?? '__main__'}`;
}

export function workspaceRootRef(projectId: string, workspacePath: string | null | undefined): WorkspaceRootRef {
  return { projectId, workspacePath: workspacePath ?? null };
}

export function workspaceTabRootForLocation(location: SessionWorkLocationVm | null | undefined): WorkspaceTabRoot {
  if (!location || location.kind === 'main') return MAIN_WORKSPACE_TAB_ROOT;
  if (location.kind === 'worktree') return { kind: 'available', workspacePath: location.path };
  return { kind: 'unavailable', reason: location.reason, workspacePath: location.path ?? null };
}

/**
 * Root a tab resolves to. `browseMain` is the user's explicit choice to view
 * the project root while the session's worktree is unavailable.
 */
export function effectiveWorkspaceTabRoot(sessionRoot: WorkspaceTabRoot, browseMain: boolean): WorkspaceTabRoot {
  return sessionRoot.kind === 'unavailable' && browseMain ? MAIN_WORKSPACE_TAB_ROOT : sessionRoot;
}

export function sameWorkspaceTabRoot(left: WorkspaceTabRoot, right: WorkspaceTabRoot) {
  if (left.kind !== right.kind || !sameWorkspacePath(left.workspacePath, right.workspacePath)) return false;
  return left.kind === 'available' || (right.kind === 'unavailable' && left.reason === right.reason);
}

export function worktreePathOfLocation(location: SessionWorkLocationVm | null | undefined) {
  return location?.kind === 'worktree' ? location.path : null;
}

export function worktreeBranchOfLocation(location: SessionWorkLocationVm | null | undefined) {
  return location?.kind === 'worktree' ? location.branch ?? null : null;
}
