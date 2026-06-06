import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { AcpUsageVm } from '@/types';
import { formatTokenCount } from '@/lib/format-token';

export { formatTokenCount } from '@/lib/format-token';

export interface AcpUsagePanelProps {
  usage: AcpUsageVm | null | undefined;
  isRunning: boolean;
}

export function AcpUsagePanel({ usage }: AcpUsagePanelProps) {
  const { t } = useTranslation();

  const hasData = useMemo(() => {
    return usage != null && (usage.used != null || usage.size != null);
  }, [usage]);

  if (!hasData) return null;

  const used = usage!.used;
  const size = usage!.size;

  return (
    <div className="px-1 space-y-1 text-xs text-muted-foreground">
      {/* Row 1: Context Window */}
      <div className="flex items-center gap-2">
        <span className="text-muted-foreground/80">{t('acp.usagePanel.contextWindow')}</span>
        <span className="text-foreground/80 tabular-nums">
          {used != null ? formatTokenCount(used) : '--'}
          {size != null ? ` / ${formatTokenCount(size)}` : ''}
        </span>
      </div>

      {/* Row 2: Token Usage breakdown */}
      {hasTokenBreakdown(usage!) ? (
        <div className="flex items-center gap-2">
          <span className="text-muted-foreground/80">{t('acp.usagePanel.tokenUsage')}</span>
          <span className="flex items-center gap-3 tabular-nums text-foreground/80">
            {usage!.inputTokens != null ? <span>{t('acp.usagePanel.input')} {formatTokenCount(usage!.inputTokens)}</span> : null}
            {usage!.outputTokens != null ? <span>{t('acp.usagePanel.output')} {formatTokenCount(usage!.outputTokens)}</span> : null}
            {usage!.cachedReadTokens != null ? <span>{t('acp.usagePanel.cacheRead')} {formatTokenCount(usage!.cachedReadTokens)}</span> : null}
            {usage!.totalTokens != null ? <span className="font-medium">{t('acp.usagePanel.total')} {formatTokenCount(usage!.totalTokens)}</span> : null}
          </span>
        </div>
      ) : null}
    </div>
  );
}

function hasTokenBreakdown(usage: AcpUsageVm): boolean {
  return usage.inputTokens != null
    || usage.outputTokens != null
    || usage.cachedReadTokens != null
    || usage.cachedWriteTokens != null
    || usage.totalTokens != null;
}
