import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', () => ({
  copyImageToClipboard: vi.fn(() => Promise.resolve()),
  saveImageAs: vi.fn(() => Promise.resolve(true)),
}));

import { copyImageToClipboard, saveImageAs } from '@/api';
import {
  copyImageAsset,
  imageActionInput,
  saveImageAssetAs,
} from '@/lib/image-actions';

describe('image asset actions', () => {
  beforeEach(() => {
    vi.mocked(copyImageToClipboard).mockClear();
    vi.mocked(saveImageAs).mockClear();
  });

  it('keeps desktop path sources lightweight instead of serializing image bytes', async () => {
    const attachment = {
      id: 'path-image', name: 'shot.png', size: 10, mime: 'image/png',
      path: 'D:/images/shot.png', previewUrl: 'asset://shot', source: 'dialog' as const,
    };

    const input = await imageActionInput(attachment);
    await copyImageAsset(attachment);

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
    await saveImageAssetAs(attachment);

    expect(saveImageAs).toHaveBeenCalledWith({
      source: { kind: 'bytes', dataBase64: 'AQIDBA==' },
      fileName: 'paste.png',
      mime: 'image/png',
    });
  });

  it('rejects an unavailable source with a stable structured error code', async () => {
    await expect(imageActionInput({
      name: 'missing.png', mime: 'image/png',
    })).rejects.toMatchObject({ code: 'image-action.source-unreadable', params: {} });
  });

  it('loads original bytes and actual MIME on demand instead of copying the thumbnail', async () => {
    const loadOriginal = vi.fn(async () => new Blob([Uint8Array.from([1, 2, 3])], { type: 'image/jpeg' }));
    const asset = { name: 'Image 1', mime: 'image/png', previewUrl: 'blob:thumbnail', loadOriginal };
    expect(loadOriginal).not.toHaveBeenCalled();
    expect(await imageActionInput(asset)).toEqual({ fileName: 'Image 1', mime: 'image/jpeg', source: { kind: 'bytes', dataBase64: 'AQID' } });
    expect(loadOriginal).toHaveBeenCalledTimes(1);
  });
});
