import { isBrowserAddressQuery } from './web-target';

export const BROWSER_HISTORY_LIMIT = 200;
export const BROWSER_HISTORY_SUGGESTION_LIMIT = 8;

export interface BrowserVisit {
  url: string;
  title: string;
  origin: string;
  lastVisitedAt: number;
  faviconDataUrl?: string | null;
}

export type BrowserAddressSuggestion =
  | { kind: 'search'; query: string }
  | { kind: 'visit'; visit: BrowserVisit };

export function browserVisitOrigin(url: string) {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return '';
    return parsed.origin;
  } catch {
    return '';
  }
}

export function displayBrowserVisitUrl(url: string) {
  try {
    const parsed = new URL(url);
    return `${parsed.host}${parsed.pathname}${parsed.search}${parsed.hash}`.replace(/\/$/u, '') || parsed.host;
  } catch {
    return url;
  }
}

function visitMatches(visit: BrowserVisit, needle: string) {
  return visit.title.toLowerCase().includes(needle)
    || visit.url.toLowerCase().includes(needle)
    || visit.origin.toLowerCase().includes(needle)
    || displayBrowserVisitUrl(visit.url).toLowerCase().includes(needle);
}

export function faviconForUrl(visits: readonly BrowserVisit[], url: string) {
  const origin = browserVisitOrigin(url);
  if (!origin) return null;
  return visits.find((visit) => visit.origin === origin && visit.faviconDataUrl)?.faviconDataUrl ?? null;
}

export function overlayCoverTop(overlayBottom: number, viewportTop: number) {
  return Math.max(0, Math.round(overlayBottom - viewportTop));
}

export function browserAddressSuggestions(
  visits: readonly BrowserVisit[],
  query: string,
  typed: boolean,
): BrowserAddressSuggestion[] {
  const trimmed = query.trim();
  const showRecents = !typed || trimmed.length === 0;
  const matched = (showRecents
    ? visits
    : visits.filter((visit) => visitMatches(visit, trimmed.toLowerCase()))
  ).slice(0, BROWSER_HISTORY_SUGGESTION_LIMIT);
  const suggestions: BrowserAddressSuggestion[] = matched.map((visit) => ({ kind: 'visit', visit }));
  if (!showRecents && trimmed && !isBrowserAddressQuery(trimmed)) {
    suggestions.unshift({ kind: 'search', query: trimmed });
  }
  return suggestions;
}
