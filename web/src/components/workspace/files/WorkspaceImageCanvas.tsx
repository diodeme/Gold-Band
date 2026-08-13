import { memo, useCallback, useEffect, useRef } from 'react';
import { Maximize2, RotateCcw, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  TransformComponent,
  TransformWrapper,
  type ReactZoomPanPinchContentRef,
} from 'react-zoom-pan-pinch';

import { Button } from '@/components/ui/button';
import {
  MAX_IMAGE_SCALE,
  MIN_IMAGE_SCALE,
  normalizedCtrlWheelScale,
} from '@/lib/image-zoom-gesture';

const IMAGE_ZOOM_STEP = 0.2;

export interface WorkspaceImageCanvasProps {
  src: string;
  alt: string;
  onError?: () => void;
}

export const WorkspaceImageCanvas = memo(function WorkspaceImageCanvas({
  src,
  alt,
  onError,
}: WorkspaceImageCanvasProps) {
  const { t } = useTranslation();
  const transformRef = useRef<ReactZoomPanPinchContentRef>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const scaleLabelRef = useRef<HTMLDivElement>(null);
  const transformStateRef = useRef({ scale: 1, positionX: 0, positionY: 0 });
  const pendingTransformRef = useRef<typeof transformStateRef.current | null>(null);
  const transformFrameRef = useRef<number | null>(null);

  useEffect(() => {
    transformStateRef.current = { scale: 1, positionX: 0, positionY: 0 };
    pendingTransformRef.current = null;
    if (scaleLabelRef.current) scaleLabelRef.current.textContent = '100%';
    return () => {
      if (transformFrameRef.current !== null) cancelAnimationFrame(transformFrameRef.current);
      transformFrameRef.current = null;
      pendingTransformRef.current = null;
    };
  }, [src]);

  const handleCtrlWheelZoom = useCallback((event: WheelEvent) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    event.stopPropagation();
    const viewportElement = viewportRef.current;
    if (!transformRef.current || !viewportElement) return;

    const current = pendingTransformRef.current ?? transformStateRef.current;
    const viewport = viewportElement.getBoundingClientRect();
    const nextScale = normalizedCtrlWheelScale(
      current.scale,
      event.deltaY,
      event.deltaMode,
      viewport.height,
    );
    if (nextScale === current.scale) return;

    const pointerX = event.clientX - viewport.left;
    const pointerY = event.clientY - viewport.top;
    const ratio = nextScale / current.scale;
    pendingTransformRef.current = {
      scale: nextScale,
      positionX: pointerX - (pointerX - current.positionX) * ratio,
      positionY: pointerY - (pointerY - current.positionY) * ratio,
    };
    if (transformFrameRef.current !== null) return;
    transformFrameRef.current = requestAnimationFrame(() => {
      transformFrameRef.current = null;
      const next = pendingTransformRef.current;
      pendingTransformRef.current = null;
      if (!next) return;
      transformStateRef.current = next;
      transformRef.current?.setTransform(next.positionX, next.positionY, next.scale, 0);
    });
  }, []);

  useEffect(() => {
    const viewportElement = viewportRef.current;
    if (!viewportElement) return;
    viewportElement.addEventListener('wheel', handleCtrlWheelZoom, { passive: false });
    return () => viewportElement.removeEventListener('wheel', handleCtrlWheelZoom);
  }, [handleCtrlWheelZoom, src]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden" data-workspace-image-canvas="true">
      <div className="flex h-9 shrink-0 items-center justify-end gap-1 border-b border-border/40 px-2">
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.zoomOut(IMAGE_ZOOM_STEP)} aria-label={t('workspace.filesPanel.zoomOut')}>
          <ZoomOut className="size-3.5" />
        </Button>
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.centerView(1, 180, 'easeOut')} aria-label={t('workspace.filesPanel.fitImage')}>
          <Maximize2 className="size-3.5" />
        </Button>
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.resetTransform(180, 'easeOut')} aria-label={t('workspace.filesPanel.resetImage')}>
          <RotateCcw className="size-3.5" />
        </Button>
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.zoomIn(IMAGE_ZOOM_STEP)} aria-label={t('workspace.filesPanel.zoomIn')}>
          <ZoomIn className="size-3.5" />
        </Button>
      </div>
      <TransformWrapper
        key={src}
        ref={transformRef}
        minScale={MIN_IMAGE_SCALE}
        maxScale={MAX_IMAGE_SCALE}
        initialScale={1}
        centerOnInit
        centerZoomedOut
        limitToBounds={false}
        wheel={{ disabled: true }}
        trackPadPanning={{ disabled: true }}
        panning={{ allowLeftClickPan: true, velocityDisabled: true }}
        pinch={{ disabled: false }}
        doubleClick={{ mode: 'toggle', step: 0.75 }}
        onTransform={(_, state) => {
          transformStateRef.current = state;
          if (scaleLabelRef.current) scaleLabelRef.current.textContent = `${Math.round(state.scale * 100)}%`;
        }}
      >
        <div ref={viewportRef} className="min-h-0 flex-1 overflow-hidden" data-workspace-image-viewport="true">
          <TransformComponent
            wrapperClass="!h-full !w-full cursor-grab bg-background active:cursor-grabbing"
            contentClass="!h-full !w-full items-center justify-center p-5"
          >
            <img
              src={src}
              alt={alt}
              draggable={false}
              onError={onError}
              className="max-h-full max-w-full select-none object-contain shadow-lg"
            />
          </TransformComponent>
        </div>
      </TransformWrapper>
      <div ref={scaleLabelRef} className="shrink-0 border-t border-border/40 px-3 py-1 text-[10px] text-muted-foreground">100%</div>
    </div>
  );
});
