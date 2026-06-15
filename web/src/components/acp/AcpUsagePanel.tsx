import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { AcpUsageVm } from '@/types';
import { cn } from '@/lib/utils';
import { formatTokenCount } from '@/lib/format-token';

export { formatTokenCount } from '@/lib/format-token';

export interface AcpUsagePanelProps {
  usage: AcpUsageVm | null | undefined;
  isRunning: boolean;
  compact?: boolean;
  processingLabel?: string | null;
  stepSeconds?: number | null;
  sessionSeconds?: number | null;
  className?: string;
}

export function AcpUsagePanel({ usage, isRunning, compact, processingLabel, stepSeconds, sessionSeconds, className }: AcpUsagePanelProps) {
  const { t } = useTranslation();

  const hasData = useMemo(() => {
    return usage != null && (usage.used != null || usage.size != null);
  }, [usage]);

  const showProcessing = compact && isRunning && processingLabel;
  const showTiming = compact && (stepSeconds != null || sessionSeconds != null);

  if (!hasData && !showProcessing && !showTiming) return null;

  const used = usage?.used;
  const size = usage?.size;

  const breakdown = usage ? hasTokenBreakdown(usage) : false;

  return (
    <div className={cn('px-1 text-xs text-muted-foreground', compact ? 'flex flex-wrap items-center gap-x-4 gap-y-0.5' : 'space-y-1', className)}>
      {/* Timing (compact mode, at the front) */}
      {showProcessing ? (
        <span className="flex items-center gap-1.5 font-medium text-foreground">
          <span
            aria-hidden="true"
            className="size-3.5 shrink-0 animate-spin rounded-full border-2 border-primary/25 border-t-primary [animation-duration:900ms]"
          />
          <span>{processingLabel}...</span>
        </span>
      ) : null}
      {showTiming ? (
        <>
          {stepSeconds != null ? (
            <span className="flex items-center gap-1.5">
              <span className="text-muted-foreground/80">{t('acp.timingStep')}</span>
              <span className="tabular-nums text-foreground/80">{formatElapsed(stepSeconds)}</span>
            </span>
          ) : null}
          {sessionSeconds != null ? (
            <span className="flex items-center gap-1.5">
              <span className="text-muted-foreground/80">{t('acp.timingSession')}</span>
              <span className="tabular-nums text-foreground/80">{formatElapsed(sessionSeconds)}</span>
            </span>
          ) : null}
        </>
      ) : null}

      {hasData ? (
        <span className="flex items-center gap-1.5">
          <span className="text-muted-foreground/80">{t('acp.usagePanel.contextWindow')}</span>
          <span className="text-foreground/80 tabular-nums">
            {used != null ? formatTokenCount(used) : '--'}
            {size != null ? ` / ${formatTokenCount(size)}` : ''}
          </span>
        </span>
      ) : null}

      {/* Token Usage breakdown */}
      {breakdown ? (
        <span className="flex items-center gap-1.5">
          <span className="text-muted-foreground/80">{t('acp.usagePanel.tokenUsage')}</span>
          <span className="flex items-center gap-3 tabular-nums text-foreground/80">
            {usage?.inputTokens != null ? <span>{t('acp.usagePanel.input')} {formatTokenCount(usage.inputTokens)}</span> : null}
            {usage?.outputTokens != null ? <span>{t('acp.usagePanel.output')} {formatTokenCount(usage.outputTokens)}</span> : null}
            {usage?.cachedReadTokens != null ? <span>{t('acp.usagePanel.cacheRead')} {formatTokenCount(usage.cachedReadTokens)}</span> : null}
            {usage?.totalTokens != null ? <span className="font-medium">{t('acp.usagePanel.total')} {formatTokenCount(usage.totalTokens)}</span> : null}
          </span>
        </span>
      ) : null}
    </div>
  );
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

function hasTokenBreakdown(usage: AcpUsageVm): boolean {
  return usage.inputTokens != null
    || usage.outputTokens != null
    || usage.cachedReadTokens != null
    || usage.cachedWriteTokens != null
    || usage.totalTokens != null;
}
