import { parseMarkdownIntoBlocks } from 'streamdown';
import { describe, expect, it, vi } from 'vitest';
import { createIncrementalMarkdownBlockParser } from '@/lib/incremental-markdown-blocks';

describe('incremental Markdown block parser', () => {
  it('re-lexes only the mutable tail of append-only Markdown', () => {
    const parseBlocks = vi.fn(parseMarkdownIntoBlocks);
    const parseIncrementally = createIncrementalMarkdownBlockParser(parseBlocks);

    const first = parseIncrementally('第一段\n\n第二段');
    expect(first.join('')).toBe('第一段\n\n第二段');
    expect(parseBlocks).toHaveBeenLastCalledWith('第一段\n\n第二段');

    const second = parseIncrementally('第一段\n\n第二段继续');
    expect(second.join('')).toBe('第一段\n\n第二段继续');
    expect(parseBlocks).toHaveBeenLastCalledWith('第二段继续');

    const third = parseIncrementally('第一段\n\n第二段继续\n\n第三段');
    expect(third.join('')).toBe('第一段\n\n第二段继续\n\n第三段');
    expect(parseBlocks).toHaveBeenLastCalledWith('第二段继续\n\n第三段');

    expect(parseIncrementally('第一段\n\n第二段继续\n\n第三段')).toBe(third);
    expect(parseBlocks).toHaveBeenCalledTimes(3);
  });

  it('falls back to a full parse when the stream rewrites prior content', () => {
    const parseBlocks = vi.fn(parseMarkdownIntoBlocks);
    const parseIncrementally = createIncrementalMarkdownBlockParser(parseBlocks);

    parseIncrementally('旧内容\n\n尾部');
    const rewritten = parseIncrementally('新内容\n\n尾部');

    expect(rewritten.join('')).toBe('新内容\n\n尾部');
    expect(parseBlocks).toHaveBeenLastCalledWith('新内容\n\n尾部');
  });

});
