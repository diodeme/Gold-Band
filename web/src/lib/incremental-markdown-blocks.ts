import { parseMarkdownIntoBlocks } from 'streamdown';

export type MarkdownBlockParser = (markdown: string) => string[];

/**
 * Keeps completed Streamdown blocks stable while an append-only message grows.
 * Only the previous mutable tail is sent back through the block lexer.
 */
export function createIncrementalMarkdownBlockParser(
  parseBlocks: MarkdownBlockParser = parseMarkdownIntoBlocks,
): MarkdownBlockParser {
  let previousMarkdown = '';
  let previousBlocks: string[] = [];

  return (markdown) => {
    if (markdown === previousMarkdown) return previousBlocks;

    if (!markdown.startsWith(previousMarkdown) || previousBlocks.length === 0) {
      previousMarkdown = markdown;
      previousBlocks = parseBlocks(markdown);
      return previousBlocks;
    }

    const stableBlocks = previousBlocks.slice(0, -1);
    const stableLength = stableBlocks.reduce((length, block) => length + block.length, 0);
    const mutableTail = markdown.slice(stableLength);
    const nextBlocks = [...stableBlocks, ...parseBlocks(mutableTail)];
    previousMarkdown = markdown;
    previousBlocks = nextBlocks;
    return nextBlocks;
  };
}
