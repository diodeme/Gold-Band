import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', () => ({
  copyImageToClipboard: vi.fn(() => Promise.resolve()),
  saveImageAs: vi.fn(() => Promise.resolve(true)),
}));

import { copyImageToClipboard, saveImageAs } from '@/api';
import {
  attachmentImageActionInput,
  copyAttachmentImage,
  saveAttachmentImageAs,
} from '@/lib/image-actions';

describe('attachment image actions', () => {
  beforeEach(() => {
    vi.mocked(copyImageToClipboard).mockClear();
    vi.mocked(saveImageAs).mockClear();
  });

  it('keeps desktop path sources lightweight instead of serializing image bytes', async () => {
    const attachment = {
      id: 'path-image', name: 'shot.png', size: 10, mime: 'image/png',
      path: 'D:/images/shot.png', previewUrl: 'asset://shot', source: 'dialog' as const,
    };

    const input = await attachmentImageActionInput(attachment);
    await copyAttachmentImage(attachment);

    expect(input).toEqual({
      source: { kind: 'path', path: 'D:/images/shot.png' },
      fileName: 'shot.png',
      mime: 'image/png',
    });
    expect(copyImageToClipboard).toHaveBeenCalledWith(input);
  });

  it('serializes a pasted in-memory image only when the user selects an action', async () => {
    const file = new File([Uint8Array.from([1, 2, 3, 4])], 'paste.png', { type: 'image/png' });
    const attachment = {
      id: 'memory-image', name: 'paste.png', size: file.size, mime: file.type,
      file, previewUrl: 'blob:paste', source: 'paste' as const,
    };

    expect(copyImageToClipboard).not.toHaveBeenCalled();
    await saveAttachmentImageAs(attachment);

    expect(saveImageAs).toHaveBeenCalledWith({
      source: { kind: 'bytes', dataBase64: 'AQIDBA==' },
      fileName: 'paste.png',
      mime: 'image/png',
    });
  });

  it('rejects an unavailable source with a stable structured error code', async () => {
    await expect(attachmentImageActionInput({
      id: 'missing', name: 'missing.png', size: 4, mime: 'image/png', source: 'paste',
    })).rejects.toMatchObject({ code: 'image-action.source-unreadable', params: {} });
  });
});
