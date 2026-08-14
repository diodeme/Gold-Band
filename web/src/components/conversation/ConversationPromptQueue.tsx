import {
  Check,
  ChevronDown,
  CornerDownLeft,
  Paperclip,
  Pencil,
  MessageSquareQuote,
  Trash2,
  X,
} from 'lucide-react';
import { useEffect, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from '@/components/ui/collapsible';
import { Textarea } from '@/components/ui/textarea';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { ConversationPromptQueueVm } from '@/types';

const DEFAULT_VISIBLE_ITEMS = 3;

export interface ConversationPromptQueueProps {
  queue: ConversationPromptQueueVm;
  sessionActive: boolean;
  mutationPending: boolean;
  attachedAbove?: boolean;
  integratedInfoTab?: boolean;
  onEdit: (itemId: string, content: string) => void | Promise<void>;
  onUse: (itemId: string) => void | Promise<void>;
  onDelete: (itemId: string) => void | Promise<void>;
}

export function ConversationPromptQueue({
  queue,
  sessionActive,
  mutationPending,
  attachedAbove = false,
  integratedInfoTab = false,
  onEdit,
  onUse,
  onDelete,
}: ConversationPromptQueueProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const hasMore = queue.items.length > DEFAULT_VISIBLE_ITEMS;
  const visibleItems = expanded ? queue.items : queue.items.slice(0, DEFAULT_VISIBLE_ITEMS);

  useEffect(() => {
    if (!hasMore) setExpanded(false);
    if (editingId && !queue.items.some((item) => item.id === editingId)) {
      setEditingId(null);
      setDraft('');
    }
  }, [editingId, hasMore, queue.items]);

  if (queue.items.length === 0) return null;

  const startEdit = (itemId: string, content: string) => {
    setEditingId(itemId);
    setDraft(content);
  };
  const cancelEdit = () => {
    setEditingId(null);
    setDraft('');
  };
  const saveEdit = async () => {
    const content = draft.trim();
    if (!editingId || !content || mutationPending) return;
    await onEdit(editingId, content);
    cancelEdit();
  };

  return (
    <Collapsible
      open={expanded}
      onOpenChange={setExpanded}
      className={cn(
        'overflow-hidden border border-b-0 border-border bg-muted/35',
        attachedAbove ? 'rounded-none' : 'rounded-t-2xl',
        integratedInfoTab && !attachedAbove && 'rounded-tl-none bg-card',
      )}
      data-testid="conversation-prompt-queue"
    >
      <div className="flex items-center justify-between gap-3 px-3 py-2 text-xs text-muted-foreground">
        <span className="font-medium text-foreground/80">
          {t('acp.promptQueue.title', { count: queue.items.length, max: queue.maxItems })}
        </span>
        {hasMore ? (
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="sm" className="h-6 gap-1 px-2 text-xs">
              {expanded ? t('acp.promptQueue.collapse') : t('acp.promptQueue.showMore')}
              <ChevronDown className={cn('size-3 transition-transform', expanded && 'rotate-180')} />
            </Button>
          </CollapsibleTrigger>
        ) : null}
      </div>
      <div className="divide-y divide-border/50 border-t border-border/50">
        {visibleItems.slice(0, DEFAULT_VISIBLE_ITEMS).map((item, index) => (
          <QueueItem
            key={item.id}
            index={index}
            item={item}
            editing={editingId === item.id}
            draft={draft}
            sessionActive={sessionActive}
            mutationPending={mutationPending}
            onDraftChange={setDraft}
            onEdit={() => startEdit(item.id, item.content)}
            onCancel={cancelEdit}
            onSave={() => { void saveEdit(); }}
            onUse={() => { void onUse(item.id); }}
            onDelete={() => { void onDelete(item.id); }}
          />
        ))}
        {hasMore ? (
          <CollapsibleContent>
            {visibleItems.slice(DEFAULT_VISIBLE_ITEMS).map((item, index) => (
              <QueueItem
                key={item.id}
                index={index + DEFAULT_VISIBLE_ITEMS}
                item={item}
                editing={editingId === item.id}
                draft={draft}
                sessionActive={sessionActive}
                mutationPending={mutationPending}
                onDraftChange={setDraft}
                onEdit={() => startEdit(item.id, item.content)}
                onCancel={cancelEdit}
                onSave={() => { void saveEdit(); }}
                onUse={() => { void onUse(item.id); }}
                onDelete={() => { void onDelete(item.id); }}
              />
            ))}
          </CollapsibleContent>
        ) : null}
      </div>
    </Collapsible>
  );
}

interface QueueItemProps {
  index: number;
  item: ConversationPromptQueueVm['items'][number];
  editing: boolean;
  draft: string;
  sessionActive: boolean;
  mutationPending: boolean;
  onDraftChange: (value: string) => void;
  onEdit: () => void;
  onCancel: () => void;
  onSave: () => void;
  onUse: () => void;
  onDelete: () => void;
}

function QueueItem({
  index,
  item,
  editing,
  draft,
  sessionActive,
  mutationPending,
  onDraftChange,
  onEdit,
  onCancel,
  onSave,
  onUse,
  onDelete,
}: QueueItemProps) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-10 items-center gap-2 px-3 py-1.5" data-queue-item-id={item.id}>
      <span className="w-4 shrink-0 text-center text-ui-caption tabular-nums text-muted-foreground">
        {index + 1}
      </span>
      {editing ? (
        <Textarea
          autoFocus
          value={draft}
          onChange={(event) => onDraftChange(event.target.value)}
          className="min-h-9 resize-none bg-background/70 py-1.5 text-sm"
          aria-label={t('acp.promptQueue.editInput')}
        />
      ) : (
        <div className="min-w-0 flex-1">
          <p className="line-clamp-2 whitespace-pre-wrap break-words text-sm leading-5 text-foreground/90">
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
      )}
      <TooltipProvider delayDuration={250}>
        <div className="flex shrink-0 items-center gap-0.5">
          {editing ? (
            <>
              <QueueIconButton label={t('acp.promptQueue.save')} disabled={!draft.trim() || mutationPending} onClick={onSave}>
                <Check className="size-3.5" />
              </QueueIconButton>
              <QueueIconButton label={t('acp.promptQueue.cancel')} disabled={mutationPending} onClick={onCancel}>
                <X className="size-3.5" />
              </QueueIconButton>
            </>
          ) : (
            <>
              <QueueIconButton label={t('acp.promptQueue.edit')} disabled={mutationPending} onClick={onEdit}>
                <Pencil className="size-3.5" />
              </QueueIconButton>
              <QueueIconButton label={t('acp.promptQueue.use')} disabled={sessionActive || mutationPending} onClick={onUse}>
                <CornerDownLeft className="size-3.5" />
              </QueueIconButton>
              <QueueIconButton label={t('acp.promptQueue.delete')} disabled={mutationPending} onClick={onDelete} destructive>
                <Trash2 className="size-3.5" />
              </QueueIconButton>
            </>
          )}
        </div>
      </TooltipProvider>
    </div>
  );
}

function QueueIconButton({
  label,
  disabled,
  onClick,
  destructive = false,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  destructive?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
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
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
