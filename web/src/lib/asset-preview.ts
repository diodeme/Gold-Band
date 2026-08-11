import type { ContentVm } from '@/types';

export interface MessageAttachmentPreview {
  name: string;
  path: string;
  type: string;
  size: number;
}

export interface MessageAttachmentPreviewGroups {
  images: MessageAttachmentPreview[];
  files: MessageAttachmentPreview[];
}

function metadataRecord(metadata: unknown): Record<string, unknown> {
  return metadata && typeof metadata === 'object' && !Array.isArray(metadata)
    ? metadata as Record<string, unknown>
    : {};
}

export function isImageMimeType(value?: string | null): boolean {
  return Boolean(value?.startsWith('image/') && !value.includes('svg'));
}

export function imageMimeTypeFromContent(content: ContentVm | null | undefined): string | null {
  if (!content) return null;
  const metadata = metadataRecord(content.metadata);
  const metadataMime = typeof metadata.mimeType === 'string' ? metadata.mimeType : null;
  if (metadataMime) return metadataMime;
  const match = content.content.match(/^data:([^;,]+)[;,]/);
  return match?.[1] ?? null;
}

export function imageSrcFromContent(content: ContentVm | null | undefined): string | null {
  if (!content) return null;
  const mime = imageMimeTypeFromContent(content);
  if (!isImageMimeType(mime)) return null;
  return content.content.startsWith('data:image/') ? content.content : null;
}

export function isImageMessageAttachment(attachment: MessageAttachmentPreview): boolean {
  return isImageMimeType(attachment.type);
}

export function isTaskInputMessageAttachment(attachment: MessageAttachmentPreview): boolean {
  return attachment.path.replaceAll('\\', '/').startsWith('task-inputs/');
}

export function messageAttachmentPreviewsFromRaw(raw: unknown): MessageAttachmentPreview[] {
  const attachments = metadataRecord(raw).attachments;
  if (!Array.isArray(attachments)) return [];
  return attachments.filter((attachment): attachment is MessageAttachmentPreview => {
    const value = metadataRecord(attachment);
    return typeof value.name === 'string'
      && typeof value.path === 'string'
      && typeof value.type === 'string'
      && typeof value.size === 'number';
  });
}

export function groupMessageAttachmentPreviews(
  attachments: readonly MessageAttachmentPreview[],
): MessageAttachmentPreviewGroups {
  const groups: MessageAttachmentPreviewGroups = { images: [], files: [] };
  for (const attachment of attachments) {
    if (isImageMessageAttachment(attachment)) {
      groups.images.push(attachment);
    } else {
      groups.files.push(attachment);
    }
  }
  return groups;
}
