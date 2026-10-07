import { memo, useCallback, useEffect, useLayoutEffect, useRef, type ReactNode } from 'react';
import { LoaderCircle, Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  TransformComponent,
  TransformWrapper,
  type ReactZoomPanPinchContentRef,
} from 'react-zoom-pan-pinch';

import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ImageActionsContextMenu } from '@/components/shared/ImageActionsContextMenu';
import { useImageActions } from '@/hooks/useImageActions';
import type { ImageActionAsset } from '@/lib/image-actions';
import {
  MAX_IMAGE_SCALE,
  MIN_IMAGE_SCALE,
  normalizedWheelScale,
  fitImageScale,
  type ImageCanvasSize,
  type ImageCanvasViewState,
} from '@/lib/image-zoom-gesture';

const IMAGE_ZOOM_STEP = 0.2;

export interface WorkspaceImageCanvasProps {
  src: string;
  alt: string;
  imageActionAsset?: ImageActionAsset;
  onError?: () => void;
  /** Stable identity when the same image receives a refreshed preview URL. */
  resourceKey?: string;
  /** Optional known intrinsic size; attachments read it from the loaded image. */
  imageSize?: ImageCanvasSize;
  initialViewState?: ImageCanvasViewState | null;
  onViewStateChange?: (state: ImageCanvasViewState) => void;
  toolbarBefore?: ReactNode;
  toolbarAfter?: ReactNode;
}

export const WorkspaceImageCanvas = memo(function WorkspaceImageCanvas(props: WorkspaceImageCanvasProps) {
  return <ImageCanvas key={props.resourceKey ?? props.src} {...props} />;
});

