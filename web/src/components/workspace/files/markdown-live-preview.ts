import type { Extension } from '@codemirror/state';

const TABLE_DELIMITER_PATTERN = /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)+\|?\s*$/u;
const MARKDOWN_IMAGE_PATTERN = /!\[[^\]]*\]\(/u;

export function markdownHasTableImages(markdown: string) {
  const lines = markdown.split(/\r?\n/u);
  for (let index = 1; index < lines.length; index += 1) {
    if (!TABLE_DELIMITER_PATTERN.test(lines[index] ?? '')) continue;
    if (MARKDOWN_IMAGE_PATTERN.test(lines[index - 1] ?? '')) return true;
    for (let bodyIndex = index + 1; bodyIndex < lines.length; bodyIndex += 1) {
      const line = lines[bodyIndex] ?? '';
      if (!line.trim() || !line.includes('|')) break;
      if (MARKDOWN_IMAGE_PATTERN.test(line)) return true;
    }
  }
  return false;
}

export async function loadMarkdownLanguageExtension(): Promise<Extension> {
  const [{ markdown, markdownLanguage }, atomic] = await Promise.all([
    import('@codemirror/lang-markdown'),
    import('@atomic-editor/editor'),
  ]);
  return markdown({
    base: markdownLanguage,
    extensions: atomic.highlightMarkdown,
  });
}

export async function loadMarkdownPreviewExtensions(
  onLinkClick: (href: string) => void,
  enableTables: boolean,
): Promise<Extension[]> {
  const atomic = await import('@atomic-editor/editor');
  return [
    atomic.atomicMarkdownSyntax,
    ...(enableTables ? [atomic.tables({ onLinkClick })] : []),
    atomic.inlinePreview({ onLinkClick }),
  ];
}
