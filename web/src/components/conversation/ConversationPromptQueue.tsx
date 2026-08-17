import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import {
  ChevronDown,
  CornerDownLeft,
  GripVertical,
  ListPlus,
  MessageSquareQuote,
  Paperclip,
  Pencil,
  Trash2,
} from 'lucide-react';
import { useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { ACP_SESSION_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import type { ConversationPromptQueueVm, ConversationQueuedPromptVm } from '@/types';

const DEFAULT_VISIBLE_ITEMS = 3;

export function moveQueueItemIds(
  itemIds: readonly string[],
  activeId: string,
  overId: string,
): string[] {
  const fromIndex = itemIds.indexOf(activeId);
  const toIndex = itemIds.indexOf(overId);
  if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return [...itemIds];
  return arrayMove([...itemIds], fromIndex, toIndex);
}

export interface ConversationPromptQueueProps {
  queue: ConversationPromptQueueVm;
  sessionActive: boolean;
  mutationPending: boolean;
  composerOccupied: boolean;
  attachedAbove?: boolean;
  integratedInfoTab?: boolean;
  onRestore: (itemId: string) => void | Promise<void>;
  onReorder: (orderedItemIds: string[], expectedRevision: number) => void | Promise<void>;
  onUse: (itemId: string) => void | Promise<void>;
  onDelete: (itemId: string) => void | Promise<void>;
}

export function ConversationPromptQueue({
  queue,
  sessionActive,
  mutationPending,
  composerOccupied,
  attachedAbove = false,
  integratedInfoTab = false,
  onRestore,
  onReorder,
  onUse,
  onDelete,
}: ConversationPromptQueueProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(true);
  const [showAll, setShowAll] = useState(false);
  const [optimisticOrder, setOptimisticOrder] = useState<{
    baseRevision: number;
    itemIds: string[];
  } | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const orderedItems = useMemo(() => {
    if (!optimisticOrder || optimisticOrder.baseRevision !== queue.revision) return queue.items;
    const itemsById = new Map(queue.items.map((item) => [item.id, item]));
    const items = optimisticOrder.itemIds.flatMap((id) => {
      const item = itemsById.get(id);
      return item ? [item] : [];
    });
    return items.length === queue.items.length ? items : queue.items;
  }, [optimisticOrder, queue.items, queue.revision]);
  const hasMore = orderedItems.length > DEFAULT_VISIBLE_ITEMS;
  const visibleItems = showAll ? orderedItems : orderedItems.slice(0, DEFAULT_VISIBLE_ITEMS);

  if (queue.items.length === 0) return null;

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || mutationPending) return;
    const currentIds = orderedItems.map((item) => item.id);
    const nextIds = moveQueueItemIds(currentIds, String(active.id), String(over.id));
    if (nextIds.every((id, index) => id === currentIds[index])) return;
    setOptimisticOrder({ baseRevision: queue.revision, itemIds: nextIds });
    void Promise.resolve(onReorder(nextIds, queue.revision)).catch(() => {
      setOptimisticOrder(null);
    });
  };

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className={cn(
        'overflow-hidden bg-card',
        ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
        attachedAbove ? 'rounded-none' : 'rounded-t-2xl',
        integratedInfoTab && !attachedAbove && 'rounded-tl-none',
      )}
      data-testid="conversation-prompt-queue"
    >
      <CollapsibleTrigger asChild>
        <Button
          variant="ghost"
          className="h-auto w-full justify-between rounded-none border-0 px-3 py-2 font-normal shadow-none hover:bg-transparent focus-visible:border-transparent focus-visible:ring-0"
          data-queue-trigger="true"
        >
          <span className="flex min-w-0 items-center gap-2 text-xs">
            <ListPlus className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="text-muted-foreground">{t('acp.promptQueue.title')}</span>
            <span className="truncate font-medium text-foreground">
              {t('acp.promptQueue.summary', { count: queue.items.length, max: queue.maxItems })}
            </span>
          </span>
          <ChevronDown
            className={cn(
              'size-3.5 shrink-0 text-muted-foreground transition-transform',
              open && 'rotate-180',
            )}
          />
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent className="data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down overflow-hidden">
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
          <SortableContext
            items={visibleItems.map((item) => item.id)}
            strategy={verticalListSortingStrategy}
          >
            <div className="px-3 pb-1.5" data-queue-items="true">
              {visibleItems.map((item) => (
                <QueueItem
                  key={item.id}
                  index={orderedItems.findIndex((candidate) => candidate.id === item.id)}
                  item={item}
                  sessionActive={sessionActive}
                  mutationPending={mutationPending}
                  composerOccupied={composerOccupied}
                  onRestore={() => { void onRestore(item.id); }}
                  onUse={() => { void onUse(item.id); }}
                  onDelete={() => { void onDelete(item.id); }}
                />
              ))}
            </div>
          </SortableContext>
        </DndContext>
        {hasMore ? (
          <div className="px-3 py-1" data-queue-show-more-row="true">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 w-full gap-1.5 rounded-md text-xs font-normal text-muted-foreground shadow-none hover:bg-muted/30 hover:text-foreground"
              aria-expanded={showAll}
              onClick={() => setShowAll((current) => !current)}
              data-queue-show-more="true"
            >
              {showAll
                ? t('acp.promptQueue.showFirst', { count: DEFAULT_VISIBLE_ITEMS })
                : t('acp.promptQueue.showMore')}
              <ChevronDown className={cn('size-3.5 transition-transform', showAll && 'rotate-180')} />
            </Button>
          </div>
        ) : null}
      </CollapsibleContent>
    </Collapsible>
  );
}

