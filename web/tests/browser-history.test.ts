import { describe, expect, it } from 'vitest';
import {
  browserAddressSuggestions,
  browserVisitOrigin,
  displayBrowserVisitUrl,
  faviconForUrl,
  overlayCoverTop,
  type BrowserVisit,
} from '@/components/workspace/browser/browser-history';

const github: BrowserVisit = {
  url: 'https://github.com/diodeme/Gold-Band/pull/new/codex/website-demo-final',
  title: 'Sign in to GitHub · GitHub',
  origin: 'https://github.com',
  lastVisitedAt: 3,
  faviconDataUrl: 'data:image/png;base64,aaa',
};
const local: BrowserVisit = {
  url: 'http://localhost:3000/zh#before',
  title: 'Gold Band',
  origin: 'http://localhost:3000',
  lastVisitedAt: 2,
};
const baidu: BrowserVisit = {
  url: 'https://www.baidu.com/',
  title: '百度一下，你就知道',
  origin: 'https://www.baidu.com',
  lastVisitedAt: 1,
};

describe('browser address history', () => {
  it('shows recent visits when the address bar has no typed query', () => {
    const suggestions = browserAddressSuggestions([github, local, baidu], '', false);
    expect(suggestions.map((item) => item.kind === 'visit' ? item.visit.url : item.query)).toEqual([
      github.url,
      local.url,
      baidu.url,
    ]);
    expect(browserAddressSuggestions([github], github.url, false)).toEqual(suggestions.slice(0, 1));
  });

  it('filters visits and adds a search row for ordinary words', () => {
    const suggestions = browserAddressSuggestions([github, local, baidu], 'git', true);
    expect(suggestions[0]).toEqual({ kind: 'search', query: 'git' });
    expect(suggestions.slice(1)).toEqual([{ kind: 'visit', visit: github }]);
  });

  it('does not add a search row for typed addresses', () => {
    const suggestions = browserAddressSuggestions([github, local], 'github.com', true);
    expect(suggestions).toEqual([{ kind: 'visit', visit: github }]);
  });

  it('reuses an origin favicon for any page on that site', () => {
    expect(faviconForUrl([github, local], 'https://github.com/gold-band')).toBe('data:image/png;base64,aaa');
    expect(faviconForUrl([github, local], 'http://localhost:3000/other')).toBeNull();
  });

  it('only insets the native page by the overlay that actually covers it', () => {
    expect(overlayCoverTop(120, 200)).toBe(0);
    expect(overlayCoverTop(260, 200)).toBe(60);
  });

  it('derives origin and compact url for the suggestion row', () => {
    expect(browserVisitOrigin(github.url)).toBe('https://github.com');
    expect(displayBrowserVisitUrl(github.url)).toBe('github.com/diodeme/Gold-Band/pull/new/codex/website-demo-final');
    expect(browserVisitOrigin('about:blank')).toBe('');
  });
});
