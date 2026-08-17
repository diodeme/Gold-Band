import { useEffect, useId, useLayoutEffect, useRef, type ReactNode } from 'react';
import { Command, CommandGroup, CommandItem, CommandList } from '@/components/ui/command';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import { getScrollTopForActiveSlashCommand } from '@/lib/slash-command';
import type { AcpCommandItemVm } from '@/types';

interface SlashCommandMenuProps {
  open: boolean;
  commands: readonly AcpCommandItemVm[];
  activeIndex: number;
  onActiveIndexChange: (index: number) => void;
  onDismiss: () => void;
  onSelect: (index: number) => void;
  variant?: 'popover' | 'inline';
  children: ReactNode;
}

const COMMAND_ROW_HEIGHT_PX = 36;
const COMMAND_GROUP_VERTICAL_PADDING_PX = 4;
const COMMAND_MENU_MAX_HEIGHT_PX = 266;

export function SlashCommandMenu({
  open,
  commands,
  activeIndex,
  onActiveIndexChange,
  onDismiss,
  onSelect,
  variant = 'popover',
  children,
}: SlashCommandMenuProps) {
  const activeValue = commands[activeIndex]?.name;
  const menuId = useId();
  const inlineRootRef = useRef<HTMLDivElement>(null);
  const commandListRef = useRef<HTMLDivElement>(null);
  const commandItemRefs = useRef<Array<HTMLDivElement | null>>([]);
  const menuHeight = Math.min(
    Math.max(commands.length, 1) * COMMAND_ROW_HEIGHT_PX + COMMAND_GROUP_VERTICAL_PADDING_PX,
    COMMAND_MENU_MAX_HEIGHT_PX,
  );

  useLayoutEffect(() => {
    if (!open) return;
    const scrollContainer = commandListRef.current;
    const item = commandItemRefs.current[activeIndex];
    if (!scrollContainer || !item) return;

    const containerRect = scrollContainer.getBoundingClientRect();
    const itemRect = item.getBoundingClientRect();
    const itemOffsetTop = scrollContainer.scrollTop + itemRect.top - containerRect.top;
    scrollContainer.scrollTop = getScrollTopForActiveSlashCommand({
      containerScrollTop: scrollContainer.scrollTop,
      containerHeight: scrollContainer.clientHeight,
      itemOffsetTop,
      itemOffsetHeight: itemRect.height,
    });
  }, [activeIndex, commands, open]);

  useEffect(() => {
    if (!open || variant !== 'inline') return undefined;
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && !inlineRootRef.current?.contains(target)) {
        onDismiss();
      }
    };
    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => document.removeEventListener('pointerdown', handlePointerDown, true);
  }, [onDismiss, open, variant]);

  const commandMenu = (
    <Command
      shouldFilter={false}
      value={activeValue}
      className={cn(variant === 'inline' && 'bg-transparent')}
      onValueChange={(value) => {
        const index = commands.findIndex((command) => command.name === value);
        if (index >= 0) onActiveIndexChange(index);
      }}
    >
      <CommandList
        ref={commandListRef}
        style={{ height: menuHeight }}
        className="gold-themed-scrollbar max-h-none overscroll-contain"
      >
        <CommandGroup className="p-0.5">
          {commands.map((command, index) => (
            <CommandItem
              ref={(item) => {
                commandItemRefs.current[index] = item;
              }}
              key={command.name}
              id={`${menuId}-item-${index}`}
              value={command.name}
              className={cn(
                'grid h-9 grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)_auto] items-center gap-3 rounded-lg px-3 py-0 text-ui-compact transition-[background-color,box-shadow] before:absolute before:inset-y-2 before:left-1 before:w-0.5 before:rounded-full before:bg-primary/60 before:opacity-0 data-[selected=true]:bg-primary/[0.07] data-[selected=true]:text-foreground data-[selected=true]:ring-1 data-[selected=true]:ring-inset data-[selected=true]:ring-primary/15 data-[selected=true]:before:opacity-100 dark:before:bg-foreground/65 dark:data-[selected=true]:bg-foreground/[0.10] dark:data-[selected=true]:ring-foreground/15',
                index === activeIndex && 'bg-primary/[0.07] text-foreground ring-1 ring-inset ring-primary/15 before:opacity-100 dark:bg-foreground/[0.10] dark:ring-foreground/15',
              )}
              onMouseDown={(event) => event.preventDefault()}
              onSelect={() => onSelect(index)}
            >
              <span className="truncate font-medium text-foreground">
                /{command.name}
              </span>
              <span className="min-w-0 truncate text-xs text-muted-foreground/90">
                {command.description}
              </span>
              {command.inputHint ? (
                <span className="max-w-44 shrink-0 truncate rounded-full border border-border/50 bg-muted/55 px-2 py-0.5 text-ui-micro leading-4 text-muted-foreground">
                  {command.inputHint}
                </span>
              ) : <span />}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
    </Command>
  );

  if (variant === 'inline') {
    return (
      <div ref={inlineRootRef} className="relative min-w-0">
        {children}
        {open ? (
          <div
            data-slot="slash-command-menu"
            className="absolute inset-x-[-1rem] top-8 z-50 w-[calc(100%+2rem)] rounded-xl border border-border/45 bg-popover/98 px-2 py-1 shadow-[0_18px_42px_-20px_rgba(0,0,0,0.42)] backdrop-blur-xl"
          >
            {commandMenu}
          </div>
        ) : null}
      </div>
    );
  }

  return (
    <Popover open={open} onOpenChange={(nextOpen) => {
      if (!nextOpen) onDismiss();
    }}>
      <PopoverAnchor asChild>{children}</PopoverAnchor>
      <PopoverContent
        data-slot="slash-command-menu"
        side="top"
        align="start"
        sideOffset={8}
        className="w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-1rem)] rounded-xl border-border/50 bg-popover/95 p-1.5 shadow-[0_18px_48px_-20px_rgba(0,0,0,0.45)] backdrop-blur-xl"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        {commandMenu}
      </PopoverContent>
    </Popover>
  );
}
