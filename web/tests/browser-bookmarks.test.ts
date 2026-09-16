import { describe, expect, it } from 'vitest';
import {
  bookmarkByOrigin,
  isUrlBookmarked,
  moveBookmarkIds,
  type BrowserBookmark,
} from '@/components/workspace/browser/browser-bookmarks';

const github: BrowserBookmark = {
  bookmarkId: 'github',
  url: 'https://github.com/',
  title: 'GitHub',
  origin: 'https://github.com',
};
const figma: BrowserBookmark = {
  bookmarkId: 'figma',
  url: 'https://www.figma.com/',
  title: 'Figma',
  origin: 'https://www.figma.com',
};

describe('browser bookmarks', () => {
  it('treats any path on the same origin as already bookmarked', () => {
    expect(isUrlBookmarked([github, figma], 'https://github.com/gold-band')).toBe(true);
    expect(isUrlBookmarked([github, figma], 'https://www.figma.com/files')).toBe(true);
    expect(isUrlBookmarked([github], 'https://example.com/')).toBe(false);
    expect(isUrlBookmarked([github], 'about:blank')).toBe(false);
    expect(bookmarkByOrigin([github], 'https://github.com/settings')?.bookmarkId).toBe('github');
  });

  it('reorders by stable bookmark ids', () => {
    expect(moveBookmarkIds(['github', 'figma', 'mdn'], 'github', 'mdn')).toEqual(['figma', 'mdn', 'github']);
    expect(moveBookmarkIds(['github', 'figma'], 'github', 'github')).toEqual(['github', 'figma']);
    expect(moveBookmarkIds(['github', 'figma'], 'missing', 'figma')).toEqual(['github', 'figma']);
  });
});
