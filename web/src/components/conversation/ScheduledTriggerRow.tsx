import { AlarmClock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { ScheduledTriggerPayloadVm } from '@/types';

export function ScheduledTriggerRow({ payload, onOpen }: { payload: ScheduledTriggerPayloadVm; onOpen: () => void }) {
  const { t } = useTranslation();
  const label = t(`scheduled.trigger.${payload.triggerKind}`, { defaultValue: payload.triggerKind });
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" onClick={onOpen} className="flex w-full min-w-0 items-center gap-2 border-y border-border/45 px-1 py-2 text-left text-xs text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
          <AlarmClock className="size-3.5 shrink-0 text-foreground" aria-hidden="true" />
          <span className="shrink-0 font-medium text-foreground">{label}</span>
          <span className="min-w-0 truncate">{payload.instructionSummary}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent>{payload.instructionSummary}</TooltipContent>
    </Tooltip>
  );
}
