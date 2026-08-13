import { describe, expect, it } from 'vitest';
import {
  MAX_COMPOSER_QUOTE_CHARS,
  MAX_COMPOSER_QUOTES,
  addComposerQuote,
  createUserPromptSubmission,
  serializeUserPromptSubmission,
  userPromptQuotesFromRaw,
  type ComposerQuote,
} from '@/lib/composer-context';

const quote = (id: string, text: string, sourceKey = id): ComposerQuote => ({ id, text, sourceKey });

describe('composer quote contract', () => {
  it('keeps display text and structured quotes separate from the agent prompt', () => {
    const first = addComposerQuote([], quote('one', '第一行\n第二行'));
    expect(first.ok).toBe(true);
    if (!first.ok) return;
    const second = addComposerQuote(first.quotes, quote('two', '另一段'));
    expect(second.ok).toBe(true);
    if (!second.ok) return;
    const submission = createUserPromptSubmission('继续解释', second.quotes);
    expect(submission).toEqual({
      displayText: '继续解释',
      quotes: [
        { id: 'one', sourceMessageKey: 'one', text: '第一行\n第二行' },
        { id: 'two', sourceMessageKey: 'two', text: '另一段' },
      ],
    });
    expect(serializeUserPromptSubmission(submission)).toBe('> 第一行\n> 第二行\n\n> 另一段\n\n继续解释');
  });

  it('does not infer quotes from user-authored Markdown blockquotes', () => {
    const submission = createUserPromptSubmission('> 这是用户自己输入的引用格式', []);
    expect(submission.displayText).toBe('> 这是用户自己输入的引用格式');
    expect(submission.quotes).toEqual([]);
    expect(userPromptQuotesFromRaw({ source: 'goldBandPrompt' })).toEqual([]);
  });

  it('only reads valid explicit quote metadata', () => {
    expect(userPromptQuotesFromRaw({
      quotes: [
        { id: 'one', sourceMessageKey: 'message-1', text: '引用内容' },
        { id: '', sourceMessageKey: 'message-2', text: 'invalid' },
      ],
    })).toEqual([{ id: 'one', sourceMessageKey: 'message-1', text: '引用内容' }]);
  });

  it('rejects the same selection from the same source message', () => {
    const existing = [quote('one', '相同内容', 'message-1')];
    expect(addComposerQuote(existing, quote('two', '相同内容', 'message-1'))).toMatchObject({
      ok: false,
      code: 'composer.quote.duplicate',
    });
  });

  it('keeps the existing draft unchanged when the total character limit would be exceeded', () => {
    const existing = [quote('one', 'a'.repeat(MAX_COMPOSER_QUOTE_CHARS))];
    expect(addComposerQuote(existing, quote('two', 'b'))).toEqual({
      ok: false,
      code: 'composer.quote.limit-exceeded',
      maxChars: MAX_COMPOSER_QUOTE_CHARS,
    });
    expect(existing).toHaveLength(1);
  });

  it('keeps the quote list bounded independently of the character budget', () => {
    const existing = Array.from({ length: MAX_COMPOSER_QUOTES }, (_, index) =>
      quote(`quote-${index}`, 'x', `message-${index}`),
    );

    expect(addComposerQuote(existing, quote('overflow', 'x', 'overflow'))).toEqual({
      ok: false,
      code: 'composer.quote.count-exceeded',
      maxQuotes: MAX_COMPOSER_QUOTES,
    });
  });
});
