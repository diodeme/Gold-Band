import { useTranslation } from 'react-i18next';

import { acpSystemNoticeCopy } from '@/lib/acp-system-notice';
import type { AcpUiEventVm } from '@/types';

export function AcpSystemNoticeDivider({
  event,
}: {
  event: Pick<AcpUiEventVm, 'kind' | 'raw'>;
}) {
  const { t } = useTranslation();
  const copy = acpSystemNoticeCopy(t, event);
  if (!copy) return null;
  return (
    <div
      data-acp-system-notice="true"
      className="flex items-center gap-3 py-2 text-xs text-muted-foreground"
    >
      <span className="h-px min-w-4 flex-1 bg-border/70" />
      <span className="max-w-[min(40rem,calc(100%-3rem))] text-center leading-5 [overflow-wrap:anywhere]">
        {copy}
      </span>
      <span className="h-px min-w-4 flex-1 bg-border/70" />
    </div>
  );
}