interface QueueItemProps {
  index: number;
  item: ConversationQueuedPromptVm;
  sessionActive: boolean;
  mutationPending: boolean;
  composerOccupied: boolean;
  onRestore: () => void;
  onUse: () => void;
  onDelete: () => void;
}

function QueueItem({
  index,
  item,
  sessionActive,
  mutationPending,
  composerOccupied,
  onRestore,
  onUse,
  onDelete,
}: QueueItemProps) {
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: item.id, disabled: mutationPending });
  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        'flex min-h-8 min-w-0 items-center gap-1.5 py-1 text-xs',
        isDragging && 'relative z-10 bg-card opacity-80 shadow-sm',
      )}
      data-queue-item-id={item.id}
      data-queue-dragging={isDragging ? 'true' : 'false'}
    >
      <TooltipProvider delayDuration={250}>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              ref={setActivatorNodeRef}
              type="button"
              variant="ghost"
              size="icon"
              className="size-7 shrink-0 cursor-grab touch-none rounded-full text-muted-foreground active:cursor-grabbing"
              disabled={mutationPending}
              {...attributes}
              {...listeners}
              aria-label={t('acp.promptQueue.reorder')}
            >
              <GripVertical className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('acp.promptQueue.reorder')}</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <span className="w-4 shrink-0 text-center text-ui-caption tabular-nums text-muted-foreground">
        {index + 1}
      </span>
      <div className="min-w-0 flex-1">
        <p className="line-clamp-2 whitespace-pre-wrap break-words leading-5 text-foreground/90 [overflow-wrap:anywhere]">
          {item.content}
        </p>
        {item.attachmentCount > 0 || item.quoteCount > 0 ? (
          <div className="mt-0.5 flex items-center gap-2 text-ui-caption text-muted-foreground">
            {item.quoteCount > 0 ? (
              <span className="inline-flex items-center gap-1">
                <MessageSquareQuote className="size-3" />
                {t('acp.userQuoteCount', { count: item.quoteCount })}
              </span>
            ) : null}
            {item.attachmentCount > 0 ? (
              <span className="inline-flex items-center gap-1">
                <Paperclip className="size-3" />
                {item.attachmentCount}
              </span>
            ) : null}
          </div>
        ) : null}
      </div>
      <TooltipProvider delayDuration={250}>
        <div className="flex shrink-0 items-center gap-0.5">
          <QueueIconButton
            label={t('acp.promptQueue.edit')}
            tooltipLabel={composerOccupied ? t('acp.promptQueue.editDraftOccupied') : undefined}
            disabled={mutationPending || composerOccupied}
            onClick={onRestore}
          >
            <Pencil className="size-3.5" />
          </QueueIconButton>
          <QueueIconButton label={t('acp.promptQueue.use')} disabled={sessionActive || mutationPending} onClick={onUse}>
            <CornerDownLeft className="size-3.5" />
          </QueueIconButton>
          <QueueIconButton label={t('acp.promptQueue.delete')} disabled={mutationPending} onClick={onDelete} destructive>
            <Trash2 className="size-3.5" />
          </QueueIconButton>
        </div>
      </TooltipProvider>
    </div>
  );
}

function QueueIconButton({
  label,
  tooltipLabel,
  disabled,
  onClick,
  destructive = false,
  children,
}: {
  label: string;
  tooltipLabel?: string;
  disabled: boolean;
  onClick: () => void;
  destructive?: boolean;
  children: ReactNode;
}) {
  const button = (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn('size-7 rounded-full', destructive && 'text-destructive hover:text-destructive')}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </Button>
  );
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {disabled ? <span className="inline-flex" tabIndex={0}>{button}</span> : button}
      </TooltipTrigger>
      <TooltipContent>{tooltipLabel ?? label}</TooltipContent>
    </Tooltip>
  );
}
