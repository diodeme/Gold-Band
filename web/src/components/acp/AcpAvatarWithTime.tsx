import { cn } from '@/lib/utils';
import { AvatarDisplay } from '@/components/avatar/AvatarDisplay';
import { useAvatarPreferences } from '@/components/avatar/AvatarPreferencesContext';
import { parseTimestamp } from '@/lib/datetime';

interface AcpAvatarWithTimeProps {
  tone: 'assistant' | 'user';
  timestamp?: string | null;
  className?: string;
}

interface AcpAvatarProps {
  tone: 'assistant' | 'user';
  className?: string;
  fallbackClassName?: string;
}

function parseAcpTimestampMs(value: string): number | null {
  return parseTimestamp(value)?.getTime() ?? null;
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

export function AcpAvatar({ tone, className, fallbackClassName }: AcpAvatarProps) {
  const avatars = useAvatarPreferences();
  const kind = tone === 'assistant' ? 'agent' : 'user';
  const profile = avatars[kind];

  return (
    <AvatarDisplay
      kind={kind}
      profile={profile}
      className={cn(
        'mt-0.5 size-9',
        tone === 'assistant'
          ? 'bg-card text-muted-foreground'
          : 'border-[color-mix(in_srgb,var(--primary)_24%,var(--border))] bg-[color-mix(in_srgb,var(--primary)_12%,var(--card))] text-[color-mix(in_srgb,var(--primary)_72%,white)]',
        className,
      )}
      fallbackClassName={cn(
        tone === 'assistant'
          ? 'bg-card text-muted-foreground'
          : 'bg-[color-mix(in_srgb,var(--primary)_12%,var(--card))] text-[color-mix(in_srgb,var(--primary)_72%,white)]',
        fallbackClassName,
      )}
    />
  );
}

export function AcpAvatarWithTime({ tone, timestamp, className }: AcpAvatarWithTimeProps) {

  return (
    <div className={cn('flex shrink-0 flex-col items-center gap-1', className)}>
      <AcpAvatar tone={tone} />
      <span className="text-ui-micro text-muted-foreground/60 leading-none dark:text-muted-foreground/50">
        {formatMessageTime(timestamp)}
      </span>
    </div>
  );
}

export { formatMessageTime, parseAcpTimestampMs };
