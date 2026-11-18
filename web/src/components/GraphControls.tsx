import { ControlButton, Controls } from '@xyflow/react';
import { Check, Expand, Maximize2, Minimize, MoveDown, MoveRight, Shuffle, ZoomIn, ZoomOut } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { WorkflowGraphDirectionPreference } from '@/types';

export interface GraphDirectionControlProps {
  preference: WorkflowGraphDirectionPreference;
  onChange: (next: WorkflowGraphDirectionPreference) => void;
}

export function GraphControls({
  disabled = false,
  onZoomIn,
  onZoomOut,
  onFitView,
  direction,
  onFullscreen,
  fullscreenActive = false,
}: {
  disabled?: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFitView: () => void;
  /** When present, renders the horizontal/vertical direction switcher. */
  direction?: GraphDirectionControlProps;
  /** When present, renders the fullscreen toggle. */
  onFullscreen?: () => void;
  /** Flips the toggle into its "leave fullscreen" affordance. */
  fullscreenActive?: boolean;
}) {
  const { t } = useTranslation();
  const directionIcon = direction?.preference === 'vertical'
    ? <MoveDown />
    : direction?.preference === 'horizontal'
      ? <MoveRight />
      : <Shuffle />;
  const fullscreenLabel = fullscreenActive ? t('graph.exitFullscreen') : t('graph.fullscreen');
  return (
    <Controls showZoom={false} showFitView={false} showInteractive={false} position="bottom-right">
      {onFullscreen ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <ControlButton disabled={disabled} aria-label={fullscreenLabel} onClick={onFullscreen}>
              {fullscreenActive ? <Minimize /> : <Expand />}
            </ControlButton>
          </TooltipTrigger>
          <TooltipContent side="left">{fullscreenLabel}</TooltipContent>
        </Tooltip>
      ) : null}
      {direction ? (
        <DropdownMenu>
          <Tooltip>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <ControlButton disabled={disabled} aria-label={t('graph.direction')}>{directionIcon}</ControlButton>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent side="left">{t('graph.direction')}</TooltipContent>
          </Tooltip>
          <DropdownMenuContent align="end" sideOffset={4}>
            <DropdownMenuItem
              aria-label={t('graph.directionAuto')}
              onClick={() => direction.onChange('auto')}
            >
              <Shuffle className="size-4" />
              <span className="flex-1">{t('graph.directionAuto')}</span>
              {direction.preference === 'auto' ? <Check aria-hidden className="size-4 text-muted-foreground" /> : null}
            </DropdownMenuItem>
            <DropdownMenuItem
              aria-label={t('graph.directionHorizontal')}
              onClick={() => direction.onChange('horizontal')}
            >
              <MoveRight className="size-4" />
              <span className="flex-1">{t('graph.directionHorizontal')}</span>
              {direction.preference === 'horizontal' ? <Check aria-hidden className="size-4 text-muted-foreground" /> : null}
            </DropdownMenuItem>
            <DropdownMenuItem
              aria-label={t('graph.directionVertical')}
              onClick={() => direction.onChange('vertical')}
            >
              <MoveDown className="size-4" />
              <span className="flex-1">{t('graph.directionVertical')}</span>
              {direction.preference === 'vertical' ? <Check aria-hidden className="size-4 text-muted-foreground" /> : null}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : null}
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
