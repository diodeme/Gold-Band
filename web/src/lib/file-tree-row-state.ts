import { cn } from '@/lib/utils';

export function fileTreeRowStateClassName(selected: boolean, focused: boolean) {
  return cn(
    'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
    selected && 'bg-accent text-accent-foreground shadow-[inset_2px_0_0_var(--accent-foreground)] hover:bg-accent hover:text-accent-foreground',
    focused && !selected && 'ring-1 ring-inset ring-ring/60',
  );
}

export function fileTreeIconStateClassName(selected: boolean) {
  return selected
    ? 'text-accent-foreground'
    : 'text-foreground/65 group-hover:text-foreground';
}
