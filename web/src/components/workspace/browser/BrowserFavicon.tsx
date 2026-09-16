import { useState } from 'react';
import { Globe } from 'lucide-react';
import { cn } from '@/lib/utils';

export function BrowserFavicon({
  src,
  className,
}: {
  src?: string | null;
  className?: string;
}) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  if (!src || failedSrc === src) {
    return <Globe className={cn('size-3.5 shrink-0 text-muted-foreground', className)} />;
  }
  return (
    <img
      src={src}
      alt=""
      data-browser-favicon="true"
      className={cn('size-3.5 shrink-0 rounded-sm', className)}
      onError={() => setFailedSrc(src)}
    />
  );
}
