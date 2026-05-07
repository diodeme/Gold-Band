import type { ComponentProps, ReactNode } from 'react';
import { Card, CardContent } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { cn } from '@/lib/utils';

type PageProps = ComponentProps<'section'> & { flush?: boolean };

export function Page({ children, className, flush = false, ...props }: PageProps) {
  return <section className={cn('min-h-0 flex-1 overflow-hidden', flush ? 'p-0' : 'overflow-y-auto p-8', className)} {...props}>{children}</section>;
}

export function PageScroll({ children, className }: { children: ReactNode; className?: string }) {
  return <ScrollArea className={cn('h-full', className)}>{children}</ScrollArea>;
}

export function PageHeader({ eyebrow, title, subtitle, actions, className }: { eyebrow?: ReactNode; title: ReactNode; subtitle?: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex items-start justify-between gap-6', className)}>
      <div className="min-w-0 space-y-2">
        {eyebrow ? <p className="font-mono text-xs uppercase tracking-[0.22em] text-primary">{eyebrow}</p> : null}
        <h1 className="truncate text-3xl font-semibold tracking-tight text-foreground">{title}</h1>
        {subtitle ? <div className="max-w-4xl text-sm leading-6 text-muted-foreground">{subtitle}</div> : null}
      </div>
      {actions ? <Actions>{actions}</Actions> : null}
    </div>
  );
}

export function ModuleBar({ title, tabs, actions, className }: { title: ReactNode; tabs?: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex min-h-16 min-w-0 items-center justify-between gap-4 border-b bg-background/60 px-5 backdrop-blur xl:px-6', className)}>
      <div className="flex min-w-0 flex-wrap items-center gap-3">
        <strong className="whitespace-nowrap text-sm text-foreground">{title}</strong>
        {tabs ? <Separator orientation="vertical" className="h-6" /> : null}
        <div className="min-w-0">{tabs}</div>
      </div>
      {actions ? <Actions>{actions}</Actions> : null}
    </div>
  );
}

export function Actions({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('flex shrink-0 flex-wrap items-center justify-end gap-2', className)}>{children}</div>;
}

export function MetricsBar({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-5', className)}>{children}</div>;
}

export function Metric({ label, value, compact = false, className }: { label: ReactNode; value: ReactNode; compact?: boolean; className?: string }) {
  return (
    <Card className={cn('gap-2 border-border/80 bg-muted/35 py-4', compact && 'py-3', className)}>
      <CardContent className={cn('space-y-1 px-4', compact && 'px-3')}>
        <span className="block text-xs uppercase tracking-[0.16em] text-muted-foreground">{label}</span>
        <strong className="block truncate text-sm text-foreground">{value}</strong>
      </CardContent>
    </Card>
  );
}

export function EmptyState({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('grid min-h-28 place-items-center rounded-lg border border-dashed border-border bg-muted/20 p-6 text-center text-sm text-muted-foreground', className)}>{children}</div>;
}

export function CodeBlock({ children, className }: { children: ReactNode; className?: string }) {
  return <pre className={cn('max-w-full overflow-auto rounded-lg border bg-gold-surface-low p-4 font-mono text-xs leading-6 text-foreground', className)}>{children}</pre>;
}
