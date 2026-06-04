import { Bot, User } from 'lucide-react';
import { cn } from '@/lib/utils';

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
  const Icon = tone === 'assistant' ? Bot : User;

  return (
    <div className={cn('flex flex-col items-center gap-0.5 shrink-0', className)}>
      <div className={cn(
        'mt-1 flex size-7 shrink-0 items-center justify-center rounded-full border',
        tone === 'assistant' ? 'bg-card text-muted-foreground' : 'bg-primary/10 text-primary',
      )}>
        <Icon className="size-3.5" />
      </div>
      <span className="text-[10px] text-muted-foreground/60 leading-none dark:text-muted-foreground/50">
        {formatMessageTime(timestamp)}
      </span>
    </div>
  );
}

export { formatMessageTime, parseAcpTimestampMs };
