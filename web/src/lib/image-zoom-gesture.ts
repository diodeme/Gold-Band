export const MIN_IMAGE_SCALE = 0.1;
export const MAX_IMAGE_SCALE = 8;

export interface ImageCanvasViewState {
  scale: number;
  positionX: number;
  positionY: number;
}

export interface ImageCanvasSize {
  width: number;
  height: number;
}

export function fitImageScale(image: ImageCanvasSize, viewport: ImageCanvasSize) {
  const padding = 40;
  return Math.max(MIN_IMAGE_SCALE, Math.min(
    1, (viewport.width - padding) / image.width, (viewport.height - padding) / image.height,
  ));
}

const WHEEL_ZOOM_SENSITIVITY = 0.003;
const MAX_NORMALIZED_WHEEL_DELTA = 120;
const WHEEL_LINE_HEIGHT_PX = 16;

export function normalizedWheelScale(
  currentScale: number,
  deltaY: number,
  deltaMode: number,
  pageHeight: number,
) {
  const deltaPixels = deltaY * (deltaMode === 1
    ? WHEEL_LINE_HEIGHT_PX
    : deltaMode === 2
      ? Math.max(1, pageHeight)
      : 1);
  const boundedDelta = Math.max(
    -MAX_NORMALIZED_WHEEL_DELTA,
    Math.min(MAX_NORMALIZED_WHEEL_DELTA, deltaPixels),
  );
  const nextScale = currentScale * Math.exp(-boundedDelta * WHEEL_ZOOM_SENSITIVITY);
  return Math.max(MIN_IMAGE_SCALE, Math.min(MAX_IMAGE_SCALE, nextScale));
}
