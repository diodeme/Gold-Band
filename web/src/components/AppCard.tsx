import type { ComponentProps } from 'react';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

export function AppCard({ className, ...props }: ComponentProps<typeof Card>) {
  return <Card className={cn('border-border/55 bg-card/70 shadow-none', className)} {...props} />;
}