function ImageCanvas({
  src,
  alt,
  imageActionAsset,
  onError,
  imageSize,
  initialViewState,
  onViewStateChange,
  toolbarBefore,
  toolbarAfter,
}: WorkspaceImageCanvasProps) {
  const { t } = useTranslation();
  const transformRef = useRef<ReactZoomPanPinchContentRef>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);
  const initialFitCompleteRef = useRef(Boolean(initialViewState));
  const initialFitObserverRef = useRef<ResizeObserver | null>(null);
  const scaleLabelRef = useRef<HTMLDivElement>(null);
  const initialState = useRef(initialViewState ?? { scale: 1, positionX: 0, positionY: 0 }).current;
  const transformStateRef = useRef(initialState);
  const pendingTransformRef = useRef<typeof transformStateRef.current | null>(null);
  const transformFrameRef = useRef<number | null>(null);
  const imageActions = useImageActions(imageActionAsset);

  useEffect(() => {
    return () => {
      if (transformFrameRef.current !== null) cancelAnimationFrame(transformFrameRef.current);
      transformFrameRef.current = null;
      pendingTransformRef.current = null;
    };
  }, []);

  const fitImage = useCallback((duration = 180) => {
    const viewport = viewportRef.current;
    const width = imageSize?.width ?? imageRef.current?.naturalWidth ?? 0;
    const height = imageSize?.height ?? imageRef.current?.naturalHeight ?? 0;
    if (!viewport || !transformRef.current?.instance.contentComponent || width <= 0 || height <= 0
      || viewport.clientWidth <= 0 || viewport.clientHeight <= 0) return false;
    const scale = fitImageScale({ width, height }, { width: viewport.clientWidth, height: viewport.clientHeight });
    transformRef.current.centerView(scale, duration, 'easeOut');
    return true;
  }, [imageSize?.width, imageSize?.height]);

  const initializeFit = useCallback(() => {
    if (initialFitCompleteRef.current || !fitImage(0)) return;
    initialFitCompleteRef.current = true;
    initialFitObserverRef.current?.disconnect();
    initialFitObserverRef.current = null;
  }, [fitImage]);

  useLayoutEffect(() => {
    initializeFit();
    const viewport = viewportRef.current;
    if (initialFitCompleteRef.current || !viewport) return;
    // Wait for the first usable layout only; later resizing must not reset user zoom.
    const observer = new ResizeObserver(initializeFit);
    initialFitObserverRef.current = observer;
    observer.observe(viewport);
    return () => { observer.disconnect(); initialFitObserverRef.current = null; };
  }, [initializeFit]);

  const handleWheelZoom = useCallback((event: WheelEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const viewportElement = viewportRef.current;
    if (!transformRef.current || !viewportElement) return;

    const current = pendingTransformRef.current ?? transformStateRef.current;
    const viewport = viewportElement.getBoundingClientRect();
    const nextScale = normalizedWheelScale(
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
    viewportElement.addEventListener('wheel', handleWheelZoom, { passive: false });
    return () => viewportElement.removeEventListener('wheel', handleWheelZoom);
  }, [handleWheelZoom]);

  const image = (
    <img
      ref={imageRef}
      src={src}
      alt={alt}
      draggable={false}
      onError={onError}
      onLoad={initializeFit}
      className="max-w-none shrink-0 select-none object-contain shadow-lg"
      style={imageSize ? { width: imageSize.width, height: imageSize.height } : undefined}
    />
  );

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden" data-workspace-image-canvas="true">
      <div className="flex h-9 shrink-0 items-center justify-end gap-1 border-b border-border/40 px-2">
        {toolbarBefore}
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.zoomOut(IMAGE_ZOOM_STEP)} aria-label={t('workspace.filesPanel.zoomOut')}>
          <ZoomOut className="size-3.5" />
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => fitImage()} aria-label={t('workspace.filesPanel.fitImage')}>
              <Maximize2 className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('workspace.filesPanel.fitImage')}</TooltipContent>
        </Tooltip>
        <Button type="button" size="icon" variant="ghost" className="size-7" onClick={() => transformRef.current?.zoomIn(IMAGE_ZOOM_STEP)} aria-label={t('workspace.filesPanel.zoomIn')}>
          <ZoomIn className="size-3.5" />
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" size="sm" variant="ghost" className="h-7 px-1.5 text-xs" onClick={() => transformRef.current?.centerView(1, 180, 'easeOut')} aria-label={t('workspace.filesPanel.originalSize')}>100%</Button>
          </TooltipTrigger>
          <TooltipContent>{t('workspace.filesPanel.originalSize')}</TooltipContent>
        </Tooltip>
        {toolbarAfter}
      </div>
      <TransformWrapper
        ref={transformRef}
        onInit={initializeFit}
        minScale={MIN_IMAGE_SCALE}
        maxScale={MAX_IMAGE_SCALE}
        initialScale={initialState.scale}
        initialPositionX={initialState.positionX}
        initialPositionY={initialState.positionY}
        centerZoomedOut
        limitToBounds={false}
        wheel={{ disabled: true }}
        trackPadPanning={{ disabled: true }}
        panning={{
          allowLeftClickPan: true,
          allowRightClickPan: false,
          velocityDisabled: true,
        }}
        pinch={{ disabled: false }}
        doubleClick={{ mode: 'toggle', step: 0.75 }}
        onTransform={(_, state) => {
          transformStateRef.current = state;
          if (scaleLabelRef.current) scaleLabelRef.current.textContent = `${Math.round(state.scale * 100)}%`;
          onViewStateChange?.({ scale: state.scale, positionX: state.positionX, positionY: state.positionY });
        }}
      >
        <div ref={viewportRef} className="min-h-0 flex-1 overflow-hidden" data-workspace-image-viewport="true">
          <TransformComponent
            wrapperClass="!h-full !w-full cursor-grab bg-background active:cursor-grabbing"
            contentClass="items-center justify-center"
          >
            {imageActionAsset ? (
              <ImageActionsContextMenu
                actions={imageActions}
                triggerClassName="inline-flex max-h-full max-w-full"
              >
                {image}
              </ImageActionsContextMenu>
            ) : image}
          </TransformComponent>
        </div>
      </TransformWrapper>
      <div className="flex shrink-0 items-center justify-between gap-3 border-t border-border/40 px-3 py-1 text-ui-micro text-muted-foreground">
        <span aria-live="polite" className="inline-flex min-w-0 items-center gap-1 truncate">
          {imageActions.pending ? <LoaderCircle className="size-3 shrink-0 animate-spin" /> : null}
          {imageActions.message}
          {imageSize ? `${imageSize.width} × ${imageSize.height}` : null}
        </span>
        <span ref={scaleLabelRef}>{Math.round(initialState.scale * 100)}%</span>
      </div>
    </div>
  );
}
