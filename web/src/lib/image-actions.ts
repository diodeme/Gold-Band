import { copyImageToClipboard, saveImageAs } from '@/api';
import type { ImageActionInput, ImageActionSourceInput } from '@/api/client';
import type { AttachmentItem } from './attachment-service';

const MAX_IMAGE_ACTION_BYTES = 25 * 1024 * 1024;
export const IMAGE_ACTION_FEEDBACK_DURATION_MS = 1_800;

export async function copyAttachmentImage(attachment: AttachmentItem): Promise<void> {
  await copyImageToClipboard(await attachmentImageActionInput(attachment));
}

export async function saveAttachmentImageAs(attachment: AttachmentItem): Promise<boolean> {
  return saveImageAs(await attachmentImageActionInput(attachment));
}

export async function attachmentImageActionInput(
  attachment: AttachmentItem,
): Promise<ImageActionInput> {
  return {
    source: await attachmentImageSource(attachment),
    fileName: attachment.name,
    mime: attachment.mime,
  };
}

async function attachmentImageSource(attachment: AttachmentItem): Promise<ImageActionSourceInput> {
  if (attachment.path) return { kind: 'path', path: attachment.path };
  if (attachment.file) return blobImageSource(attachment.file);
  if (attachment.previewUrl) {
    const response = await fetch(attachment.previewUrl, { cache: 'no-store' });
    if (!response.ok) throw structuredImageActionError('image-action.source-unreadable');
    return blobImageSource(await response.blob());
  }
  throw structuredImageActionError('image-action.source-unreadable');
}

async function blobImageSource(blob: Blob): Promise<ImageActionSourceInput> {
  if (blob.size === 0) throw structuredImageActionError('image-action.source-invalid');
  if (blob.size > MAX_IMAGE_ACTION_BYTES) {
    throw structuredImageActionError('image-action.source-too-large');
  }
  return {
    kind: 'bytes',
    dataBase64: arrayBufferToBase64(await blob.arrayBuffer()),
  };
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const chunkSize = 0x8000;
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

function structuredImageActionError(code: string) {
  return { code, params: {} };
}
