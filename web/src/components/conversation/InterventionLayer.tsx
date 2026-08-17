import type { ReactNode } from 'react';

import { cn } from '@/lib/utils';

export function InterventionLayer({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'mx-auto w-full max-w-[var(--conversation-content-rail-max-inline-size)] space-y-4 px-5 pb-10',
        className,
      )}
      data-conversation-intervention-layer="true"
    >
      {children}
    </div>
  );
}
