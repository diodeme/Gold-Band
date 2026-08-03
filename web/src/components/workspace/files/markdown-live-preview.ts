import type { Extension } from '@codemirror/state';

export async function loadMarkdownLivePreviewExtensions(
  onLinkClick: (href: string) => void,
  enableTables: boolean,
): Promise<Extension[]> {
  const [{ markdown, markdownLanguage }, atomic] = await Promise.all([
    import('@codemirror/lang-markdown'),
    import('@atomic-editor/editor'),
  ]);
  return [
    markdown({
      base: markdownLanguage,
      extensions: atomic.highlightMarkdown,
    }),
    atomic.atomicMarkdownSyntax,
    ...(enableTables ? [atomic.tables({ onLinkClick })] : []),
    atomic.inlinePreview({ onLinkClick }),
  ];
}
