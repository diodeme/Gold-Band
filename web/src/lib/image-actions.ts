import { copyImageToClipboard, saveImageAs } from '@/api';
import type { ImageActionInput, ImageActionSourceInput } from '@/api/client';

const MAX_IMAGE_ACTION_BYTES = 25 * 1024 * 1024;
export const IMAGE_ACTION_FEEDBACK_DURATION_MS = 1_800;

export interface ImageActionAsset {
  name: string;
  mime: string;
  path?: string;
  file?: Blob;
  previewUrl?: string;
}

export async function copyImageAsset(asset: ImageActionAsset): Promise<void> {
  await copyImageToClipboard(await imageActionInput(asset));
}

export async function saveImageAssetAs(asset: ImageActionAsset): Promise<boolean> {
  return saveImageAs(await imageActionInput(asset));
}

export async function imageActionInput(asset: ImageActionAsset): Promise<ImageActionInput> {
  return {
    source: await imageActionSource(asset),
    fileName: asset.name,
    mime: asset.mime,
  };
}

async function imageActionSource(asset: ImageActionAsset): Promise<ImageActionSourceInput> {
  if (asset.path) return { kind: 'path', path: asset.path };
  if (asset.file) return blobImageSource(asset.file);
  if (asset.previewUrl) {
    const response = await fetch(asset.previewUrl, { cache: 'no-store' });
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
