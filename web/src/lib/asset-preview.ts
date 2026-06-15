import type { ContentVm } from '@/types';

export interface MessageAttachmentPreview {
  name: string;
  path: string;
  type: string;
  size: number;
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
