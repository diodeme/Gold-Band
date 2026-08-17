import { useEffect, useState, type ReactNode } from 'react';
import { LoaderCircle, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { ReadonlyUnifiedDiff } from '@/components/workspace/files/ReadonlyUnifiedDiff';
import {
  loadTurnFileComparison,
  readCachedTurnFileComparison,
  turnFileComparisonCacheKey,
} from '@/lib/turn-file-comparison-cache';
import type { FileComparisonVm, TurnFileChangeVm, TurnFileLocatorVm } from '@/types';

export function TurnFileDiffPreview({
  locator,
  changeSetId,
  change,
}: {
  locator: TurnFileLocatorVm;
  changeSetId: string;
  change: TurnFileChangeVm;
}) {
  const { t } = useTranslation();
  const requestKey = turnFileComparisonCacheKey(locator, changeSetId, change.id);
  const initialComparison = readCachedTurnFileComparison(locator, changeSetId, change.id);
  const [state, setState] = useState<{
    key: string;
    comparison: FileComparisonVm | null;
    errorCode: string | null;
  }>(() => ({ key: requestKey, comparison: initialComparison, errorCode: null }));

  useEffect(() => {
    let cancelled = false;
    const cached = readCachedTurnFileComparison(locator, changeSetId, change.id);
    setState({ key: requestKey, comparison: cached, errorCode: null });
    if (cached) return () => { cancelled = true; };
    void loadTurnFileComparison(locator, changeSetId, change.id)
      .then((comparison) => {
        if (!cancelled) setState({ key: requestKey, comparison, errorCode: null });
      })
      .catch((reason: unknown) => {
        if (!cancelled) setState({ key: requestKey, comparison: null, errorCode: commandErrorCode(reason) });
      });
    return () => { cancelled = true; };
  }, [change.id, changeSetId, locator, requestKey]);

  const comparison = state.key === requestKey ? state.comparison : initialComparison;
  const errorCode = state.key === requestKey ? state.errorCode : null;
  const addedLines = comparison?.stats.addedLines ?? change.addedLines ?? 0;
  const deletedLines = comparison?.stats.deletedLines ?? change.deletedLines ?? 0;

  return (
    <section className="flex h-[clamp(10rem,44vh,24rem)] min-h-0 w-[min(40rem,calc(100vw-2rem))] min-w-0 flex-col overflow-hidden" data-turn-file-diff-preview={change.id}>
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-2.5 text-xs">
        <span className="min-w-0 flex-1 truncate font-mono text-foreground">{change.logicalPath}</span>
        <span className="shrink-0 tabular-nums text-emerald-600 dark:text-emerald-400">+{addedLines}</span>
        <span className="shrink-0 tabular-nums text-destructive">-{deletedLines}</span>
      </header>
      <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
        {errorCode ? (
          <PreviewMessage
            icon={<TriangleAlert className="size-4 text-destructive" />}
            text={t(`errors.${errorCode}`, { defaultValue: t('turnFiles.previewLoadFailed') })}
          />
        ) : !comparison ? (
          <PreviewMessage icon={<LoaderCircle className="size-4 animate-spin" />} text={t('turnFiles.previewLoading')} />
        ) : comparison.limitationCode && !comparison.after && !comparison.before ? (
          <PreviewMessage
            icon={<TriangleAlert className="size-4 text-amber-500" />}
            text={t(`errors.${comparison.limitationCode}`, { defaultValue: t('turnFiles.previewUnavailable') })}
          />
        ) : (
          <ReadonlyUnifiedDiff comparison={comparison} ariaLabel={t('turnFiles.diffPreview')} />
        )}
      </div>
    </section>
  );
}

function PreviewMessage({ icon, text }: { icon: ReactNode; text: string }) {
  return <div className="flex h-full items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">{icon}{text}</div>;
}

function commandErrorCode(reason: unknown) {
  return typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
    ? reason.code
    : 'turn-files.change-set-not-found';
}
