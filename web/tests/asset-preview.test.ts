import { describe, expect, it } from 'vitest';
import { imageSrcFromContent, isImageMessageAttachment } from '../src/lib/asset-preview';

describe('asset preview helpers', () => {
  it('returns image data URLs for image ContentVm values', () => {
    const src = 'data:image/png;base64,AAAA';

    expect(imageSrcFromContent({
      title: 'image.png',
      kind: 'input-attachment',
      content: src,
      metadata: { mimeType: 'image/png', isImage: true },
    })).toBe(src);
  });

  it('does not treat text or svg values as image previews', () => {
    expect(imageSrcFromContent({
      title: 'notes.txt',
      kind: 'input-attachment',
      content: 'hello',
      metadata: { mimeType: 'text/plain' },
    })).toBeNull();

    expect(isImageMessageAttachment({
      name: 'icon.svg',
      path: 'task-inputs/icon.svg',
      type: 'image/svg+xml',
      size: 42,
    })).toBe(false);
  });
});
