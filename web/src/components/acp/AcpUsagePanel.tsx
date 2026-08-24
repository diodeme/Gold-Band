import { memo, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import type { AcpUsageVm } from '@/types';
import { cn } from '@/lib/utils';
import { formatTokenCount } from '@/lib/format-token';
import { AcpProcessingSpinner } from '@/components/acp/AcpProcessingSpinner';
import { GitFork } from 'lucide-react';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { ACP_SESSION_COMPOSER_BORDER_WIDTH_PX } from '@/lib/conversation-composer-layout';

export { formatTokenCount } from '@/lib/format-token';

export interface AcpUsagePanelProps {
  usage: AcpUsageVm | null | undefined;
  processingLabel?: string | null;
  sessionSeconds?: number | null;
  worktreePath?: string | null;
  className?: string;
}

type ContextGaugeStyle = CSSProperties & {
  '--context-usage-percent': string;
  '--context-usage-color': string;
};

export const CONTEXT_USAGE_THRESHOLDS = {
  elevated: 60,
  warning: 75,
  critical: 90,
} as const;

export type ContextUsageTone = 'unknown' | 'healthy' | 'elevated' | 'warning' | 'critical';

const ACP_SESSION_INFO_CONNECTOR_RADIUS_PX = 10;
const ACP_SESSION_INFO_CONNECTOR_EXTENT_PX = ACP_SESSION_INFO_CONNECTOR_RADIUS_PX
  + ACP_SESSION_COMPOSER_BORDER_WIDTH_PX;
const ACP_SESSION_INFO_CONNECTOR_VIEW_BOX = [
  -ACP_SESSION_COMPOSER_BORDER_WIDTH_PX,
  0,
  ACP_SESSION_INFO_CONNECTOR_EXTENT_PX,
  ACP_SESSION_INFO_CONNECTOR_EXTENT_PX,
].join(' ');
const ACP_SESSION_INFO_CONNECTOR_FILL_PATH = [
  `M ${-ACP_SESSION_COMPOSER_BORDER_WIDTH_PX} 0`,
  `H 0 A ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} 0 0 0 ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX}`,
  `V ${ACP_SESSION_INFO_CONNECTOR_EXTENT_PX}`,
  `H ${-ACP_SESSION_COMPOSER_BORDER_WIDTH_PX} Z`,
].join(' ');
const ACP_SESSION_INFO_CONNECTOR_STROKE_PATH = `M 0 0 A ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} 0 0 0 ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX} ${ACP_SESSION_INFO_CONNECTOR_RADIUS_PX}`;

const CONTEXT_USAGE_TONE_COLORS: Record<ContextUsageTone, string> = {
  unknown: 'var(--muted-foreground)',
  healthy: 'var(--gold-success)',
  elevated: 'var(--gold-running)',
  warning: 'var(--gold-warning)',
  critical: 'var(--gold-danger)',
};

export const AcpUsagePanel = memo(function AcpUsagePanel({
  usage,
  processingLabel,
  sessionSeconds,
  worktreePath,
  className,
}: AcpUsagePanelProps) {
  const { t } = useTranslation();

  const used = usage?.used != null && usage.used > 0 ? usage.used : null;
  const size = usage?.size != null && usage.size > 0 ? usage.size : null;
  const percentage = contextUsagePercentage(used, size);
  const usageTone = contextUsageTone(percentage);
  const percentageLabel = percentage == null ? '--' : `${percentage}%`;
  const usageLabel = `${used == null ? '--' : formatTokenCount(used)}${size == null ? '' : ` / ${formatTokenCount(size)}`}`;
  const tokenRows = usage == null ? [] : tokenUsageRows(usage);
  const showProcessing = Boolean(processingLabel);
  const showTiming = sessionSeconds != null;
  const showWorktree = Boolean(worktreePath?.trim());
  const showContext = hasAcpUsagePanelContent(usage);

  if (!showProcessing && !showTiming && !showWorktree && !showContext) return null;

  const gaugeStyle: ContextGaugeStyle = {
    '--context-usage-percent': `${percentage ?? 0}%`,
    '--context-usage-color': CONTEXT_USAGE_TONE_COLORS[usageTone],
  };

  return (
    <div
      className={cn(
        'flex flex-wrap items-center gap-x-4 gap-y-1 px-1 text-xs leading-4 text-muted-foreground/75',
        className,
      )}
      data-acp-session-info="true"
    >
      {showProcessing ? (
        <span className="flex min-w-0 items-center gap-1.5 font-medium text-foreground">
          <AcpProcessingSpinner className="size-3.5 shrink-0" />
          <span className="truncate">{processingLabel}</span>
        </span>
      ) : null}

      {showTiming ? (
        <span className="flex shrink-0 items-center gap-1.5">
          <span className="text-muted-foreground/80">{t('acp.timingSession')}</span>
          <span className="tabular-nums text-foreground/80">{formatElapsed(sessionSeconds)}</span>
        </span>
      ) : null}

      {showContext ? (
        <span className="flex shrink-0 items-center gap-1.5">
          <span className="text-muted-foreground/80">{t('acp.usagePanel.contextWindow')}</span>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
                aria-label={`${t('acp.usagePanel.contextWindow')} ${t('acp.usagePanel.occupied')} ${usageLabel} ${percentageLabel}`}
                data-context-usage-gauge="true"
                data-context-usage-tone={usageTone}
              >
                <span
                  aria-hidden="true"
                  className="grid size-6 place-items-center rounded-full bg-[conic-gradient(var(--context-usage-color)_var(--context-usage-percent),var(--border)_0)] p-0.5"
                  style={gaugeStyle}
                >
                  <span className="flex size-full items-center justify-center rounded-full bg-background text-[9px] font-medium leading-none tracking-[-0.02em] tabular-nums text-foreground">
                    {percentage ?? '--'}
                  </span>
                </span>
              </button>
            </TooltipTrigger>
            <TooltipContent side="top" sideOffset={6} className="min-w-44 p-3">
              <div className="space-y-2">
                <div className="flex items-baseline justify-between gap-4 border-b border-border/60 pb-2">
                  <span className="text-muted-foreground">{t('acp.usagePanel.occupied')}</span>
                  <span className="font-medium tabular-nums text-popover-foreground">{usageLabel}</span>
                </div>
                {tokenRows.length > 0 ? (
                  <dl className="space-y-1.5">
                    {tokenRows.map(([labelKey, value]) => (
                      <div key={labelKey} className="flex items-center justify-between gap-6">
                        <dt className="text-muted-foreground">{t(labelKey)}</dt>
                        <dd className="font-medium tabular-nums">{formatTokenCount(value)}</dd>
                      </div>
                    ))}
                  </dl>
                ) : null}
              </div>
            </TooltipContent>
          </Tooltip>
        </span>
      ) : null}

      {showWorktree ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              className="ml-auto flex min-w-0 shrink items-center gap-1 rounded-sm text-foreground/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              tabIndex={0}
              data-acp-worktree="true"
            >
              <GitFork className="size-3.5 shrink-0" />
              <span className="truncate">{t('conversation.runtime.worktree')}</span>
            </span>
          </TooltipTrigger>
          <TooltipContent side="top" sideOffset={6} className="max-w-[min(36rem,calc(100vw-2rem))] break-all">
            {worktreePath}
          </TooltipContent>
        </Tooltip>
      ) : null}

      <svg
        aria-hidden="true"
        className="pointer-events-none absolute -right-2.5 bottom-[calc(-1*var(--acp-session-composer-border-width))] h-[calc(0.625rem+var(--acp-session-composer-border-width))] w-[calc(0.625rem+var(--acp-session-composer-border-width))] overflow-visible"
        data-acp-session-info-connector="true"
        viewBox={ACP_SESSION_INFO_CONNECTOR_VIEW_BOX}
      >
        <path
          d={ACP_SESSION_INFO_CONNECTOR_FILL_PATH}
          fill="var(--card)"
        />
        <path
          d={ACP_SESSION_INFO_CONNECTOR_STROKE_PATH}
          fill="none"
          stroke="var(--border)"
          strokeWidth={ACP_SESSION_COMPOSER_BORDER_WIDTH_PX}
          vectorEffect="non-scaling-stroke"
        />
      </svg>
    </div>
  );
}, areUsagePanelPropsEqual);

