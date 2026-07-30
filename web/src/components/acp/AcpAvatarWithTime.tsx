import { cn } from '@/lib/utils';
import { AvatarDisplay } from '@/components/avatar/AvatarDisplay';
import { useAvatarPreferences } from '@/components/avatar/AvatarPreferencesContext';

interface AcpAvatarWithTimeProps {
  tone: 'assistant' | 'user';
  timestamp?: string | null;
  className?: string;
}

function parseAcpTimestampMs(value: string): number | null {
  // ACP timestamps are Unix epoch seconds with optional "Z" suffix, e.g. "1778771541Z"
  const numeric = value.match(/^(\d+(?:\.\d+)?)Z?$/);
  if (numeric) return Number(numeric[1]) * 1000;
  // Fallback: ISO 8601 or other Date-parseable format
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function formatMessageTime(raw?: string | null): string {
  if (!raw) return '--:--';
  try {
    const ms = parseAcpTimestampMs(raw);
    if (ms == null) return '--:--';
    const date = new Date(ms);
    const hours = date.getHours().toString().padStart(2, '0');
    const minutes = date.getMinutes().toString().padStart(2, '0');
    return `${hours}:${minutes}`;
  } catch {
    return '--:--';
  }
}

export function AcpAvatarWithTime({ tone, timestamp, className }: AcpAvatarWithTimeProps) {
  const avatars = useAvatarPreferences();
  const kind = tone === 'assistant' ? 'agent' : 'user';
  const profile = avatars[kind];

  return (
    <div className={cn('flex shrink-0 flex-col items-center gap-1', className)}>
      <AvatarDisplay
        kind={kind}
        profile={profile}
        className={cn(
          'mt-0.5 size-9',
          tone === 'assistant'
            ? 'bg-card text-muted-foreground'
            : 'border-[color-mix(in_srgb,var(--primary)_24%,var(--border))] bg-[color-mix(in_srgb,var(--primary)_12%,var(--card))] text-[color-mix(in_srgb,var(--primary)_72%,white)]',
        )}
        fallbackClassName={tone === 'assistant'
          ? 'bg-card text-muted-foreground'
          : 'bg-[color-mix(in_srgb,var(--primary)_12%,var(--card))] text-[color-mix(in_srgb,var(--primary)_72%,white)]'}
      />
      <span className="text-[10px] text-muted-foreground/60 leading-none dark:text-muted-foreground/50">
        {formatMessageTime(timestamp)}
      </span>
    </div>
  );
}

export { formatMessageTime, parseAcpTimestampMs };
