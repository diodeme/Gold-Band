export const GIT_DOWNLOAD_URL = 'https://git-scm.com/downloads';

export function isGitVersionCapabilityError(code?: string | null) {
  return code === 'git.version-unsupported' || code === 'git.version-unavailable';
}
