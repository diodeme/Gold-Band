export function isLocalFileHref(href: string) {
  const value = href.trim();
  if (!value || value.startsWith('#')) return false;
  if (/^[a-z]:[\\/]/iu.test(value) || /^file:\/\//iu.test(value)) return true;
  const pathWithoutTarget = value
    .replace(/#L\d+(?:-L?\d+)?$/iu, '')
    .replace(/:\d+(?::\d+)?$/u, '');
  if (/^[a-z][a-z\d+.-]*:/iu.test(pathWithoutTarget) || /^\/\//u.test(pathWithoutTarget)) return false;
  return true;
}
