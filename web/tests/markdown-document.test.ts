import { describe, expect, it } from 'vitest';
import { isMarkdownDocumentPath } from '@/components/workspace/files/markdown-document';

describe('Markdown document routing', () => {
  it('recognizes the supported Markdown filename extensions regardless of case', () => {
    expect(isMarkdownDocumentPath('README.md')).toBe(true);
    expect(isMarkdownDocumentPath('Plan.MARKDOWN')).toBe(true);
    expect(isMarkdownDocumentPath('notes.md   ')).toBe(true);
    expect(isMarkdownDocumentPath('notes.txt')).toBe(false);
  });
});
