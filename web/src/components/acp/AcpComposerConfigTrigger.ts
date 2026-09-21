import { cva } from 'class-variance-authority';
import { isOverflowing, useOverflowTooltip } from '@/hooks/useOverflowTooltip';

export const DEFAULT_ACP_COMPOSER_CONFIG_ALIGN = 'start' as const;
export const ACP_COMPOSER_CONFIG_DROPDOWN_MODAL = false;

/** Keep related ACP configuration choices in the same menu interaction. */
export function keepAcpConfigMenuOpenOnSelect(event: Event) {
  event.preventDefault();
}

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

export function formatAcpCompositeSelectionParts(
  names: Array<string | null | undefined>,
  unspecifiedLabel: string,
) {
  const parts = names.map((name) => name?.trim() || null);
  const selected = parts.filter((name): name is string => Boolean(name));
  if (selected.length === 0) return unspecifiedLabel;
  if (parts[0]) return [parts[0], ...parts.slice(1).filter((name): name is string => Boolean(name))].join(' · ');
  return [unspecifiedLabel, ...selected].join(' · ');
}

export function formatAcpCompositeSelection(
  primaryName: string | null | undefined,
  secondaryName: string | null | undefined,
  unspecifiedLabel: string,
) {
  return formatAcpCompositeSelectionParts([primaryName, secondaryName], unspecifiedLabel);
}

export function isAcpComposerConfigValueOverflowing(element: HTMLElement | null) {
  return isOverflowing(element);
}

/** Keep overflow measurement local to the hovered/focused composer control. */
export function useAcpComposerConfigOverflowTooltip() {
  return useOverflowTooltip<HTMLSpanElement>();
}
