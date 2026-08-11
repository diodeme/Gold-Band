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
    <div className={cn('space-y-4 px-5 pb-5', className)} data-conversation-intervention-layer="true">
      {children}
    </div>
  );
}

