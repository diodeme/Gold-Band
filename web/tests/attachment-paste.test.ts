/** @vitest-environment jsdom */

import { describe, expect, it } from 'vitest';
import { createLongPasteAttachmentItem } from '@/lib/attachment-service';

const inlineContentMaxBytes = 64_000;

describe('long text paste attachment contract', () => {
  it('uses the configured UTF-8 byte threshold instead of JavaScript characters', () => {
    expect(createLongPasteAttachmentItem('x'.repeat(64_000), inlineContentMaxBytes)).toBeNull();
    expect(createLongPasteAttachmentItem('你'.repeat(21_333), inlineContentMaxBytes)).toBeNull();
    expect(createLongPasteAttachmentItem('你'.repeat(21_334), inlineContentMaxBytes)).not.toBeNull();
  });

  it('keeps the exact oversized paste in one generated text attachment', () => {
    const content = `  ${'x'.repeat(63_999)}`;
    const attachment = createLongPasteAttachmentItem(content, inlineContentMaxBytes);

    expect(attachment).toMatchObject({
      size: new TextEncoder().encode(content).byteLength,
      mime: 'text/plain;charset=utf-8',
      source: 'generated',
    });
    expect(attachment?.name).toMatch(/^pasted-text-\d+-[a-z0-9]+\.txt$/);
    expect(attachment?.file).toBeInstanceOf(File);
  });
});
