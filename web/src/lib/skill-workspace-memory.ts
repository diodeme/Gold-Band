export const SKILL_PROJECT_WORKSPACE_STORAGE_KEY = 'gold-band:context-management:skill-project-workspace';

export interface SkillWorkspaceOption {
  workspacePath: string;
}

type SkillWorkspaceStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

function browserStorage(): SkillWorkspaceStorage | null {
  if (typeof window === 'undefined') {
    return null;
  }
  return window.localStorage;
}

export function rememberSkillProjectWorkspace(
  workspacePath: string,
  storage: SkillWorkspaceStorage | null = browserStorage(),
) {
  if (!storage) {
    return;
  }
  try {
    storage.setItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY, workspacePath);
  } catch {
    // Ignore unavailable storage; selection still works for the current render.
  }
}

export function readRememberedSkillProjectWorkspace(
  workspaces: SkillWorkspaceOption[],
  storage: SkillWorkspaceStorage | null = browserStorage(),
) {
  if (!storage) {
    return '';
  }
  try {
    const remembered = storage.getItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY) ?? '';
    if (!remembered) {
      return '';
    }
    if (workspaces.length === 0) {
      return '';
    }
    if (workspaces.some((workspace) => workspace.workspacePath === remembered)) {
      return remembered;
    }
    storage.removeItem(SKILL_PROJECT_WORKSPACE_STORAGE_KEY);
  } catch {
    return '';
  }
  return '';
}
