export const MAX_COMPOSER_QUOTE_CHARS = 12_000;
export const MAX_COMPOSER_QUOTES = 64;
export const MAX_COMPOSER_QUOTE_ID_LENGTH = 128;
export const MAX_COMPOSER_QUOTE_SOURCE_KEY_LENGTH = 512;

export interface ComposerQuote {
  id: string;
  sourceKey: string;
  text: string;
}

import type { ConversationPromptInput, UserPromptQuote } from '@/types';

export type AddComposerQuoteResult =
  | { ok: true; quotes: ComposerQuote[] }
  | { ok: false; code: 'composer.quote.limit-exceeded'; maxChars: number }
  | { ok: false; code: 'composer.quote.count-exceeded'; maxQuotes: number }
  | { ok: false; code: 'composer.quote.duplicate' };

export function composerQuoteChars(quotes: readonly ComposerQuote[]) {
  return quotes.reduce((total, quote) => total + quote.text.length, 0);
}

export function addComposerQuote(
  quotes: readonly ComposerQuote[],
  quote: ComposerQuote,
  maxChars = MAX_COMPOSER_QUOTE_CHARS,
): AddComposerQuoteResult {
  const text = quote.text.trim();
  if (quotes.length >= MAX_COMPOSER_QUOTES) {
    return { ok: false, code: 'composer.quote.count-exceeded', maxQuotes: MAX_COMPOSER_QUOTES };
  }
  if (quotes.some((item) => item.sourceKey === quote.sourceKey && item.text === text)) {
    return { ok: false, code: 'composer.quote.duplicate' };
  }
  if (composerQuoteChars(quotes) + text.length > maxChars) {
    return { ok: false, code: 'composer.quote.limit-exceeded', maxChars };
  }
  return { ok: true, quotes: [...quotes, { ...quote, text }] };
}

export function createUserPromptSubmission(
  content: string,
  quotes: readonly ComposerQuote[],
): ConversationPromptInput {
  const displayText = content.trim();
  const promptQuotes = quotes.map(({ id, sourceKey, text }) => ({
    id,
    sourceMessageKey: sourceKey,
    text,
  }));
  return { displayText, quotes: promptQuotes };
}

export function hasUserPromptPayload(content: string, attachmentCount: number) {
  return content.trim().length > 0 || attachmentCount > 0;
}

export function serializeUserPromptSubmission(input: ConversationPromptInput) {
  const displayText = input.displayText.trim();
  if (input.quotes.length === 0) return displayText;
  const quoteBlocks = input.quotes.map((quote) =>
    quote.text
      .split('\n')
      .map((line) => `> ${line}`)
      .join('\n'),
  );
  return `${quoteBlocks.join('\n\n')}\n\n${displayText}`;
}

export function userPromptQuotesFromRaw(raw: unknown): UserPromptQuote[] {
  if (!raw || typeof raw !== 'object') return [];
  const quotes = (raw as { quotes?: unknown }).quotes;
  if (!Array.isArray(quotes)) return [];
  return quotes.flatMap((quote) => {
    if (!quote || typeof quote !== 'object') return [];
    const { id, sourceMessageKey, text } = quote as Record<string, unknown>;
    return typeof id === 'string'
      && id.length > 0
      && typeof sourceMessageKey === 'string'
      && sourceMessageKey.length > 0
      && typeof text === 'string'
      && text.length > 0
      ? [{ id, sourceMessageKey, text }]
      : [];
  });
}
