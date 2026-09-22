import {
  closestCenter,
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  rectSortingStrategy,
  SortableContext,
  useSortable,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { FilePlus, X } from 'lucide-react';
import { useMemo, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { BrowserFavicon } from './BrowserFavicon';
import {
  moveBookmarkIds,
  type BrowserBookmark,
} from './browser-bookmarks';
import { displayBrowserVisitUrl, faviconForUrl } from './browser-history';
import type { BrowserVisit } from './browser-history';

export function BrowserPortal({
  bookmarks,
  visits,
  onOpenBlank,
  onOpenBookmark,
  onDeleteBookmark,
  onReorder,
}: {
  bookmarks: readonly BrowserBookmark[];
  visits: readonly BrowserVisit[];
  onOpenBlank: () => void;
  onOpenBookmark: (url: string) => void;
  onDeleteBookmark: (bookmarkId: string) => void;
  onReorder: (orderedIds: string[]) => void;
}) {
  const { t } = useTranslation();
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 8 } }));
  const bookmarkIds = useMemo(() => bookmarks.map((bookmark) => bookmark.bookmarkId), [bookmarks]);

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over) return;
    const nextIds = moveBookmarkIds(bookmarkIds, String(active.id), String(over.id));
    if (nextIds.every((id, index) => id === bookmarkIds[index])) return;
    onReorder(nextIds);
  };

  return (
    <div className="gold-themed-scrollbar min-h-0 flex-1 overflow-y-auto" data-browser-portal="true">
      <div className="mx-auto grid w-full max-w-3xl grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-2 p-6">
        <button
          type="button"
          className="flex min-h-[5.5rem] flex-col items-start gap-1.5 rounded-xl bg-muted/25 p-3 text-left hover:bg-muted/40"
          data-browser-open-blank="true"
          onClick={onOpenBlank}
        >
          <FilePlus className="size-5 text-muted-foreground" />
          <span className="text-sm font-medium text-foreground">{t('workspace.browser.openBlankPage')}</span>
        </button>
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
          <SortableContext items={bookmarkIds} strategy={rectSortingStrategy}>
            {bookmarks.map((bookmark) => (
              <SortableBookmarkCard
                key={bookmark.bookmarkId}
                bookmark={bookmark}
                faviconSrc={bookmark.faviconDataUrl ?? faviconForUrl(visits, bookmark.url)}
                onOpen={() => onOpenBookmark(bookmark.url)}
                onDelete={() => onDeleteBookmark(bookmark.bookmarkId)}
              />
            ))}
          </SortableContext>
        </DndContext>
      </div>
    </div>
  );
}

function SortableBookmarkCard({
  bookmark,
  faviconSrc,
  onOpen,
  onDelete,
}: {
  bookmark: BrowserBookmark;
  faviconSrc?: string | null;
  onOpen: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: bookmark.bookmarkId });
  const style: CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };
  const displayUrl = displayBrowserVisitUrl(bookmark.url);

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        'group relative flex min-h-[5.5rem] flex-col items-start gap-1.5 rounded-xl bg-muted/25 p-3 text-left hover:bg-muted/40',
        isDragging && 'z-10 opacity-80 shadow-sm',
      )}
      data-browser-bookmark={bookmark.bookmarkId}
      {...attributes}
      {...listeners}
    >
      <button type="button" className="flex min-w-0 flex-1 flex-col items-start gap-1.5 self-stretch text-left outline-none" onClick={onOpen}>
        <BrowserFavicon src={faviconSrc} className="size-5" />
        <span className="w-full truncate text-sm font-medium text-foreground">{bookmark.title}</span>
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="w-full truncate text-[11px] text-muted-foreground">{displayUrl}</span>
          </TooltipTrigger>
          <TooltipContent>{bookmark.url}</TooltipContent>
        </Tooltip>
      </button>
      <Button
        type="button"
        size="icon-xs"
        variant="ghost"
        className="absolute top-1.5 right-1.5 opacity-0 group-hover:opacity-100"
        aria-label={t('workspace.browser.deleteBookmark')}
        data-browser-delete-bookmark="true"
        onClick={(event) => {
          event.stopPropagation();
          onDelete();
        }}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <X />
      </Button>
    </div>
  );
}
