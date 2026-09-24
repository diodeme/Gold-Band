/** @vitest-environment jsdom */

import { describe, expect, it } from 'vitest';

import { revokeAttachmentPreviewUrls, type AttachmentItem } from '@/lib/attachment-service';
import {
  createDraftAttachmentWorkspaceResource,
  detachDraftAttachmentPreview,
} from '@/components/workspace/right-workspace-context';

describe('draft attachment preview ownership', () => {
  it('keeps the open preview after the composer releases its object URL', async () => {
    const file = new File(['png'], 'image.png', { type: 'image/png' });
    const composer: AttachmentItem = {
      id: 'attachment-1',
      name: 'image.png',
      size: file.size,
      mime: 'image/png',
      file,
      previewUrl: URL.createObjectURL(file),
      source: 'paste',
    };
    const sameName: AttachmentItem = {
      ...composer,
      id: 'attachment-2',
      previewUrl: URL.createObjectURL(file),
    };
    const opened = detachDraftAttachmentPreview(composer);
    const other = detachDraftAttachmentPreview(sameName);
    revokeAttachmentPreviewUrls([composer, sameName]);

    expect(opened.previewUrl).not.toBe(composer.previewUrl);
    expect(other.previewUrl).not.toBe(opened.previewUrl);
    const resource = createDraftAttachmentWorkspaceResource({
      scopeKey: 'conversation:project-1:task-1:run-1',
      projectId: 'project-1',
      attachment: composer,
    });
    expect(resource.key).toContain(composer.id);
    expect(resource.key).not.toBe(createDraftAttachmentWorkspaceResource({
      scopeKey: resource.scopeKey,
      projectId: 'project-1',
      attachment: sameName,
    }).key);
    expect(resource.attachment.previewUrl?.startsWith('blob:')).toBe(true);
    const openedBlob = await fetch(opened.previewUrl!).then((response) => response.blob());
    expect(openedBlob.size).toBe(file.size);
    await expect(fetch(composer.previewUrl!)).rejects.toThrow();

    revokeAttachmentPreviewUrls([opened, other, resource.attachment]);
  });
});
