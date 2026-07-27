import { cva } from 'class-variance-authority';

export const acpComposerConfigTriggerVariants = cva(
  'inline-flex h-9 w-auto min-w-0 items-center justify-between whitespace-nowrap rounded-full border px-3 py-0 text-xs font-normal text-foreground shadow-none outline-none transition-[color,box-shadow] focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10 disabled:cursor-not-allowed disabled:opacity-50 [&>svg]:size-3.5 [&>svg]:shrink-0 [&>svg]:text-muted-foreground [&>svg]:opacity-100',
  {
    variants: {
      compact: {
        true: 'max-w-[min(22rem,100%)] gap-1.5 border-border/60 bg-background/50 hover:bg-background/70 dark:bg-background/50 dark:hover:bg-background/70',
        false: 'min-w-[130px] max-w-[220px] flex-1 gap-2 border-border/50 bg-gold-surface-high/35 hover:bg-gold-surface-high/55 dark:bg-gold-surface-high/35 dark:hover:bg-gold-surface-high/55',
      },
    },
    defaultVariants: {
      compact: false,
    },
  },
);

export const ACP_COMPOSER_CONFIG_TRIGGER_LABEL_CLASS = 'shrink-0 text-muted-foreground';
export const ACP_COMPOSER_CONFIG_TRIGGER_VALUE_CLASS = 'min-w-0 flex-1 truncate text-left text-foreground';
export const ACP_COMPOSER_CONFIG_TRIGGER_ICON_CLASS = 'size-3.5 shrink-0 text-muted-foreground';
