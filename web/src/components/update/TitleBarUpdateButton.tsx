import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { UpdateStatusVm } from '@/types';
import { downloadPercent, useUpdateDownloadProgress } from './update-progress-store';
import { updateActionState } from './update-state';

export function TitleBarUpdateButton({ status, onOpen }: { status: UpdateStatusVm; onOpen: () => void }) {
  const { t } = useTranslation();
  const action = updateActionState(status);
  if (!action || !status.update) return null;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="mx-1 flex h-6 items-center rounded-full bg-gold-attention px-3 text-xs font-semibold text-gold-attention-foreground transition-opacity hover:opacity-90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          onClick={onOpen}
          data-titlebar-update-action={action}
        >
          {action === 'downloading' ? <DownloadingLabel /> : t(`settings.updater.titleBar.${action}`)}
        </button>
      </TooltipTrigger>
      <TooltipContent>{t('settings.updater.titleBar.tooltip', { version: status.update.version })}</TooltipContent>
    </Tooltip>
  );
}

function DownloadingLabel() {
  const { t } = useTranslation();
  const percent = downloadPercent(useUpdateDownloadProgress());
  return percent === null
    ? t('settings.updater.titleBar.downloading')
    : t('settings.updater.titleBar.downloadingPercent', { percent });
}
