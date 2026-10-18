import type { TFunction } from 'i18next';

import { formatSize } from '@/lib/attachment-service';
import { isWholeFileDiffQuote, type ComposerQuoteFailure } from '@/lib/composer-context';
import type { UserPromptQuote, UserPromptQuoteSource } from '@/types';

/** Composer error text for a quote that could not be added, including what budget remains. */
export function composerQuoteFailureMessage(t: TFunction, failure: ComposerQuoteFailure) {
  switch (failure.code) {
    case 'composer.quote.limit-exceeded':
      return t('acp.quoteLimitExceeded', {
        size: failure.chars.toLocaleString(),
        max: failure.maxChars.toLocaleString(),
        remaining: Math.max(0, failure.remainingChars).toLocaleString(),
      });
    case 'composer.quote.diff-limit-exceeded':
      return t('acp.quoteDiffLimitExceeded', {
        size: formatSize(failure.bytes),
        max: formatSize(failure.maxBytes),
        remaining: formatSize(Math.max(0, failure.remainingBytes)),
      });
    case 'composer.quote.count-exceeded':
      return t('acp.quoteCountExceeded', { max: failure.maxQuotes });
    case 'composer.quote.duplicate':
      return t('acp.quoteDuplicate');
  }
}

function fileName(path: string) {
  return path.split(/[\\/]/).at(-1) || path;
}

function lineRange(startLine: number, endLine: number) {
  return startLine === endLine ? `${startLine}` : `${startLine}-${endLine}`;
}

/** Short chip label naming where a quote came from. */
export function quoteLabel(t: TFunction, quote: Pick<UserPromptQuote, 'source'>, index: number) {
  const { source } = quote;
  switch (source.kind) {
    case 'agentMessage':
      return t('acp.quoteLabel', { index: index + 1 });
    case 'file':
      return `${fileName(source.label)}:${lineRange(source.startLine, source.endLine)}`;
    case 'diff':
      return t(isWholeFileDiffQuote(quote) ? 'acp.quoteDiffFileLabel' : 'acp.quoteDiffLabel', { name: fileName(source.path) });
  }
}

/** Full origin line for a file or diff quote; agent message quotes need none. */
export function quoteSourceDetail(t: TFunction, source: UserPromptQuoteSource) {
  switch (source.kind) {
    case 'agentMessage':
      return null;
    case 'file':
      return t('acp.quoteFileSource', { path: source.label, lines: lineRange(source.startLine, source.endLine) });
    case 'diff': {
      const path = source.scope === 'file' ? t('acp.quoteDiffFileLabel', { name: source.path }) : source.path;
      return `${path} · ${t(`acp.quoteDiffOrigin.${source.origin}`, { revision: source.revision ?? '' })}`;
    }
  }
}
