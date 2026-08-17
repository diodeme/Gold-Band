/** @vitest-environment jsdom */

import { describe, expect, it } from 'vitest';
import {
  LONG_PASTE_ATTACHMENT_THRESHOLD_CHARS,
  createLongPasteAttachmentItem,
} from '@/lib/attachment-service';

describe('long text paste attachment contract', () => {
  it('uses the paste-only 6,400 character threshold', () => {
    expect(LONG_PASTE_ATTACHMENT_THRESHOLD_CHARS).toBe(6_400);
    expect(createLongPasteAttachmentItem('x'.repeat(6_400))).toBeNull();
  });

  it('keeps the exact oversized paste in one generated text attachment', () => {
    const content = `  ${'x'.repeat(6_399)}`;
    const attachment = createLongPasteAttachmentItem(content);

    expect(attachment).toMatchObject({
      size: content.length,
      mime: 'text/plain;charset=utf-8',
      source: 'generated',
    });
    expect(attachment?.name).toMatch(/^pasted-text-\d+-[a-z0-9]+\.txt$/);
    expect(attachment?.file).toBeInstanceOf(File);
  });
});
