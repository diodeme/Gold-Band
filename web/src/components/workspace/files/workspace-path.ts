/** Slash-normalized path key; Windows drive paths compare case-insensitively like the backend. */
export function workspacePathKey(path: string) {
  const normalized = path.replaceAll('\\', '/').replace(/\/+$/u, '');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

/** Whether `path` is `ancestor` itself or lies below it. */
export function workspacePathIsWithin(path: string, ancestor: string) {
  const child = workspacePathKey(path);
  const parent = workspacePathKey(ancestor);
  return child === parent || child.startsWith(`${parent}/`);
}

/** Map a path under `from` to the same position under `to`, keeping the original suffix. */
export function remapWorkspacePath(path: string, from: string, to: string) {
  if (!workspacePathIsWithin(path, from)) return path;
  const suffix = path.replaceAll('\\', '/').slice(from.replace(/[\\/]+$/u, '').length);
  const separator = to.includes('\\') ? '\\' : '/';
  return `${to}${separator === '\\' ? suffix.replaceAll('/', '\\') : suffix}`;
}
