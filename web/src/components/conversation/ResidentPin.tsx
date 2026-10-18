import { useId, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

// 以主题蓝（running）为主色：钉头微混 emphasis 成靛蓝，中段一道高光形成金属反光，针尖向 accent-foreground 过渡成青色；各主题自动适配。
export const RESIDENT_PIN_GRADIENT_STOPS = [
  { offset: 0, color: 'color-mix(in oklab, var(--gold-running), var(--gold-emphasis) 25%)' },
  { offset: 0.35, color: 'var(--gold-running)' },
  { offset: 0.55, color: 'color-mix(in oklab, var(--gold-running), white 40%)' },
  { offset: 0.72, color: 'var(--gold-running)' },
  { offset: 1, color: 'color-mix(in oklab, var(--gold-running), var(--accent-foreground) 60%)' },
] as const;

// 造型取自 Fluent UI System Icons 的 pin-24-filled / pin-24-regular（MIT）；常驻时渐变沿钉头到针尖方向。
const PIN_FILLED_PATH = 'm21.068 7.758l-4.826-4.826a2.75 2.75 0 0 0-4.404.715l-2.435 4.87a.75.75 0 0 1-.426.374l-4.166 1.44a1.25 1.25 0 0 0-.476 2.065L7.439 15.5L3 19.94V21h1.06l4.44-4.44l3.105 3.105a1.25 1.25 0 0 0 2.065-.476l1.44-4.166a.75.75 0 0 1 .373-.426l4.87-2.435a2.75 2.75 0 0 0 .715-4.404';
const PIN_REGULAR_PATH = 'M16.2425 2.93189L21.0682 7.75765C22.3955 9.08491 22.0324 11.3224 20.3535 12.1619L15.4826 14.5973C15.3073 14.685 15.1732 14.8379 15.1092 15.0232L13.6699 19.1895C13.3684 20.0622 12.2574 20.3181 11.6045 19.6653L8.50002 16.5607L4.06074 21.0001H3L3.00008 19.9394L7.43936 15.5001L4.33487 12.3956C3.682 11.7427 3.93791 10.6317 4.81061 10.3302L8.97688 8.89096C9.16223 8.82694 9.31512 8.69287 9.40281 8.51748L11.8382 3.6466C12.6777 1.96772 14.9152 1.60462 16.2425 2.93189ZM20.0076 8.81831L15.1818 3.99255C14.5785 3.38924 13.5614 3.55429 13.1799 4.31742L10.7445 9.18829C10.4814 9.71446 10.0227 10.1167 9.46666 10.3087L5.67812 11.6175L12.3826 18.322L13.6914 14.5335C13.8835 13.9774 14.2857 13.5188 14.8118 13.2557L19.6827 10.8202C20.4458 10.4387 20.6109 9.42161 20.0076 8.81831Z';

/** Resident Direct conversation marker. `pinned={false}` is the outline glyph of an available toggle. */
export function ResidentPin({ pinned = true, className }: { pinned?: boolean; className?: string }) {
  const gradientId = `resident-pin-${useId().replace(/[^\w-]/g, '')}`;
  if (!pinned) {
    return (
      <svg data-resident-pin="off" viewBox="0 0 24 24" className={cn('size-3 shrink-0', className)} aria-hidden="true">
        <path fill="currentColor" d={PIN_REGULAR_PATH} />
      </svg>
    );
  }
  return (
    <svg data-resident-pin="on" viewBox="0 0 24 24" className={cn('size-3 shrink-0', className)} aria-hidden="true">
      <defs>
        <linearGradient id={gradientId} gradientUnits="userSpaceOnUse" x1="21" y1="3" x2="4" y2="20">
          {RESIDENT_PIN_GRADIENT_STOPS.map(({ offset, color }) => (
            <stop key={offset} offset={offset} style={{ stopColor: color }} />
          ))}
        </linearGradient>
      </defs>
      <path fill={`url(#${gradientId})`} d={PIN_FILLED_PATH} />
    </svg>
  );
}

export interface ResidentPinToggleProps {
  /** Canonical residency of the conversation, as reported by the backend. */
  pinned: boolean;
  /** Persists the requested residency; the caller applies the returned task and reports its own errors. */
  onChange: (resident: boolean) => Promise<void>;
}

/** Header toggle with the same effect as the sidebar's residency menu item. */
export function ResidentPinToggle({ pinned, onChange }: ResidentPinToggleProps) {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          size="sm"
          variant="ghost"
          className="size-5.5 p-0 text-muted-foreground hover:text-foreground"
          aria-label={t(pinned ? 'conversation.sidebar.releaseResident' : 'conversation.sidebar.keepResident')}
          aria-pressed={pinned}
          aria-busy={pending}
          disabled={pending}
          data-resident-toggle
          onClick={() => {
            setPending(true);
            void onChange(!pinned).finally(() => setPending(false));
          }}
        >
          {pending ? <Loader2 className="size-3 animate-spin" /> : <ResidentPin pinned={pinned} />}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="max-w-64">
        {t(pinned ? 'conversation.sidebar.releaseResidentHint' : 'conversation.sidebar.keepResidentHint')}
      </TooltipContent>
    </Tooltip>
  );
}
