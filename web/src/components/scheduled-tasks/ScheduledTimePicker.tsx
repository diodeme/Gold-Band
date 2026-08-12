import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Clock3 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';

const hours = Array.from({ length: 24 }, (_, value) => String(value).padStart(2, '0'));
const minutes = Array.from({ length: 60 }, (_, value) => String(value).padStart(2, '0'));

function timeParts(value: string) {
  const match = /^(\d{2}):(\d{2})$/.exec(value);
  return {
    hour: match && hours.includes(match[1]) ? match[1] : '00',
    minute: match && minutes.includes(match[2]) ? match[2] : '00',
  };
}

export function normalizeScheduledTimeInput(value: string) {
  const trimmed = value.trim();
  const colonMatch = /^(\d{1,2}):(\d{1,2})$/.exec(trimmed);
  const compactMatch = /^(\d{3,4})$/.exec(trimmed);
  const match = colonMatch ?? (compactMatch
    ? [compactMatch[0], compactMatch[1].slice(0, -2), compactMatch[1].slice(-2)]
    : null);
  if (!match) return null;

  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (!Number.isInteger(hour) || hour < 0 || hour > 23 || !Number.isInteger(minute) || minute < 0 || minute > 59) {
    return null;
  }
  return `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
}

function isTimeInputDraft(value: string) {
  return /^\d{0,2}:?\d{0,2}$/.test(value) || /^\d{0,4}$/.test(value);
}

export function ScheduledTimePicker({
  value,
  onValueChange,
  invalid = false,
}: {
  value: string;
  onValueChange: (value: string) => void;
  invalid?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const [draftInvalid, setDraftInvalid] = useState(false);
  const pickerId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const { hour, minute } = useMemo(() => timeParts(value), [value]);

  useEffect(() => {
    setDraft(value);
    setDraftInvalid(false);
  }, [value]);

  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() => {
      for (const column of panelRef.current?.querySelectorAll<HTMLElement>('[data-time-column]') ?? []) {
        const viewport = column.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]');
        const selected = column.querySelector<HTMLElement>('[data-selected="true"]');
        if (!viewport || !selected) continue;
        viewport.scrollTop = Math.max(0, selected.offsetTop - (viewport.clientHeight - selected.offsetHeight) / 2);
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [hour, minute, open]);

  const commitDraft = () => {
    const normalized = normalizeScheduledTimeInput(draft);
    if (!normalized) {
      setDraftInvalid(true);
      return false;
    }
    setDraft(normalized);
    setDraftInvalid(false);
    if (normalized !== value) onValueChange(normalized);
    return true;
  };

  const chooseTime = (nextValue: string) => {
    setDraft(nextValue);
    setDraftInvalid(false);
    onValueChange(nextValue);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <div className="relative">
        <Input
          value={draft}
          type="text"
          inputMode="numeric"
          autoComplete="off"
          maxLength={5}
          aria-label={t('scheduled.dialog.time')}
          aria-controls={pickerId}
          aria-expanded={open}
          aria-haspopup="dialog"
          aria-invalid={invalid || draftInvalid}
          placeholder="HH:mm"
          className="pr-9 font-normal tabular-nums"
          onChange={(event) => {
            const nextDraft = event.target.value;
            if (!isTimeInputDraft(nextDraft)) return;
            setDraft(nextDraft);
            setDraftInvalid(false);
            if (/^\d{2}:\d{2}$/.test(nextDraft)) {
              const normalized = normalizeScheduledTimeInput(nextDraft);
              if (normalized && normalized !== value) onValueChange(normalized);
            }
          }}
          onBlur={commitDraft}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              if (commitDraft()) setOpen(false);
            } else if (event.key === 'Escape') {
              setDraft(value);
              setDraftInvalid(false);
              setOpen(false);
            } else if (event.key === 'ArrowDown') {
              setOpen(true);
            }
          }}
        />
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t('scheduled.dialog.openTimePicker')}
            className="absolute right-1 top-1 size-7 text-muted-foreground hover:text-foreground"
          >
            <Clock3 className="size-4" />
          </Button>
        </PopoverTrigger>
      </div>
      <PopoverContent
        id={pickerId}
        align="end"
        className="w-48 p-2"
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        <div ref={panelRef} className="grid grid-cols-2 gap-1" role="group" aria-label={t('scheduled.dialog.time')}>
          <div data-time-column>
            <div className="px-2 pb-1 text-center text-[11px] text-muted-foreground">{t('scheduled.dialog.hours')}</div>
            <ScrollArea className="h-52">
              <div className="space-y-0.5 pr-2">
                {hours.map((option) => (
                  <Button
                    key={option}
                    type="button"
                    variant="ghost"
                    size="sm"
                    data-selected={option === hour}
                    aria-pressed={option === hour}
                    className={cn('h-8 w-full px-2 tabular-nums', option === hour && 'bg-accent text-accent-foreground')}
                    onClick={() => chooseTime(`${option}:${minute}`)}
                  >
                    {option}
                  </Button>
                ))}
              </div>
            </ScrollArea>
          </div>
          <div data-time-column>
            <div className="px-2 pb-1 text-center text-[11px] text-muted-foreground">{t('scheduled.dialog.minutes')}</div>
            <ScrollArea className="h-52">
              <div className="space-y-0.5 pr-2">
                {minutes.map((option) => (
                  <Button
                    key={option}
                    type="button"
                    variant="ghost"
                    size="sm"
                    data-selected={option === minute}
                    aria-pressed={option === minute}
                    className={cn('h-8 w-full px-2 tabular-nums', option === minute && 'bg-accent text-accent-foreground')}
                    onClick={() => {
                      chooseTime(`${hour}:${option}`);
                      setOpen(false);
                    }}
                  >
                    {option}
                  </Button>
                ))}
              </div>
            </ScrollArea>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
