function normalizePathSeparators(path: string) {
  return path.replaceAll('\\', '/');
}

function stripTrailingSlash(path: string) {
  return path.endsWith('/') ? path.slice(0, -1) : path;
}

export interface SkillStorageHintInput {
  source: string;
  editing: boolean;
  directoryPath?: string | null;
  workspacePath?: string | null;
  translate?: (key: string, params?: Record<string, string>) => string;
}

const defaultTranslate = (key: string, params?: Record<string, string>) => {
  const path = params?.path ?? '';
  switch (key) {
    case 'contextManagement.skills.storageGlobal':
      return `Available across every project. Saved to ${path}`;
    case 'contextManagement.skills.storageProject':
      return `Project-level. Saved to ${path}`;
    default:
      return path;
  }
};

export function skillStorageHint({
  source,
  editing,
  directoryPath,
  workspacePath,
  translate = defaultTranslate,
}: SkillStorageHintInput) {
  if (!editing || !directoryPath) {
    const path = source === 'global'
      ? '~/.gold-band/skills/<name>/SKILL.md'
      : '<project>/.gold-band/skills/<name>/SKILL.md';
    return translate(
      source === 'global' ? 'contextManagement.skills.storageGlobal' : 'contextManagement.skills.storageProject',
      { path },
    );
  }

  const normalizedDirectory = normalizePathSeparators(directoryPath);
  const skillFilePath = `${stripTrailingSlash(normalizedDirectory)}/SKILL.md`;

  if (source === 'project' && workspacePath) {
    const normalizedWorkspace = stripTrailingSlash(normalizePathSeparators(workspacePath));
    if (
      normalizedDirectory === normalizedWorkspace
      || normalizedDirectory.startsWith(`${normalizedWorkspace}/`)
    ) {
      return translate('contextManagement.skills.storageProject', {
        path: `<project>${skillFilePath.slice(normalizedWorkspace.length)}`,
      });
    }
  }

  return translate(
    source === 'global' ? 'contextManagement.skills.storageGlobal' : 'contextManagement.skills.storageProject',
    { path: skillFilePath },
  );
}
