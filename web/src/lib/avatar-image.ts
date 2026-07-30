import type { Area } from 'react-easy-crop';

export const AVATAR_OUTPUT_SIZE = 320;
export const MAX_AVATAR_SOURCE_BYTES = 10 * 1024 * 1024;
export const SUPPORTED_AVATAR_MIME_TYPES = ['image/png', 'image/jpeg', 'image/webp'] as const;

export interface CroppedAvatarPayload {
  mimeType: string;
  dataBase64: string;
}

export async function readAvatarFile(file: File): Promise<string> {
  if (!SUPPORTED_AVATAR_MIME_TYPES.includes(file.type as (typeof SUPPORTED_AVATAR_MIME_TYPES)[number])) {
    throw new Error('avatar.unsupported-image-type');
  }
  if (file.size <= 0 || file.size > MAX_AVATAR_SOURCE_BYTES) {
    throw new Error('avatar.source-image-too-large');
  }
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === 'string'
      ? resolve(reader.result)
      : reject(new Error('avatar.invalid-image-data'));
    reader.onerror = () => reject(new Error('avatar.invalid-image-data'));
    reader.readAsDataURL(file);
  });
}

export async function cropAvatarImage(imageSource: string, crop: Area): Promise<CroppedAvatarPayload> {
  const image = await loadImage(imageSource);
  const canvas = document.createElement('canvas');
  canvas.width = AVATAR_OUTPUT_SIZE;
  canvas.height = AVATAR_OUTPUT_SIZE;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('avatar.crop-failed');
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = 'high';
  context.drawImage(
    image,
    crop.x,
    crop.y,
    crop.width,
    crop.height,
    0,
    0,
    AVATAR_OUTPUT_SIZE,
    AVATAR_OUTPUT_SIZE,
  );
  const blob = await canvasToBlob(canvas, 'image/webp', 0.9);
  const dataUrl = await blobToDataUrl(blob);
  return {
    mimeType: blob.type || 'image/webp',
    dataBase64: dataUrl.slice(dataUrl.indexOf(',') + 1),
  };
}

function loadImage(source: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('avatar.invalid-image-data'));
    image.src = source;
  });
}

function canvasToBlob(canvas: HTMLCanvasElement, type: string, quality: number) {
  return new Promise<Blob>((resolve, reject) => {
    canvas.toBlob((blob) => blob ? resolve(blob) : reject(new Error('avatar.crop-failed')), type, quality);
  });
}

function blobToDataUrl(blob: Blob) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === 'string'
      ? resolve(reader.result)
      : reject(new Error('avatar.crop-failed'));
    reader.onerror = () => reject(new Error('avatar.crop-failed'));
    reader.readAsDataURL(blob);
  });
}
