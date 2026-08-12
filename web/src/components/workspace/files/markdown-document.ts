const MARKDOWN_DOCUMENT_EXTENSION = /\.(?:md|markdown)$/iu;

export function isMarkdownDocumentPath(path: string) {
  return MARKDOWN_DOCUMENT_EXTENSION.test(path.trim());
}
