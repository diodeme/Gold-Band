import { useTranslation } from 'react-i18next';

import { acpSystemNoticeCopy } from '@/lib/acp-system-notice';
import type { AcpUiEventVm } from '@/types';

export const ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT = {
  rootClassName: 'flex min-w-0 items-center gap-3 py-2 text-xs text-muted-foreground',
  lineClassName: 'h-px min-w-6 flex-1 bg-border',
  copyClassName: 'min-w-0 text-center leading-5 break-words',
} as const;

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
      className={ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.rootClassName}
    >
      <span className={ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName} />
      <span className={ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.copyClassName}>
        {copy}
      </span>
      <span className={ACP_SYSTEM_NOTICE_DIVIDER_LAYOUT.lineClassName} />
    </div>
  );
}
