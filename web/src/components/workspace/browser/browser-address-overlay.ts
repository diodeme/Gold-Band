import type {
  BrowserAddressSuggestionOverlayInput,
  BrowserAddressSuggestionOverlayItemVm,
  BrowserAddressSuggestionOverlayThemeVm,
} from '@/api/client';
import { displayBrowserVisitUrl, type BrowserAddressSuggestion } from './browser-history';

export { BROWSER_ADDRESS_SUGGESTION_ACTION_EVENT } from '@/api/client';
export const BROWSER_ADDRESS_SUGGESTION_WEBVIEW_LABEL = 'gb-browser-address-suggestions';
export const BROWSER_ADDRESS_SUGGESTION_ROW_HEIGHT = 32;
export const BROWSER_ADDRESS_SUGGESTION_PADDING = 4;
export const BROWSER_ADDRESS_SUGGESTION_GAP = 4;
export const BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER = 8;

const BROWSER_ADDRESS_SUGGESTION_REVISION_STRIDE = 1_024;
let browserAddressSuggestionRevision = Date.now() * BROWSER_ADDRESS_SUGGESTION_REVISION_STRIDE;

const THEME_VARIABLES = [
  '--popover',
  '--popover-foreground',
  '--foreground',
  '--muted-foreground',
  '--accent',
  '--accent-foreground',
  '--border',
  '--ring',
  '--radius',
  '--gb-theme-ui-font-family',
  '--gold-scrollbar-track',
  '--gold-scrollbar-thumb',
  '--gold-scrollbar-thumb-hover',
] as const;

export function nextBrowserAddressSuggestionRevision() {
  browserAddressSuggestionRevision = Math.max(
    browserAddressSuggestionRevision + BROWSER_ADDRESS_SUGGESTION_REVISION_STRIDE,
    Date.now() * BROWSER_ADDRESS_SUGGESTION_REVISION_STRIDE,
  );
  return browserAddressSuggestionRevision;
}

export function browserAddressSuggestionKey(suggestion: BrowserAddressSuggestion) {
  return suggestion.kind === 'search' ? `search:${suggestion.query}` : `visit:${suggestion.visit.url}`;
}

export function browserAddressSuggestionOverlayItems(
  suggestions: readonly BrowserAddressSuggestion[],
  searchLabel: string,
  removeLabel: string,
): BrowserAddressSuggestionOverlayItemVm[] {
  return suggestions.map((suggestion) => suggestion.kind === 'search'
    ? {
        key: browserAddressSuggestionKey(suggestion),
        kind: 'search',
        title: suggestion.query,
        detail: searchLabel,
        faviconDataUrl: null,
        removeLabel: null,
      }
    : {
        key: browserAddressSuggestionKey(suggestion),
        kind: 'visit',
        title: suggestion.visit.title || displayBrowserVisitUrl(suggestion.visit.url),
        detail: displayBrowserVisitUrl(suggestion.visit.url),
        faviconDataUrl: suggestion.visit.faviconDataUrl ?? null,
        removeLabel,
      });
}

export function browserAddressSuggestionOverlayBounds(
  anchor: Pick<DOMRect, 'left' | 'right' | 'top' | 'bottom' | 'width'>,
  itemCount: number,
  viewportWidth: number,
  viewportHeight: number,
) {
  const idealHeight = itemCount * BROWSER_ADDRESS_SUGGESTION_ROW_HEIGHT
    + BROWSER_ADDRESS_SUGGESTION_PADDING * 2;
  const width = Math.max(1, Math.min(anchor.width, viewportWidth - BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER * 2));
  const x = Math.max(
    BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER,
    Math.min(anchor.left, viewportWidth - width - BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER),
  );
  const belowY = anchor.bottom + BROWSER_ADDRESS_SUGGESTION_GAP;
  const availableBelow = viewportHeight - belowY - BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER;
  const availableAbove = anchor.top - BROWSER_ADDRESS_SUGGESTION_GAP - BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER;
  const placeAbove = availableBelow < idealHeight && availableAbove > availableBelow;
  const availableHeight = Math.max(1, placeAbove ? availableAbove : availableBelow);
  const height = Math.max(1, Math.min(idealHeight, availableHeight));
  const y = placeAbove
    ? Math.max(BROWSER_ADDRESS_SUGGESTION_VIEWPORT_GUTTER, anchor.top - BROWSER_ADDRESS_SUGGESTION_GAP - height)
    : belowY;
  return { x, y, width, height };
}

export function readBrowserAddressSuggestionTheme(): BrowserAddressSuggestionOverlayThemeVm {
  const root = document.documentElement;
  const computed = getComputedStyle(root);
  return {
    dark: root.classList.contains('dark'),
    themeId: root.dataset.theme ?? '',
    colorScheme: root.dataset.colorScheme ?? '',
    visualQuality: root.dataset.visualQuality ?? '',
    materialModel: root.dataset.materialModel ?? '',
    variables: Object.fromEntries(THEME_VARIABLES.map((name) => [name, computed.getPropertyValue(name).trim()])),
  };
}

export function createBrowserAddressSuggestionOverlayInput({
  revision,
  anchor,
  suggestions,
  activeIndex,
  searchLabel,
  removeLabel,
}: {
  revision: number;
  anchor: DOMRect;
  suggestions: readonly BrowserAddressSuggestion[];
  activeIndex: number | null;
  searchLabel: string;
  removeLabel: string;
}): BrowserAddressSuggestionOverlayInput {
  return {
    revision,
    bounds: browserAddressSuggestionOverlayBounds(
      anchor,
      suggestions.length,
      window.innerWidth,
      window.innerHeight,
    ),
    activeIndex,
    items: browserAddressSuggestionOverlayItems(suggestions, searchLabel, removeLabel),
    theme: readBrowserAddressSuggestionTheme(),
  };
}
