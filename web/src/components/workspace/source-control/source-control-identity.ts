export function normalizeSourceControlWorkspacePath(workspacePath: string | null | undefined) {
  if (!workspacePath) return null;
  const normalized = workspacePath.replaceAll('\\', '/').replace(/\/+$/u, '');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

export function sourceControlWorkspaceSessionKey(
  projectId: string,
  workspacePath: string | null | undefined,
) {
  return `${projectId}\u0000${normalizeSourceControlWorkspacePath(workspacePath) ?? '__main__'}`;
}

export function sameSourceControlWorkspacePath(
  left: string | null | undefined,
  right: string | null | undefined,
) {
  return normalizeSourceControlWorkspacePath(left) === normalizeSourceControlWorkspacePath(right);
}
