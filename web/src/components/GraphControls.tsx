import { ControlButton, Controls } from '@xyflow/react';
import { LocateFixed, Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

export function GraphControls({
  disabled = false,
  onZoomIn,
  onZoomOut,
  onFitView,
  onFocusNode,
}: {
  disabled?: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFitView: () => void;
  onFocusNode?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Controls showZoom={false} showFitView={false} showInteractive={false} position="bottom-right">
      {onFocusNode && <Tooltip>
        <TooltipTrigger asChild>
          <ControlButton disabled={disabled} aria-label={t('graph.focusNode')} onClick={onFocusNode}><LocateFixed /></ControlButton>
        </TooltipTrigger>
        <TooltipContent side="left">{t('graph.focusNode')}</TooltipContent>
      </Tooltip>}
      <Tooltip>
        <TooltipTrigger asChild>
          <ControlButton disabled={disabled} aria-label={t('graph.zoomIn')} onClick={onZoomIn}><ZoomIn /></ControlButton>
        </TooltipTrigger>
        <TooltipContent side="left">{t('graph.zoomIn')}</TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <ControlButton disabled={disabled} aria-label={t('graph.zoomOut')} onClick={onZoomOut}><ZoomOut /></ControlButton>
        </TooltipTrigger>
        <TooltipContent side="left">{t('graph.zoomOut')}</TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <ControlButton disabled={disabled} aria-label={t('graph.fitView')} onClick={onFitView}><Maximize2 /></ControlButton>
        </TooltipTrigger>
        <TooltipContent side="left">{t('graph.fitView')}</TooltipContent>
      </Tooltip>
    </Controls>
  );
}
