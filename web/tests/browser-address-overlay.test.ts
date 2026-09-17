import { describe, expect, it } from 'vitest';
import {
  BROWSER_ADDRESS_SUGGESTION_ROW_HEIGHT,
  browserAddressSuggestionOverlayBounds,
  browserAddressSuggestionOverlayItems,
  nextBrowserAddressSuggestionRevision,
} from '@/components/workspace/browser/browser-address-overlay';
import type { BrowserVisit } from '@/components/workspace/browser/browser-history';

const visit: BrowserVisit = {
  url: 'https://github.com/diodeme/Gold-Band',
  title: 'Gold Band · GitHub',
  origin: 'https://github.com',
  lastVisitedAt: 1,
  faviconDataUrl: 'data:image/png;base64,aaa',
};

describe('browser address native overlay', () => {
  it('allocates revisions monotonically across address-field remounts', () => {
    const firstMountRevision = nextBrowserAddressSuggestionRevision();
    const secondMountRevision = nextBrowserAddressSuggestionRevision();

    expect(secondMountRevision).toBeGreaterThan(firstMountRevision);
  });

  it('anchors below the address field without changing the page viewport', () => {
    expect(browserAddressSuggestionOverlayBounds(
      { left: 100, right: 600, top: 50, bottom: 78, width: 500 },
      8,
      1_000,
      700,
    )).toEqual({ x: 100, y: 82, width: 500, height: BROWSER_ADDRESS_SUGGESTION_ROW_HEIGHT * 8 + 8 });
  });

  it('flips above and clamps to the application viewport near the bottom edge', () => {
    expect(browserAddressSuggestionOverlayBounds(
      { left: -20, right: 980, top: 650, bottom: 678, width: 1_000 },
      8,
      600,
      700,
    )).toEqual({ x: 8, y: 382, width: 584, height: 264 });
  });

  it('projects only the bounded display fields required by the overlay', () => {
    expect(browserAddressSuggestionOverlayItems(
      [{ kind: 'search', query: 'gold band' }, { kind: 'visit', visit }],
      '搜索网页',
      '删除访问记录',
    )).toEqual([
      {
        key: 'search:gold band',
        kind: 'search',
        title: 'gold band',
        detail: '搜索网页',
        faviconDataUrl: null,
        removeLabel: null,
      },
      {
        key: `visit:${visit.url}`,
        kind: 'visit',
        title: visit.title,
        detail: 'github.com/diodeme/Gold-Band',
        faviconDataUrl: visit.faviconDataUrl,
        removeLabel: '删除访问记录',
      },
    ]);
  });
});