export function contextUsagePercentage(
  used: number | null | undefined,
  size: number | null | undefined,
): number | null {
  if (used == null || size == null || used < 0 || size <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((used / size) * 100)));
}

export function contextUsageTone(
  percentage: number | null | undefined,
): ContextUsageTone {
  if (percentage == null) return 'unknown';
  if (percentage >= CONTEXT_USAGE_THRESHOLDS.critical) return 'critical';
  if (percentage >= CONTEXT_USAGE_THRESHOLDS.warning) return 'warning';
  if (percentage >= CONTEXT_USAGE_THRESHOLDS.elevated) return 'elevated';
  return 'healthy';
}

export function hasAcpUsagePanelContent(
  usage: AcpUsageVm | null | undefined,
) {
  if (usage == null) return false;
  const used = usage.used != null && usage.used > 0 ? usage.used : null;
  const size = usage.size != null && usage.size > 0 ? usage.size : null;
  return used != null || size != null || tokenUsageRows(usage).length > 0;
}

function tokenUsageRows(usage: AcpUsageVm): Array<[string, number]> {
  const rows: Array<[string, number | null | undefined]> = [
    ['acp.usagePanel.input', usage.inputTokens],
    ['acp.usagePanel.output', usage.outputTokens],
    ['acp.usagePanel.cacheRead', usage.cachedReadTokens],
    ['acp.usagePanel.cacheWrite', usage.cachedWriteTokens],
    ['acp.usagePanel.total', usage.totalTokens],
  ];
  return rows.filter((row): row is [string, number] => row[1] != null);
}

function areUsagePanelPropsEqual(previous: AcpUsagePanelProps, next: AcpUsagePanelProps) {
  return previous.className === next.className
    && previous.processingLabel === next.processingLabel
    && previous.sessionSeconds === next.sessionSeconds
    && previous.worktreePath === next.worktreePath
    && usageFieldsEqual(previous.usage, next.usage);
}

function usageFieldsEqual(
  previous: AcpUsageVm | null | undefined,
  next: AcpUsageVm | null | undefined,
) {
  if (previous === next) return true;
  return previous?.used === next?.used
    && previous?.size === next?.size
    && previous?.inputTokens === next?.inputTokens
    && previous?.outputTokens === next?.outputTokens
    && previous?.cachedReadTokens === next?.cachedReadTokens
    && previous?.cachedWriteTokens === next?.cachedWriteTokens
    && previous?.totalTokens === next?.totalTokens;
}

function formatElapsed(totalSeconds: number): string {
  if (totalSeconds < 60) return `${totalSeconds}s`;
  if (totalSeconds < 3600) {
    const m = Math.floor(totalSeconds / 60);
    const s = totalSeconds % 60;
    return `${m}m ${s}s`;
  }
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  return `${h}h ${m}m`;
}
