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

export interface LocalFileLinkTarget {
  line: number;
  column: number | null;
  endLine: number | null;
  displayText: string;
  sourceSuffix: string;
}

export function parseLocalFileLinkTarget(href: string): LocalFileLinkTarget | null {
  const value = href.trim();
  const fragment = value.match(/#L(\d+)(?:-L?(\d+))?$/iu);
  if (fragment) {
    const line = Number(fragment[1]);
    const endLine = fragment[2] ? Number(fragment[2]) : null;
    return {
      line,
      column: null,
      endLine,
      displayText: endLine == null ? `:${line}` : `:${line}-${endLine}`,
      sourceSuffix: fragment[0],
    };
  }

  const suffix = value.match(/:(\d+)(?::(\d+))?$/u);
  if (!suffix) return null;
  const line = Number(suffix[1]);
  const column = suffix[2] ? Number(suffix[2]) : null;
  return {
    line,
    column,
    endLine: null,
    displayText: column == null ? `:${line}` : `:${line}:${column}`,
    sourceSuffix: suffix[0],
  };
}

export function isExternalUrlHref(href: string) {
  return /^(?:https?:\/\/|mailto:|tel:)/iu.test(href.trim());
}

export function isDocumentAnchorHref(href: string) {
  return href.trim().startsWith('#');
}
