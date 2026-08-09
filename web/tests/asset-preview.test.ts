import { describe, expect, it } from 'vitest';
import {
  groupMessageAttachmentPreviews,
  imageSrcFromContent,
  isImageMessageAttachment,
  isTaskInputMessageAttachment,
  messageAttachmentPreviewsFromRaw,
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

  it('preserves image and regular file attachments from the same user message', () => {
    expect(messageAttachmentPreviewsFromRaw({
      attachments: [
        { name: 'image.png', path: 'task-inputs/image.png', type: 'image/png', size: 81_401 },
        { name: 'acp.raw.jsonl', path: 'task-inputs/acp.raw.jsonl', type: 'application/json', size: 1_672_643 },
      ],
    })).toEqual([
      { name: 'image.png', path: 'task-inputs/image.png', type: 'image/png', size: 81_401 },
      { name: 'acp.raw.jsonl', path: 'task-inputs/acp.raw.jsonl', type: 'application/json', size: 1_672_643 },
    ]);
  });

  it('groups message images and regular files into independent display rows', () => {
    const image = { name: 'image.png', path: 'task-inputs/image.png', type: 'image/png', size: 81_401 };
    const file = { name: 'acp.raw.jsonl', path: 'task-inputs/acp.raw.jsonl', type: 'application/json', size: 1_672_643 };

    expect(groupMessageAttachmentPreviews([file, image])).toEqual({
      images: [image],
      files: [file],
    });
  });
});
