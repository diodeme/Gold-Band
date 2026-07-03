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
}

export function skillStorageHint({
  source,
  editing,
  directoryPath,
  workspacePath,
}: SkillStorageHintInput) {
  if (!editing || !directoryPath) {
    return source === 'global'
      ? 'Available across every project. Saved to ~/.gold-band/skills/<name>/SKILL.md'
      : 'Project-level. Saved to <project>/.gold-band/skills/<name>/SKILL.md';
  }

  const normalizedDirectory = normalizePathSeparators(directoryPath);
  const skillFilePath = `${stripTrailingSlash(normalizedDirectory)}/SKILL.md`;

  if (source === 'project' && workspacePath) {
    const normalizedWorkspace = stripTrailingSlash(normalizePathSeparators(workspacePath));
    if (
      normalizedDirectory === normalizedWorkspace
      || normalizedDirectory.startsWith(`${normalizedWorkspace}/`)
    ) {
      return `Project-level. Saved to <project>${skillFilePath.slice(normalizedWorkspace.length)}`;
    }
  }

  return source === 'global'
    ? `Available across every project. Saved to ${skillFilePath}`
    : `Project-level. Saved to ${skillFilePath}`;
}
