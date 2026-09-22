import { useEffect, useId, useLayoutEffect, useMemo, useRef, type ReactNode } from 'react';
import { Command, CommandGroup, CommandItem, CommandList } from '@/components/ui/command';
import { Popover, PopoverAnchor, PopoverContent } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import {
  type SlashCatalogGroup,
  commandSlashItems,
  flattenSlashCatalog,
  getScrollTopForActiveSlashCommand,
  slashCatalogItemValue,
} from '@/lib/slash-command';
import type { AcpCommandItemVm } from '@/types';

interface SlashCommandMenuProps {
  open: boolean;
  groups?: readonly SlashCatalogGroup[];
  commands?: readonly AcpCommandItemVm[];
  activeIndex: number;
  onActiveIndexChange: (index: number) => void;
  onDismiss: () => void;
  onSelect: (index: number) => void;
  variant?: 'popover' | 'inline';
  children: ReactNode;
}

const COMMAND_ROW_HEIGHT_PX = 36;
const COMMAND_GROUP_VERTICAL_PADDING_PX = 4;
const COMMAND_GROUP_HEADING_HEIGHT_PX = 28;
const COMMAND_MENU_MAX_HEIGHT_PX = 266;

function menuGroups(
  groups: readonly SlashCatalogGroup[] | undefined,
  commands: readonly AcpCommandItemVm[] | undefined,
): readonly SlashCatalogGroup[] {
  if (groups) return groups;
  const items = commandSlashItems(commands ?? []);
  return items.length > 0 ? [{ id: 'agent', heading: 'Agent', items }] : [];
}

export function SlashCommandMenu({
  open,
  groups,
  commands,
  activeIndex,
  onActiveIndexChange,
  onDismiss,
  onSelect,
  variant = 'popover',
  children,
}: SlashCommandMenuProps) {
  const catalog = useMemo(() => menuGroups(groups, commands), [commands, groups]);
  const items = useMemo(() => flattenSlashCatalog(catalog), [catalog]);
  const menuId = useId();
  const inlineRootRef = useRef<HTMLDivElement>(null);
  const commandListRef = useRef<HTMLDivElement>(null);
  const commandItemRefs = useRef<Array<HTMLDivElement | null>>([]);
  const headingCount = catalog.filter((group) => group.heading.trim()).length;
  const menuHeight = Math.min(
    Math.max(items.length, 1) * COMMAND_ROW_HEIGHT_PX
      + headingCount * COMMAND_GROUP_HEADING_HEIGHT_PX
      + COMMAND_GROUP_VERTICAL_PADDING_PX * Math.max(headingCount, 1),
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
    const nextScrollTop = getScrollTopForActiveSlashCommand({
      containerScrollTop: scrollContainer.scrollTop,
      containerHeight: scrollContainer.clientHeight,
      itemOffsetTop,
      itemOffsetHeight: itemRect.height,
    });
    if (scrollContainer.scrollTop !== nextScrollTop) {
      scrollContainer.scrollTop = nextScrollTop;
    }
  }, [activeIndex, items, open]);

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

  const activeValue = items[activeIndex] ? slashCatalogItemValue(items[activeIndex]) : undefined;
  let itemOffset = 0;
  const commandMenu = (
    <Command
      shouldFilter={false}
      value={activeValue}
      disablePointerSelection
      className={cn(variant === 'inline' && 'bg-transparent')}
    >
      <CommandList
        ref={commandListRef}
        style={{ height: menuHeight }}
        className="gold-themed-scrollbar max-h-none overscroll-contain"
        onPointerMove={(event) => {
          const row = event.target instanceof Element
            ? event.target.closest('[data-slash-index]')
            : null;
          if (!(row instanceof HTMLElement)) return;
          const index = Number(row.dataset.slashIndex);
          if (Number.isInteger(index) && index !== activeIndex) onActiveIndexChange(index);
        }}
      >
        {catalog.map((group) => {
          const start = itemOffset;
          itemOffset += group.items.length;
          return (
            <CommandGroup
              key={group.id}
              heading={group.heading || undefined}
              data-slash-group={group.id}
              className="p-0.5"
            >
              {group.items.map((item, groupIndex) => {
                const index = start + groupIndex;
                return (
                  <CommandItem
                    ref={(element) => {
                      commandItemRefs.current[index] = element;
                    }}
                    key={slashCatalogItemValue(item)}
                    id={`${menuId}-item-${index}`}
                    value={slashCatalogItemValue(item)}
                    data-slash-item-kind={item.kind}
                    data-slash-index={index}
                    data-slash-active={index === activeIndex ? 'true' : undefined}
                    className={cn(
                      'grid h-9 grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)_auto] items-center gap-2 rounded-lg px-3 py-0 text-ui-compact transition-[background-color,box-shadow] before:absolute before:inset-y-2 before:left-1 before:w-0.5 before:rounded-full before:bg-primary/60 before:opacity-0 data-[selected=true]:bg-transparent data-[selected=true]:text-foreground data-[slash-active=true]:bg-primary/[0.07] data-[slash-active=true]:text-foreground data-[slash-active=true]:ring-1 data-[slash-active=true]:ring-inset data-[slash-active=true]:ring-primary/15 data-[slash-active=true]:before:opacity-100 dark:before:bg-foreground/65 dark:data-[slash-active=true]:bg-foreground/[0.10] dark:data-[slash-active=true]:ring-foreground/15',
                    )}
                    onMouseDown={(event) => event.preventDefault()}
                    onSelect={() => onSelect(index)}
                  >
                    <span className="truncate font-medium text-foreground">
                      {item.kind === 'role' ? item.name : `/${item.name}`}
                    </span>
                    <span className="min-w-0 truncate text-xs text-muted-foreground/90">
                      {item.description}
                    </span>
                    {item.inputHint ? (
                      <span className="max-w-44 shrink-0 truncate rounded-full border border-border/50 bg-muted/55 px-2 py-0.5 text-ui-micro leading-4 text-muted-foreground">
                        {item.inputHint}
                      </span>
                    ) : <span />}
                  </CommandItem>
                );
              })}
            </CommandGroup>
          );
        })}
      </CommandList>
    </Command>
  );

  if (variant === 'inline') {
    return (
      <div ref={inlineRootRef} className={cn('relative min-w-0', open && 'z-50')}>
        {children}
        {open ? (
          <div
            data-slot="slash-command-menu"
            className="absolute inset-x-[-1rem] top-8 z-50 w-[calc(100%+2rem)] rounded-xl border border-border/45 bg-popover px-2 py-1 text-popover-foreground shadow-[0_18px_42px_-20px_rgba(0,0,0,0.42)]"
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
        className="w-[var(--radix-popover-trigger-width)] max-w-[calc(100vw-1rem)] rounded-xl border-border/50 bg-popover p-1.5 text-popover-foreground shadow-[0_18px_48px_-20px_rgba(0,0,0,0.45)]"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        {commandMenu}
      </PopoverContent>
    </Popover>
  );
}
