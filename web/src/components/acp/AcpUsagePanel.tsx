import { useState, useMemo } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from '@/components/ui/collapsible';
import { useTranslation } from 'react-i18next';
import type { AcpUsageVm } from '@/types';

export interface AcpUsagePanelProps {
  usage: AcpUsageVm | null | undefined;
  isRunning: boolean;
}

export function formatTokenCount(n: number): string {
  return n.toLocaleString();
}

export function usageRatio(used: number, size: number): number {
  if (size <= 0) return 0;
  return Math.min(used / size, 1);
}

export function ratioPercent(ratio: number): string {
  return `${(ratio * 100).toFixed(1)}%`;
}

export function AcpUsagePanel({ usage }: AcpUsagePanelProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const hasData = useMemo(() => {
    return usage != null && (usage.used != null || usage.size != null);
  }, [usage]);

  if (!hasData) return null;

  const used = usage!.used;
  const accumulated = usage!.accumulatedUsed;
  const size = usage!.size;
  const ratio = used != null && size != null ? usageRatio(used, size) : null;

  const progressColor =
    ratio != null
      ? ratio >= 0.95
        ? 'bg-red-500/60'
        : ratio >= 0.8
          ? 'bg-amber-500/60'
          : 'bg-primary/40'
      : 'bg-primary/40';

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="px-1">
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <CollapsibleTrigger className="flex items-center gap-1.5 hover:text-foreground transition-colors cursor-pointer select-none">
          {open ? <ChevronDown className="size-3" /> : <ChevronUp className="size-3" />}
          <span>{t('acp.usagePanel.title')}</span>
        </CollapsibleTrigger>
        <span className="text-foreground/80 tabular-nums">
          {used != null ? formatTokenCount(used) : '--'}
          {size != null ? ` / ${formatTokenCount(size)}` : ''} token
          {accumulated != null && accumulated > 0 && accumulated !== used ? (
            <> &middot; {t('acp.usagePanel.accumulated')} {formatTokenCount(accumulated)}</>
          ) : null}
        </span>
      </div>

      <CollapsibleContent className="mt-2 space-y-1.5 overflow-hidden data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down">
        {/* Detail breakdown */}
        <div className="grid grid-cols-2 gap-x-4 gap-y-0.5 text-[11px]">
          {usage!.inputTokens != null ? (
            <>
              <span className="text-muted-foreground/70">{t('acp.usagePanel.input')}</span>
              <span className="text-right tabular-nums text-foreground/80">{formatTokenCount(usage!.inputTokens)}</span>
            </>
          ) : null}
          {usage!.outputTokens != null ? (
            <>
              <span className="text-muted-foreground/70">{t('acp.usagePanel.output')}</span>
              <span className="text-right tabular-nums text-foreground/80">{formatTokenCount(usage!.outputTokens)}</span>
            </>
          ) : null}
          {usage!.cachedReadTokens != null ? (
            <>
              <span className="text-muted-foreground/70">{t('acp.usagePanel.cacheRead')}</span>
              <span className="text-right tabular-nums text-foreground/80">{formatTokenCount(usage!.cachedReadTokens)}</span>
            </>
          ) : null}
          {usage!.cachedWriteTokens != null ? (
            <>
              <span className="text-muted-foreground/70">{t('acp.usagePanel.cacheWrite')}</span>
              <span className="text-right tabular-nums text-foreground/80">{formatTokenCount(usage!.cachedWriteTokens)}</span>
            </>
          ) : null}
          {usage!.totalTokens != null ? (
            <>
              <span className="text-muted-foreground/70 font-medium">{t('acp.usagePanel.total')}</span>
              <span className="text-right tabular-nums text-foreground font-medium">{formatTokenCount(usage!.totalTokens)}</span>
            </>
          ) : null}
        </div>

        {/* Context window warning */}
        {ratio != null && ratio >= 0.95 ? (
          <p className="text-[11px] text-red-400/80 pt-1">{t('acp.usagePanel.contextWarning')}</p>
        ) : null}
      </CollapsibleContent>
    </Collapsible>
  );
}
