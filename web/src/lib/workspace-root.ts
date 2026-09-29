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
