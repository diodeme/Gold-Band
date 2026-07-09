import { describe, expect, it } from 'vitest';
import {
  imageSrcFromContent,
  isImageMessageAttachment,
  isTaskInputMessageAttachment,
} from '../src/lib/asset-preview';

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

  it('detects task input attachments separately from attempt user inputs', () => {
    expect(isTaskInputMessageAttachment({
      name: 'requirement.txt',
      path: 'task-inputs/requirement.txt',
      type: 'text/plain',
      size: 12,
    })).toBe(true);

    expect(isTaskInputMessageAttachment({
      name: 'image.png',
      path: 'user-inputs/image.png',
      type: 'image/png',
      size: 12,
    })).toBe(false);
  });
});
