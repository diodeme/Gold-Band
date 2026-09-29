import {
  normalizeWorkspacePath,
  sameWorkspacePath,
  workspaceRootKey,
} from '@/lib/workspace-root';

export const normalizeSourceControlWorkspacePath = normalizeWorkspacePath;
export const sourceControlWorkspaceSessionKey = workspaceRootKey;
export const sameSourceControlWorkspacePath = sameWorkspacePath;
