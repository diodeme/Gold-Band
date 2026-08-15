import { cn } from '@/lib/utils';

interface BrandLoadingStateProps {
  label: string;
  className?: string;
  logoClassName?: string;
}

export function BrandLoadingState({
  label,
  className,
  logoClassName,
}: BrandLoadingStateProps) {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={label}
      className={cn(
        'flex h-full min-h-0 w-full items-center justify-center bg-background',
        className,
      )}
      data-brand-loading-state="true"
    >
      <div className="brand-loading-logo flex shrink-0 items-center justify-center">
        <img
          src="/logo.svg"
          alt=""
          aria-hidden="true"
          className={cn('h-auto w-20 select-none object-contain', logoClassName)}
          draggable={false}
        />
      </div>
      <span className="sr-only">{label}</span>
    </div>
  );
}
