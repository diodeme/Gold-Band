import type { ComponentProps, ReactNode } from 'react';
import { Card, CardContent } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

type PageProps = ComponentProps<'section'> & { flush?: boolean };

export type PageContentVariant = 'default' | 'after-navigation';

type PageContentProps = ComponentProps<'div'> & {
  variant?: PageContentVariant;
};

export type PageHeaderVariant = 'default' | 'integrated';

interface PageHeaderProps {
  breadcrumbs?: ReactNode;
  eyebrow?: ReactNode;
  icon?: ReactNode;
  title: ReactNode;
  badges?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  metrics?: ReactNode;
  navigation?: ReactNode;
  navigationLabel?: string;
  variant?: PageHeaderVariant;
  className?: string;
}

export const pageHeaderStyles = {
  default: {
    root: 'space-y-5 border-b bg-background/60 px-5 py-4 backdrop-blur xl:px-6',
    navigationRoot: '',
    headingRow: 'flex-col gap-4 lg:flex-row lg:items-start lg:justify-between',
    identity: 'space-y-3',
    title: 'text-3xl',
    actions: 'lg:pt-1',
  },
  integrated: {
    root: 'space-y-2 px-6 pb-3 pt-8',
    navigationRoot: 'pb-1',
    headingRow: 'flex-col gap-3 sm:flex-row sm:items-start sm:justify-between',
    identity: 'space-y-1.5',
    title: 'text-lg',
    actions: '',
  },
} as const satisfies Record<PageHeaderVariant, Record<string, string>>;

export const pageContentStyles = {
  default: 'min-h-0 flex-1 px-6 pb-6 pt-4',
  'after-navigation': 'min-h-0 flex-1 px-6 pb-6 pt-2',
} as const satisfies Record<PageContentVariant, string>;

export function Page({ children, className, flush = false, ...props }: PageProps) {
  return <section className={cn('min-h-0 flex-1 overflow-hidden', flush ? 'p-0' : 'overflow-y-auto p-8', className)} {...props}>{children}</section>;
}

export function PageContent({ children, className, variant = 'default', ...props }: PageContentProps) {
  return <div data-variant={variant} className={cn(pageContentStyles[variant], className)} {...props}>{children}</div>;
}

export function PageScroll({ children, className }: { children: ReactNode; className?: string }) {
  return <ScrollArea className={cn('h-full', className)}>{children}</ScrollArea>;
}

export function PageHeader({ breadcrumbs, eyebrow, icon, title, badges, subtitle, actions, metrics, navigation, navigationLabel, variant = 'default', className }: PageHeaderProps) {
  const styles = pageHeaderStyles[variant];
  return (
    <header data-variant={variant} className={cn('shrink-0', styles.root, navigation && styles.navigationRoot, className)}>
      {breadcrumbs ? <div className="flex min-h-6 min-w-0 items-center">{breadcrumbs}</div> : null}
      <div className={cn('flex', styles.headingRow)}>
        <div data-slot="page-header-identity" className="flex min-w-0 items-center gap-3">
          {icon ? (
            <span data-slot="page-header-icon" aria-hidden="true" className="shrink-0 text-foreground [&_svg]:size-5">
              {icon}
            </span>
          ) : null}
          <div className={cn('min-w-0', styles.identity)}>
            {eyebrow ? <p className="truncate text-xs font-semibold uppercase tracking-[0.22em] text-primary">{eyebrow}</p> : null}
            <div className="flex min-w-0 flex-wrap items-center gap-3">
              <h1 className={cn('min-w-0 truncate font-semibold tracking-tight text-foreground', styles.title)}>{title}</h1>
              {badges ? <div className="flex shrink-0 flex-wrap items-center gap-2">{badges}</div> : null}
            </div>
            {subtitle ? <div className="max-w-4xl text-sm leading-6 text-muted-foreground">{subtitle}</div> : null}
          </div>
        </div>
        {actions ? <Actions className={styles.actions}>{actions}</Actions> : null}
      </div>
      {navigation ? <nav className="min-w-0" aria-label={navigationLabel}>{navigation}</nav> : null}
      {metrics ? <div className="min-w-0">{metrics}</div> : null}
    </header>
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

export function Metric({ label, value, tooltip, compact = false, className }: { label: ReactNode; value: ReactNode; tooltip?: ReactNode; compact?: boolean; className?: string }) {
  const valueNode = <strong className="block truncate text-sm text-foreground">{value}</strong>;
  return (
    <Card className={cn('h-full gap-2 border-border/45 bg-card/45 py-4 shadow-none', compact && 'py-3', className)}>
      <CardContent className={cn('flex h-full flex-col justify-between gap-1 px-4', compact && 'px-3')}>
        <span className="block text-xs uppercase tracking-[0.16em] text-muted-foreground">{label}</span>
        {tooltip ? (
          <Tooltip>
            <TooltipTrigger asChild>{valueNode}</TooltipTrigger>
            <TooltipContent className="max-w-[360px] whitespace-pre-wrap break-words" sideOffset={6}>{tooltip}</TooltipContent>
          </Tooltip>
        ) : valueNode}
      </CardContent>
    </Card>
  );
}

export function OverflowTooltip({ children, content, className, contentClassName, sideOffset = 6 }: { children: ReactNode; content: ReactNode; className?: string; contentClassName?: string; sideOffset?: number }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div className={className}>{children}</div>
      </TooltipTrigger>
      <TooltipContent className={cn('max-w-[360px] whitespace-pre-wrap break-words', contentClassName)} sideOffset={sideOffset}>
        {content}
      </TooltipContent>
    </Tooltip>
  );
}

export function EmptyState({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('grid min-h-28 place-items-center rounded-lg border border-dashed border-border bg-muted/20 p-6 text-center text-sm text-muted-foreground', className)}>{children}</div>;
}

export function CodeBlock({ children, className }: { children: ReactNode; className?: string }) {
  return <pre className={cn('max-w-full overflow-auto rounded-lg border bg-gold-surface-low p-4 font-mono text-xs leading-6 text-foreground', className)}>{children}</pre>;
}
