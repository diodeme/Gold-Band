import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { cn } from '@/lib/utils';

export function AgentIcon({
  iconKey,
  className = 'size-4',
}: {
  iconKey: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'relative inline-flex shrink-0 items-center justify-center overflow-hidden [contain:paint]',
        className,
      )}
      aria-hidden="true"
    >
      <img
        src={agentIconSrc(iconKey)}
        alt=""
        className={agentIconClass(iconKey, 'block size-full max-h-full max-w-full', { compensateWhitespace: false })}
      />
    </span>
  );
}

export function AgentIdentityLabel({
  iconKey,
  name,
  iconClassName = 'size-4',
  className,
}: {
  iconKey: string;
  name: string;
  iconClassName?: string;
  className?: string;
}) {
  return (
    <span className={cn('flex min-w-0 items-center gap-2', className)}>
      <AgentIcon iconKey={iconKey} className={iconClassName} />
      <span className="min-w-0 truncate">{name}</span>
    </span>
  );
}
