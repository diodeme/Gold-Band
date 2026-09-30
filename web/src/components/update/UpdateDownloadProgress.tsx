import { useTranslation } from 'react-i18next';
import { Progress } from '@/components/ui/progress';
import { cn } from '@/lib/utils';
import { downloadPercent, useUpdateDownloadProgress } from './update-progress-store';
import { formatBytes } from './update-state';

export function UpdateDownloadProgress({ className }: { className?: string }) {
  const { t } = useTranslation();
  const progress = useUpdateDownloadProgress();
  const percent = downloadPercent(progress);
  return (
    <div className={cn('flex min-w-0 items-center gap-2 text-xs text-muted-foreground', className)} data-update-download-progress="true">
      <Progress value={percent ?? 0} className="h-1.5 flex-1 bg-secondary" indicatorClassName="bg-gold-attention" />
      <span className="shrink-0 tabular-nums">
        {progress && progress.total != null
          ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`
          : t('settings.updater.downloaded', { size: formatBytes(progress?.downloaded ?? 0) })}
      </span>
    </div>
  );
}
