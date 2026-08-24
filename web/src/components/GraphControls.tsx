import { ControlButton, Controls } from '@xyflow/react';
import { Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

export function GraphControls({
  disabled = false,
  onZoomIn,
  onZoomOut,
  onFitView,
}: {
  disabled?: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFitView: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Controls showZoom={false} showFitView={false} showInteractive={false} position="bottom-right">
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
