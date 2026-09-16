import { arrayMove } from '@dnd-kit/sortable';
import { browserVisitOrigin } from './browser-history';

export const BROWSER_BOOKMARK_LIMIT = 32;

export interface BrowserBookmark {
  bookmarkId: string;
  url: string;
  title: string;
  origin: string;
  faviconDataUrl?: string | null;
}

export function bookmarkOrigin(url: string) {
  return browserVisitOrigin(url);
}

export function isUrlBookmarked(bookmarks: readonly BrowserBookmark[], url: string) {
  const origin = bookmarkOrigin(url);
  return origin !== '' && bookmarks.some((bookmark) => bookmark.origin === origin);
}

export function bookmarkByOrigin(bookmarks: readonly BrowserBookmark[], url: string) {
  const origin = bookmarkOrigin(url);
  if (!origin) return null;
  return bookmarks.find((bookmark) => bookmark.origin === origin) ?? null;
}

export function moveBookmarkIds(
  itemIds: readonly string[],
  activeId: string,
  overId: string,
): string[] {
  const fromIndex = itemIds.indexOf(activeId);
  const toIndex = itemIds.indexOf(overId);
  if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return [...itemIds];
  return arrayMove([...itemIds], fromIndex, toIndex);
}
