import { describe, expect, it } from 'vitest';

import i18n from '../src/i18n';
import { composerQuoteFailureMessage } from '@/lib/composer-quote-i18n';

describe('composer quote failure message', () => {
  const t = i18n.getFixedT('zh-CN');

  it('reports the size, the limit and what the message can still quote', () => {
    expect(composerQuoteFailureMessage(t, {
      ok: false, code: 'composer.quote.limit-exceeded', chars: 13_000, maxChars: 12_000, remainingChars: 500,
    })).toBe('该引用为 13,000 个字符，超出单条消息 12,000 个字符的引用上限，本条消息还可引用 500 个字符。');
    const diff = composerQuoteFailureMessage(t, {
      ok: false, code: 'composer.quote.diff-limit-exceeded', bytes: 90_000, maxBytes: 64_000, remainingBytes: 10_000,
    });
    expect(diff).toContain('超出单条消息');
    expect(diff).toContain('可选中部分内容引用');
    expect(diff).not.toMatch(/\{\{|undefined/u);
  });
});
